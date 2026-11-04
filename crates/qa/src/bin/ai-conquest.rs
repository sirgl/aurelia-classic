//! What a captured city is worth under the rules, and what the war that took it cost
//! (classic/docs/ai-conquest-payoff.md). A measurement tool only: it changes no rule and no AI.
//!
//! `cargo run --release -p aurelia-qa --bin ai-conquest -- [options]`
//!
//! --candidate X / --baseline Y   the sides (default `standard` both); --solo X: every seat X
//! --suite NAME                   a bench suite (default duel)
//! --speed NAME                   a game speed (default quick)
//! --offset K                     the suite's seeds + 1000·K (default 0)
//! --jobs N                       games in parallel (default 4)
//! --limit N                      only the suite's first N matches
//! --every N                      a player row every N rounds (default 1)
//! --out PREFIX                   PREFIX-players.csv (per player and round), PREFIX-captures.csv
//!                                (every city that changed hands), PREFIX-cities.csv (every city
//!                                held by a player that did not found it, per round)
//! --gift ROUND                   twin games: each match is played once as it is and once per
//!                                major E: at E's turn of ROUND, E gives its best non-capital city
//!                                to the next major P by a deal (both commands issued by this tool),
//!                                then the city is made what a capture makes of it (`--as`); the
//!                                twins share everything before ROUND (the games are deterministic);
//!                                at ROUND every twin, the plain one too, makes the majors meet
//! --as ai|puppet|annex|own|decide
//!                                what the gifted city becomes: the AI's own decision after a
//!                                capture (annexed at 4+ citizens after the loss, else a puppet;
//!                                default), a puppet, an annexed city, a city of its own with no
//!                                citizen lost (a deal's city), or a city awaiting the decision
//!                                that the taker's variant makes at the end of its next turn
//!                                (`ai::conquest`: `--solo puppet-price` prices it)
//! --pick best|small              which city E gives: its largest non-capital city (default) or
//!                                its smallest
//!
//! PREFIX-decisions.csv lists every captured city a `puppet-price` seat priced
//! (`ai::conquest::priced_case`): its citizens, the excess happiness before it, the three
//! keepings in hundredths of score points and the decision.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use aurelia_qa::bench::modules::{majors, standing, BenchRules, ClassicBench};
use aurelia_qa::bench::suite::{Match, Mode, Side, Suite};
use classic_rules::world::city::CityStatus;
use classic_rules::world::relations::DealItem;
use classic_rules::Command;
use kernel::ai::driver::AiDriver;
use kernel::ai::seats::Seated;
use kernel::ai::{DriverPhase, StepOutcome};
use kernel::ids::{CityHandle, PlayerId};
use kernel::transaction::journal::{CityChange, PlayerChange, UnitChange};

type R = ClassicBench;
type M = <ClassicBench as BenchRules>::M;
type Game = kernel::world::state::GameState<M>;
type Rules = kernel::rules::CompiledRuleset<M>;

#[derive(Clone)]
struct Options {
    candidate: String,
    baseline: String,
    solo: bool,
    suite: String,
    speed: String,
    offset: u64,
    jobs: usize,
    every: u32,
    out: String,
    gift: Option<u32>,
    gift_as: String,
    pick: String,
    limit: usize,
}

fn options() -> Result<Options, String> {
    let mut o = Options {
        candidate: "standard".into(),
        baseline: "standard".into(),
        solo: false,
        suite: "duel".into(),
        speed: "quick".into(),
        offset: 0,
        jobs: 4,
        every: 1,
        out: "conquest".into(),
        gift: None,
        gift_as: "ai".into(),
        pick: "best".into(),
        limit: usize::MAX,
    };
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--candidate" => o.candidate = value()?,
            "--baseline" => o.baseline = value()?,
            "--solo" => {
                let v = value()?;
                o.candidate = v.clone();
                o.baseline = v;
                o.solo = true;
            }
            "--suite" => o.suite = value()?,
            "--speed" => o.speed = value()?,
            "--offset" => o.offset = value()?.parse().map_err(|e| format!("{e}"))?,
            "--jobs" => o.jobs = value()?.parse().map_err(|e| format!("{e}"))?,
            "--every" => o.every = value()?.parse().map_err(|e| format!("{e}"))?,
            "--out" => o.out = value()?,
            "--gift" => o.gift = Some(value()?.parse().map_err(|e| format!("{e}"))?),
            "--as" => o.gift_as = value()?,
            "--pick" => o.pick = value()?,
            "--limit" => o.limit = value()?.parse().map_err(|e| format!("{e}"))?,
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(o)
}

fn main() {
    if let Err(e) = options().and_then(|o| run(&o)) {
        eprintln!("ai-conquest: {e}");
        std::process::exit(1);
    }
}

/// One game to play: a match, and the gift of a twin (None: the game as it is).
#[derive(Clone, Copy)]
struct Job {
    game: Match,
    /// (giver's index among the majors) for a gift twin.
    giver: Option<usize>,
}

#[derive(Default)]
struct Out {
    players: String,
    captures: String,
    cities: String,
    decisions: String,
}

fn run(o: &Options) -> Result<(), String> {
    let rules = R::rules();
    let suite = Suite::named(&o.suite).ok_or("unknown suite")?;
    let def = rules.speed_named(&o.speed).ok_or_else(|| format!("unknown speed {}", o.speed))?;
    let suite = suite.at_speed(&def.key, &def.values);
    let mode = if o.solo { Mode::Solo } else { Mode::Versus };
    let matches: Vec<Match> = suite.matches(mode, o.offset).into_iter().take(o.limit).collect();
    let mut jobs = Vec::new();
    for m in &matches {
        jobs.push(Job { game: *m, giver: None });
        if o.gift.is_some() {
            // The number of majors is known only from the game; duels and the standard suite's
            // maps are probed once here.
            let probe = R::new_game_at(&rules, &m.map, m.seed, suite.rounds, &suite.speed);
            for g in 0..majors(&probe).len() {
                jobs.push(Job { game: *m, giver: Some(g) });
            }
        }
    }
    let next = AtomicUsize::new(0);
    let outs: Mutex<Vec<(usize, Out)>> = Mutex::new(Vec::new());
    let started = std::time::Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..o.jobs.clamp(1, jobs.len().max(1)) {
            scope.spawn(|| {
                let rules = R::rules();
                loop {
                    let at = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = jobs.get(at) else { break };
                    match play(o, &rules, &suite, job) {
                        Ok(out) => {
                            eprintln!("{} gift {:?} done, {:.0} s", job.game.label(), job.giver, started.elapsed().as_secs_f64());
                            outs.lock().expect("outs").push((at, out));
                        }
                        Err(e) => eprintln!("{} gift {:?}: {e}", job.game.label(), job.giver),
                    }
                }
            });
        }
    });
    let mut outs = outs.into_inner().map_err(|e| e.to_string())?;
    outs.sort_by_key(|(at, _)| *at);
    let mut players = String::from("game,seed,twin,round,player,side,score,cities,population,techs,military,science,production,gold,culture,happiness,treasury,units_lost,unit_hammers_lost,military_built,military_hammers,kills,captured,lost,foreign_cities,foreign_pop,foreign_production,foreign_science,foreign_gold,foreign_unhappiness,at_war,victory\n");
    let mut captures = String::from("game,seed,twin,round,captor,victim,city,how,pop_before,pop_after,status,original_owner,original_capital,buildings_before,buildings_after\n");
    let mut cities = String::from("game,seed,twin,round,city,owner,original_owner,status,population,production,science,gold,culture,food_surplus,unhappiness\n");
    let mut decisions = String::from("game,seed,twin,round,player,tile,population,excess,puppet,annex,raze,decision\n");
    for (_, out) in &outs {
        players.push_str(&out.players);
        captures.push_str(&out.captures);
        cities.push_str(&out.cities);
        decisions.push_str(&out.decisions);
    }
    for (name, text) in [("players", players), ("captures", captures), ("cities", cities), ("decisions", decisions)] {
        let path = format!("{}-{name}.csv", o.out);
        std::fs::File::create(&path).and_then(|mut f| f.write_all(text.as_bytes())).map_err(|e| format!("{path}: {e}"))?;
        println!("wrote {path}");
    }
    println!("{} games, {:.0} s", jobs.len(), started.elapsed().as_secs_f64());
    Ok(())
}

/// Running counts of one player over the game.
#[derive(Default, Clone, Copy)]
struct Tally {
    units_lost: u32,
    hammers_lost: u32,
    military_built: u32,
    military_hammers: u32,
    kills: u32,
    captured: u32,
    lost: u32,
}

fn play(o: &Options, rules: &Rules, suite: &Suite, job: &Job) -> Result<Out, String> {
    let m = &job.game;
    let mut game = R::new_game_at(rules, &m.map, m.seed, suite.rounds, &suite.speed);
    let all = majors(&game);
    let seats: Vec<(PlayerId, Side)> = all.iter().enumerate().map(|(i, p)| (*p, m.seating.side(i))).collect();
    let mind = |name: &str| R::mind(name).ok_or_else(|| format!("no variant {name}"));
    let mut candidates = Vec::new();
    for (p, side) in &seats {
        if *side == Side::Candidate {
            candidates.push((*p, mind(&o.candidate)?));
        }
    }
    let mut driver = AiDriver::with_mind(R::settings(rules), Vec::new(), Seated::new(mind(&o.baseline)?, candidates));
    driver.set_trace_capacity(0);
    driver.record_journal();

    let twin = match job.giver {
        None => "plain".to_owned(),
        Some(g) => format!("gift{g}"),
    };
    let label = m.label();
    let gift = job.giver.zip(o.gift).map(|(g, round)| (all[g], all[(g + 1) % all.len()], round));
    let mut gifted = false;
    let mut out = Out::default();
    let mut tally: HashMap<PlayerId, Tally> = HashMap::new();
    let mut known_units: HashSet<u64> = HashSet::new();
    let mut pops: HashMap<CityHandle, (u32, usize)> = HashMap::new();
    let mut wars: HashSet<(u8, u8)> = HashSet::new();
    let side_name = |p: PlayerId| seats.iter().find(|(q, _)| *q == p).map_or("?", |(_, s)| if *s == Side::Candidate { "candidate" } else { "baseline" });

    snapshot_cities(&game, &mut pops);
    count_new_units(&game, rules, &mut known_units, &mut tally);
    classic_rules::ai::war_probe::enable(true);
    while !game.is_over() && game.view().auth.turn().round <= suite.rounds {
        let round = game.view().auth.turn().round;
        if o.gift == Some(round) {
            // Every twin, the plain one too: the majors meet (a deal needs it), so the twins
            // differ by the gift alone.
            for (i, a) in all.iter().enumerate() {
                for b in &all[i + 1..] {
                    game.debug_meet(*a, *b);
                }
            }
        }
        loop {
            if driver.phase() == DriverPhase::Observe {
                if let Some((giver, taker, at)) = gift {
                    if !gifted && round >= at && game.view().auth.active_player() == giver {
                        gifted = true;
                        match give_city(o, &mut game, rules, giver, taker) {
                            Ok((city, before, after, status)) => {
                                let buildings = game.view().auth.cities().get(city).map_or(0, |c| c.buildings.len());
                                out.captures.push_str(&format!("{label},{},{twin},{round},{},{},{},gift,{before},{after},{status:?},{},{},{buildings},{buildings}\n", m.seed, taker.0, giver.0, city_id(&game, city), giver.0, false));
                            }
                            Err(e) => {
                                out.captures.push_str(&format!("{label},{},{twin},{round},{},{},-1,gift-failed:{},0,0,-,0,false,0,0\n", m.seed, taker.0, giver.0, e.replace(',', ";")));
                            }
                        }
                        snapshot_cities(&game, &mut pops);
                    }
                }
            }
            match driver.step(&mut game, rules) {
                StepOutcome::Worked => {}
                StepOutcome::PlayerDone(player) => {
                    let journal = driver.take_journal();
                    let mut taken = Vec::new();
                    for applied in &journal {
                        let combat = matches!(applied.command, Command::AttackCity { .. });
                        for change in &applied.changes.unit_changes {
                            if let UnitChange::Destroyed { owner, kind, .. } = change {
                                let t = tally.entry(*owner).or_default();
                                t.units_lost += 1;
                                t.hammers_lost += rules.units.get(kind.0 as usize).map_or(0, |u| u.cost);
                                if *owner != player {
                                    tally.entry(player).or_default().kills += 1;
                                }
                            }
                        }
                        for change in &applied.changes.city_changes {
                            if let CityChange::OwnerChanged { city, from, to } = change {
                                taken.push((*city, *from, *to, if combat { "combat" } else { "other" }));
                            }
                        }
                        for change in &applied.changes.player_changes {
                            match *change {
                                PlayerChange::WarDeclared { player: a, enemy: b } => {
                                    wars.insert((a.0.min(b.0), a.0.max(b.0)));
                                }
                                PlayerChange::PeaceMade { player: a, enemy: b } => {
                                    wars.remove(&(a.0.min(b.0), a.0.max(b.0)));
                                }
                                _ => {}
                            }
                        }
                    }
                    for (city, from, to, how) in taken {
                        tally.entry(to).or_default().captured += 1;
                        tally.entry(from).or_default().lost += 1;
                        let (before, buildings_before) = pops.get(&city).copied().unwrap_or((0, 0));
                        let auth = game.view().auth;
                        if let Some(core) = auth.cities().get(city) {
                            out.captures.push_str(&format!(
                                "{label},{},{twin},{round},{},{},{},{how},{before},{},{:?},{},{},{buildings_before},{}\n",
                                m.seed,
                                to.0,
                                from.0,
                                core.id.0,
                                core.population,
                                core.status,
                                core.original_owner.0,
                                core.original_capital,
                                core.buildings.len()
                            ));
                        } else {
                            out.captures.push_str(&format!("{label},{},{twin},{round},{},{},-1,{how},{before},0,Razed,0,false,{buildings_before},0\n", m.seed, to.0, from.0));
                        }
                    }
                    snapshot_cities(&game, &mut pops);
                }
                StepOutcome::RoundDone => break,
                StepOutcome::Waiting if game.is_over() => break,
                StepOutcome::Waiting => return Err(format!("round {round} waits for a human")),
            }
        }
        count_new_units(&game, rules, &mut known_units, &mut tally);
        let last = game.is_over() || game.view().auth.turn().round > suite.rounds;
        if round % o.every.max(1) == 0 || last {
            let victory = R::victory(&game);
            for &p in &all {
                let row = player_row(&game, rules, p, tally.get(&p).copied().unwrap_or_default(), &wars);
                let won = victory.filter(|(w, _)| *w == p).map_or("", |(_, k)| k);
                out.players.push_str(&format!("{label},{},{twin},{round},{},{},{row},{won}\n", m.seed, p.0, side_name(p)));
            }
        }
        city_rows(&game, rules, &label, m.seed, &twin, round, &mut out.cities);
        let opt = |v: Option<i64>| v.map_or(String::new(), |v| v.to_string());
        for (r, p, tile, population, excess, case) in classic_rules::ai::war_probe::take().conquests {
            out.decisions.push_str(&format!("{label},{},{twin},{r},{},{tile},{population},{excess},{},{},{},{:?}\n", m.seed, p.0, case.puppet, opt(case.annex), opt(case.raze), case.decision));
        }
    }
    classic_rules::ai::war_probe::enable(false);
    Ok(out)
}

fn city_id(game: &Game, city: CityHandle) -> u64 {
    game.view().auth.cities().get(city).map_or(0, |c| c.id.0)
}

/// Population and building count of every city now (the state before a capture).
fn snapshot_cities(game: &Game, pops: &mut HashMap<CityHandle, (u32, usize)>) {
    pops.clear();
    for (handle, core) in game.view().auth.cities().iter() {
        pops.insert(handle, (core.population, core.buildings.len()));
    }
}

/// Military units that appeared since the last call, with their hammers.
fn count_new_units(game: &Game, rules: &Rules, known: &mut HashSet<u64>, tally: &mut HashMap<PlayerId, Tally>) {
    for core in game.view().auth.units().iter_cores() {
        if !known.insert(core.id.0) {
            continue;
        }
        if rules.is_civilian(core.kind) {
            continue;
        }
        let Some(def) = rules.units.get(core.kind.0 as usize) else { continue };
        if def.strength == 0 && def.ranged_strength == 0 {
            continue;
        }
        let t = tally.entry(core.owner).or_default();
        t.military_built += 1;
        t.military_hammers += def.cost;
    }
}

fn player_row(game: &Game, rules: &Rules, p: PlayerId, t: Tally, wars: &HashSet<(u8, u8)>) -> String {
    let s = standing::<R>(game, rules, p);
    let e = s.economy;
    let auth = game.view().auth;
    let (mut n, mut pop, mut prod, mut sci, mut gold, mut unhappy) = (0, 0, 0, 0, 0, 0);
    for (_, core) in auth.cities().iter() {
        if core.owner != p || core.original_owner == p {
            continue;
        }
        let income = classic_rules::derived::income::city_income(auth, rules, core);
        n += 1;
        pop += core.population;
        prod += income.production;
        sci += income.science;
        gold += income.gold;
        unhappy += classic_rules::derived::happiness::city_unhappiness(auth, rules, core);
    }
    let at_war = wars.iter().filter(|(a, b)| *a == p.0 || *b == p.0).count();
    format!(
        "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
        s.score,
        s.cities,
        s.population,
        s.techs,
        s.military,
        e.science,
        e.production,
        e.gold,
        e.culture,
        e.happiness,
        e.treasury,
        t.units_lost,
        t.hammers_lost,
        t.military_built,
        t.military_hammers,
        t.kills,
        t.captured,
        t.lost,
        n,
        pop,
        prod,
        sci,
        gold,
        unhappy,
        at_war
    )
}

/// One row per city its holder did not found.
fn city_rows(game: &Game, rules: &Rules, label: &str, seed: u64, twin: &str, round: u32, out: &mut String) {
    let auth = game.view().auth;
    for (_, core) in auth.cities().iter() {
        if core.owner == core.original_owner {
            continue;
        }
        let income = classic_rules::derived::income::city_income(auth, rules, core);
        let unhappy = classic_rules::derived::happiness::city_unhappiness(auth, rules, core);
        out.push_str(&format!(
            "{label},{seed},{twin},{round},{},{},{},{:?},{},{},{},{},{},{},{}\n",
            core.id.0,
            core.owner.0,
            core.original_owner.0,
            core.status,
            core.population,
            income.production,
            income.science,
            income.gold,
            income.culture,
            income.food_surplus,
            unhappy
        ));
    }
}

/// The gift of a twin: `giver` proposes its chosen non-capital city to `taker`, which accepts
/// (the deal's own rules: the city keeps its citizens and becomes the taker's own); then the
/// city is made what a capture would make of it (`--as`): its citizens less the capture's loss,
/// the status the AI's conquest decision gives, and the plunder gold. Its buildings all stay
/// (a capture loses a third of most of them: the gift is an upper bound there).
fn give_city(o: &Options, game: &mut Game, rules: &Rules, giver: PlayerId, taker: PlayerId) -> Result<(CityHandle, u32, u32, CityStatus), String> {
    let auth = game.view().auth;
    let capital = classic_rules::combat::city::capital_of(auth, rules, giver);
    let mut options: Vec<(CityHandle, u32)> = auth
        .indexes()
        .cities_of(giver)
        .iter()
        .copied()
        .filter(|c| Some(*c) != capital)
        .filter_map(|c| auth.cities().get(c).map(|core| (c, core.population)))
        .collect();
    options.sort_by_key(|(c, pop)| (*pop, auth.cities().get(*c).map_or(0, |core| core.id.0)));
    let (city, before) = match o.pick.as_str() {
        "small" => options.first().copied(),
        _ => options.last().copied(),
    }
    .ok_or("no city but the capital")?;
    let offer = auth.module().relations().next_offer_id();
    game.execute(rules, Command::ProposeDeal { from: giver, to: taker, items: vec![(giver, DealItem::City(city))] }).map_err(|e| format!("propose: {e:?}"))?;
    game.execute(rules, Command::RespondDeal { player: taker, offer, accept: true }).map_err(|e| format!("respond: {e:?}"))?;
    if game.view().auth.cities().get(city).map(|c| c.owner) != Some(taker) {
        return Err("the deal did not move the city".into());
    }
    // systems::combat::city::captured_population (crate-private): 1 + a quarter is lost above 1.
    let kept = if before <= 1 { before.max(1) } else { before.saturating_sub(1 + before / 4).max(1) };
    let (after, status) = match o.gift_as.as_str() {
        "own" => (before, CityStatus::Own),
        "puppet" => (kept, CityStatus::Puppet),
        "annex" => (kept, CityStatus::Occupied),
        "decide" => (kept, CityStatus::AwaitingDecision),
        _ if kept >= classic_rules::ai::combat::ANNEX_POPULATION => (kept, CityStatus::Occupied),
        _ => (kept, CityStatus::Puppet),
    };
    if o.gift_as != "own" {
        game.debug_set_city_status(city, status);
        game.debug_set_population(rules, city, after);
        // The plunder's expected gold (systems::plunder: 20 + 10 a citizen + 19.5 on average,
        // the city held 50 rounds and more).
        game.debug_earn(taker, 20 + 10 * before + 19);
    }
    Ok((city, before, after, status))
}
