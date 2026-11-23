//! What the bench needs of a rules module ([`BenchRules`]): its rules, a
//! new game of a suite's map, its AI minds by variant name, its score and
//! its victories. Everything else the bench reads from the kernel's world.

use kernel::ai::mind::AiMind;
use kernel::ai::plan::{unit_power, AiSettings};
use kernel::ids::{PlayerId, TileId};
use kernel::module::RulesModule;
use kernel::rules::CompiledRuleset;
use kernel::transaction::journal::ChangeJournal;
use kernel::world::state::GameState;
use serde::{Deserialize, Serialize};

use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::scenario::MapScript;
use classic_rules::ClassicGame;

use crate::spec::{size_from_name, victory_name, GameSpec};

/// The scripted expert of the net tooling (selfplay, the training grounds, the search teacher), by
/// name: `standard` before the twelfth promotion (3 November), so that the grounds' scenes, the
/// teacher and the data stay what they were measured on; `standard` plays the strategy net since.
pub const SCRIPTED_EXPERT: &str = "standard-scripted";
/// [`SCRIPTED_EXPERT`] as a variant.
pub const SCRIPTED_EXPERT_VARIANT: Variant = Variant::StandardScripted;

/// The target of a blow that may fall on a city (`BenchRules::city_blow`).
#[derive(Debug, Clone, Copy)]
pub enum CityBlow {
    City(kernel::ids::CityHandle),
    Tile(kernel::ids::TileId),
}

/// What a player has at one moment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Standing {
    /// The module's score: the one its time victory compares.
    pub score: i64,
    pub cities: u32,
    pub population: u32,
    pub techs: u32,
    /// Sum of strength × health of its units (the AI's own measure).
    pub military: u32,
    /// Tiles it owns.
    pub tiles: u32,
    /// What the module adds (reports saved before it read as zeros).
    #[serde(default)]
    pub economy: Economy,
}

/// A player's economy as the module counts it: world wonders it owns, and
/// gold, production, science, culture and faith a turn (gold after upkeep;
/// reports saved before culture and faith read them as zeros).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Economy {
    pub wonders: u32,
    pub gold: i32,
    pub production: i32,
    pub science: i32,
    /// Food surplus a turn, summed over its cities (`CityIncome::food_surplus`:
    /// what the stores get; a starving city counts negative). Reports saved
    /// before read it as zero.
    #[serde(default)]
    pub food: i32,
    #[serde(default)]
    pub culture: u32,
    #[serde(default)]
    pub faith: u32,
    /// Buildings (not world wonders) that add science, culture, gold or
    /// faith: what a personality's taste shows in (`classic_rules::ai::taste`).
    #[serde(default)]
    pub buildings: SideBuildings,
    /// Gold in the treasury, the empire's excess happiness and the golden
    /// ages it had (reports saved before read them as zeros).
    #[serde(default)]
    pub treasury: u32,
    #[serde(default)]
    pub happiness: i32,
    #[serde(default)]
    pub golden_ages: u32,
    /// Trade routes it runs.
    #[serde(default)]
    pub trade_routes: u32,
    /// City-states it is the ally of.
    #[serde(default)]
    pub allies: u32,
    /// Food delivered between its cities so far (the bench counts it from
    /// the journals, `BenchRules::delivered`; 0 without food routes).
    #[serde(default)]
    pub delivered: u64,
    /// Its religion: 1 when it founded one, its own cities whose majority
    /// follows it, and its followers in the world (reports saved before
    /// read them as zeros).
    #[serde(default)]
    pub religion: u32,
    #[serde(default)]
    pub faithful_cities: u32,
    #[serde(default)]
    pub followers: u32,
    /// The demographics screen's rows (reports saved before read zeros).
    #[serde(default)]
    pub demographics: [Demo; 7],
}

/// One row of the game's demographics screen (`classic_rules::derived::standing`, the figures the
/// AI reads as public information): the player's value and its rank among the living nations.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Demo {
    pub value: i64,
    pub rank: u32,
}

/// The rows of the demographics screen, in the order of `Measure::ALL` and with its names: population,
/// crop yield, manufactured goods, GNP, literacy (percent of the technologies known), land area, soldiers.
pub const DEMOGRAPHICS: [&str; 7] = ["population", "crop yield", "manufactured goods", "GNP", "literacy", "land area", "soldiers"];

/// Buildings by the yield they add (one building may count in several).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SideBuildings {
    pub science: u32,
    pub culture: u32,
    pub gold: u32,
    pub faith: u32,
    /// Buildings that add happiness (reports saved before read zeros).
    #[serde(default)]
    pub happiness: u32,
}

/// A rules module on the bench.
pub trait BenchRules {
    type M: RulesModule;
    type Mind: AiMind<Self::M>;

    /// The module's name (the report's `rules`).
    const NAME: &'static str;

    fn rules() -> CompiledRuleset<Self::M>;

    /// The variants by name; the first, `standard`, is the game's AI and
    /// the default of both sides.
    fn variants() -> Vec<&'static str>;

    fn mind(variant: &str) -> Option<Self::Mind>;

    fn settings(rules: &CompiledRuleset<Self::M>) -> AiSettings;

    /// A new game on `map` whose time victory comes at `rounds`.
    fn new_game(rules: &CompiledRuleset<Self::M>, map: &crate::bench::suite::MapSpec, seed: u64, rounds: u32) -> GameState<Self::M>;

    /// `new_game` at the game speed `speed` (a speed key; empty: Standard; a
    /// module without speeds plays Standard).
    fn new_game_at(rules: &CompiledRuleset<Self::M>, map: &crate::bench::suite::MapSpec, seed: u64, rounds: u32, speed: &str) -> GameState<Self::M> {
        let _ = speed;
        Self::new_game(rules, map, seed, rounds)
    }

    fn score(game: &GameState<Self::M>, rules: &CompiledRuleset<Self::M>, player: PlayerId) -> i64;

    /// The player's economy (zeros: the module counts none).
    fn economy(_game: &GameState<Self::M>, _rules: &CompiledRuleset<Self::M>, _player: PlayerId) -> Economy {
        Economy::default()
    }

    /// The winner and the kind of the victory, once the game is over.
    fn victory(game: &GameState<Self::M>) -> Option<(PlayerId, &'static str)>;

    /// The parts a spaceship of the science victory needs (0: the module
    /// has none).
    fn spaceship_parts(_rules: &CompiledRuleset<Self::M>) -> u32 {
        0
    }

    /// The player's spaceship now (zeros: the module has none).
    fn spaceship(_game: &GameState<Self::M>, _rules: &CompiledRuleset<Self::M>, _player: PlayerId) -> crate::bench::play::SpaceProgress {
        crate::bench::play::SpaceProgress::default()
    }

    /// Dev-only (`AURELIA_SPACE_TRACE`): the player's way to the spaceship now, one line (empty: the module
    /// has none). Written to stderr each time it changes.
    fn space_tick(_game: &GameState<Self::M>, _rules: &CompiledRuleset<Self::M>, _player: PlayerId) -> String {
        String::new()
    }

    /// The name of the player's leader (empty: the module has no leaders).
    fn leader(_game: &GameState<Self::M>, _rules: &CompiledRuleset<Self::M>, _player: PlayerId) -> String {
        String::new()
    }

    /// The name of a goal of the AI's grand strategy (`AiState::aim`).
    fn goal_name(_goal: u8) -> Option<&'static str> {
        None
    }

    /// A command that fights (its destroyed units and taken cities are the
    /// bench's combat record).
    fn is_combat(command: &<Self::M as RulesModule>::Command) -> bool;

    /// A command that pillages a tile (counted in the combat record: the AI's raids).
    fn is_pillage(_command: &<Self::M as RulesModule>::Command) -> bool {
        false
    }

    /// The speed's pace percent and the production a turn of each of the player's cities (probe only).
    fn order_inputs(_game: &GameState<Self::M>, _rules: &CompiledRuleset<Self::M>, _player: PlayerId) -> (u32, Vec<(kernel::ids::CityHandle, u32)>) {
        (100, Vec::new())
    }

    /// What a command strikes when it is a blow on a city's tile: the city, or a tile (probe only,
    /// `bench::rival`).
    fn city_blow(_command: &<Self::M as RulesModule>::Command) -> Option<CityBlow> {
        None
    }

    /// The original capitals of the player's rivals, by founder, and
    /// whether the player knows each one's tile (empty: the module has no
    /// domination by capitals).
    fn rival_capitals(_game: &GameState<Self::M>, _rules: &CompiledRuleset<Self::M>, _player: PlayerId) -> Vec<(PlayerId, bool)> {
        Vec::new()
    }

    /// A command whose peace both sides agreed to (an accepted deal); a
    /// peace by any other command is one side's alone.
    fn by_agreement(_command: &<Self::M as RulesModule>::Command) -> bool {
        false
    }

    /// What a command did to the deals of the players (`DealEvent`), each for the party
    /// concerned; `game` is the world after the acting player's turn (none: the module has no
    /// deals).
    fn deals(_game: &GameState<Self::M>, _rules: &CompiledRuleset<Self::M>, _command: &<Self::M as RulesModule>::Command, _changes: &ChangeJournal<Self::M>) -> Vec<(PlayerId, crate::bench::play::DealEvent)> {
        Vec::new()
    }

    /// What an accepted command bought with gold (none: the module has no purchases).
    fn purchase(_command: &<Self::M as RulesModule>::Command) -> Option<crate::bench::play::Purchase> {
        None
    }

    /// Risings of rebels in `changes`, each for the nation they rose against, with the rebels
    /// that rose (none: the module has no rebels).
    fn rebellions(_changes: &ChangeJournal<Self::M>) -> Vec<(PlayerId, u32)> {
        Vec::new()
    }

    /// The player ended its turn below the rebellion threshold of happiness (never: the module
    /// has none).
    fn rebellious(_game: &GameState<Self::M>, _player: PlayerId) -> bool {
        false
    }

    /// Food each player's cities received from its other cities in the
    /// rounds `changes` closed (none: the module has no food routes).
    fn delivered(_game: &GameState<Self::M>, _changes: &ChangeJournal<Self::M>) -> Vec<(PlayerId, u64)> {
        Vec::new()
    }

    /// The first rounds the opening's tempo is counted over (0: the module
    /// counts none).
    const OPENING_ROUNDS: u32 = 0;

    /// The players a command's meaningful decisions go to, one entry a
    /// decision; `actor` is the player whose turn it is.
    fn decisions(_actor: PlayerId, _command: &<Self::M as RulesModule>::Command, _changes: &ChangeJournal<Self::M>) -> Vec<PlayerId> {
        Vec::new()
    }

    /// A recorder of the match (None: the module records nothing; Classic:
    /// the net seats' DAgger shards when `AURELIA_NET_DAGGER` names a
    /// directory). `name` names the match's files.
    fn recorder(_name: &str, _seed: u64) -> Option<Box<dyn MatchRecorder<Self>>> {
        None
    }
}

/// The DAgger shards keep, per net player, M windows of K consecutive turns
/// (`AURELIA_NET_DAGGER_WINDOWS=K:M`, as `ai-features --window K
/// --windows-per-player M`); unset: every turn.
pub const DAGGER_WINDOWS_ENV: &str = "AURELIA_NET_DAGGER_WINDOWS";

/// What a match records while it is played (`BenchRules::recorder`).
pub trait MatchRecorder<R: BenchRules + ?Sized> {
    /// At every round's start.
    fn round_start(&mut self, game: &GameState<R::M>, rules: &CompiledRuleset<R::M>);
    /// At the start of each player's turn (the world before its first command), with the AI's
    /// memory of every player as it stands then.
    fn turn_start(&mut self, game: &GameState<R::M>, rules: &CompiledRuleset<R::M>, player: PlayerId, states: &kernel::ai::state::AiStates);
    /// At the end: the seats' minds (to take what they recorded).
    fn finish(self: Box<Self>, game: &GameState<R::M>, rules: &CompiledRuleset<R::M>, minds: &[&R::Mind]);
}

/// The DAgger recorder of the classic bench (`AURELIA_NET_DAGGER=<dir>`):
/// the truth at every player-turn's start and the nations at every round's
/// start; at the end the net seats' recorded turns become one shard
/// (`export::recorded_shard`) in the directory.
struct DaggerRecorder {
    dir: std::path::PathBuf,
    name: String,
    /// The game's seed, for the windows.
    seed: u64,
    /// `AURELIA_NET_DAGGER_WINDOWS=K:M`: per net player M windows of K
    /// consecutive turns (`export::choose_windows`); None: every turn.
    windows: Option<(usize, usize)>,
    summaries: Vec<(u32, crate::export::RoundSummary)>,
    truths: std::collections::BTreeMap<(u32, u8), Vec<i16>>,
}

impl MatchRecorder<ClassicBench> for DaggerRecorder {
    fn round_start(&mut self, game: &GameState<classic_rules::classic::Classic>, rules: &CompiledRuleset<classic_rules::classic::Classic>) {
        self.summaries.push((game.view().auth.turn().round, crate::export::round_summary(game, rules)));
    }

    fn turn_start(&mut self, game: &GameState<classic_rules::classic::Classic>, rules: &CompiledRuleset<classic_rules::classic::Classic>, player: PlayerId, _states: &kernel::ai::state::AiStates) {
        if game.view().auth.players().is_major(player) {
            self.truths.insert((game.view().auth.turn().round, player.0), crate::export::truth_row(game, rules, player));
        }
    }

    fn finish(self: Box<Self>, game: &GameState<classic_rules::classic::Classic>, rules: &CompiledRuleset<classic_rules::classic::Classic>, minds: &[&ClassicMind]) {
        let mut turns = Vec::new();
        let mut counts = crate::export::Counts::default();
        for net in minds.iter().filter_map(|m| m.net()) {
            let recording = net.take_recording();
            for (reason, n) in recording.dropped {
                *counts.dropped.entry(reason).or_default() += n;
            }
            for t in recording.turns {
                let truth = self.truths.get(&(t.round, t.player.0)).cloned().unwrap_or_else(|| vec![0; crate::export::truth_fields().len()]);
                turns.push(crate::export::RecordedTurn { round: t.round, player: t.player, turn: t.turn, truth, decisions: t.decisions });
            }
        }
        if turns.is_empty() {
            return;
        }
        counts.player_turns = turns.len() as u64;
        // The windows: per player its turns in round order, M windows of K.
        let windows = self.windows.map(|(length, per_player)| {
            let mut by_player: std::collections::BTreeMap<u8, Vec<u32>> = Default::default();
            for t in &turns {
                by_player.entry(t.player.0).or_default().push(t.round);
            }
            let mut keep = std::collections::BTreeMap::new();
            let mut id = 0;
            for (player, mut rounds) in by_player {
                rounds.sort_unstable();
                for (first, len) in crate::export::choose_windows(self.seed, PlayerId(player), rounds.len(), length, per_player) {
                    for round in &rounds[first..first + len] {
                        keep.insert((player, *round), id);
                    }
                    id += 1;
                }
            }
            keep
        });
        if let Some(keep) = &windows {
            turns.retain(|t| keep.contains_key(&(t.player.0, t.round)));
        }
        let shard = crate::export::recorded_shard(rules, self.name, None, game, turns, windows.as_ref(), self.summaries, counts);
        // A failed write (a full disk) leaves no partial shard behind.
        let out = self.dir.join(format!("{}.shard", shard.name));
        if let Err(e) = std::fs::create_dir_all(&self.dir).and_then(|_| crate::export::write_shard(&self.dir, &shard, rules)) {
            let _ = std::fs::remove_dir_all(&out);
            eprintln!("the DAgger shard {} is not written: {e}", out.display());
        }
    }
}

/// The nations of a game (not city-states or barbarians), by id.
pub fn majors<M: RulesModule>(game: &GameState<M>) -> Vec<PlayerId> {
    let players = game.view().auth.players();
    (0..players.count() as u8).map(PlayerId).filter(|p| players.is_major(*p)).collect()
}

pub fn standing<R: BenchRules>(game: &GameState<R::M>, rules: &CompiledRuleset<R::M>, player: PlayerId) -> Standing {
    let auth = game.view().auth;
    let cities = auth.indexes().cities_of(player);
    let military = auth
        .units()
        .iter_cores()
        .filter(|u| u.owner == player)
        .map(|u| unit_power(rules, u.kind, u.hp))
        .sum();
    let tiles = (0..auth.map().tile_count() as u32)
        .filter(|t| auth.map().owner(TileId(*t)).flatten() == Some(player))
        .count();
    Standing {
        score: R::score(game, rules, player),
        cities: cities.len() as u32,
        population: cities.iter().filter_map(|c| auth.cities().get(*c)).map(|c| c.population).sum(),
        techs: auth.players().techs_of(player).len() as u32,
        military,
        tiles: tiles as u32,
        economy: R::economy(game, rules, player),
    }
}

/// The classic rules at Prince with the setup screen's defaults of the size.
#[derive(Debug)]
pub struct ClassicBench;

const PRINCE: &str = "core:handicap/prince";

impl BenchRules for ClassicBench {
    type M = classic_rules::classic::Classic;
    type Mind = ClassicMind;

    const NAME: &'static str = "classic";

    fn rules() -> CompiledRuleset<Self::M> {
        classic_rules::compile_rules(classic_content::core::core_rules()).expect("classic rules compile")
    }

    fn variants() -> Vec<&'static str> {
        Variant::ALL.iter().map(|v| v.name()).collect()
    }

    /// A variant by name. The net: `net` (weights from `AURELIA_NET_WEIGHTS`),
    /// `net:<weights.bin>` or `net:dummy` (that file, so checkpoints sit at
    /// one table), `net:remote:SOCKET` or `net-remote:SOCKET` (a checkpoint
    /// served by `python -m aurelia_nn.remote serve`), each optionally `+<roles>` (`net+founder`,
    /// `net:<file>+founder,builder`: those units stay with the scripted mind,
    /// `ScriptedRoles`). A weights file that cannot be read or an unknown
    /// role is an error (a panic: the seat cannot play).
    fn mind(variant: &str) -> Option<ClassicMind> {
        use classic_rules::ai::net_e2e::mind::{NetPlayer, ScriptedRoles};
        let (base, roles) = variant.split_once('+').unwrap_or((variant, ""));
        // `net-remote:SOCKET` is `net:remote:SOCKET` (`net_e2e::remote`).
        let remote = base.strip_prefix("net-remote:").map(|s| format!("{}{s}", classic_rules::ai::net_e2e::remote::PREFIX));
        let path = match remote.as_deref().or_else(|| base.strip_prefix("net:")) {
            Some(path) => Some(path.to_owned()),
            None if base == "net" && !roles.is_empty() => Some(std::env::var(classic_rules::ai::net_e2e::mind::WEIGHTS_ENV).unwrap_or_else(|_| panic!("`{variant}` needs AURELIA_NET_WEIGHTS"))),
            None => None,
        };
        if let Some(path) = path {
            let roles = ScriptedRoles::parse(roles).unwrap_or_else(|e| panic!("{e}"));
            let mut net = NetPlayer::from_path(&path).unwrap_or_else(|e| panic!("{e}")).with_scripted(roles);
            if let Ok(list) = std::env::var(classic_rules::ai::net_e2e::mind::HEADS_ENV) {
                net = net.with_heads(classic_rules::ai::net_e2e::mind::parse_heads(&list).unwrap_or_else(|e| panic!("{e}")));
            }
            return Some(ClassicMind::with_net(Variant::Net, net));
        }
        // `X@Y`: variant X deciding a captured city as variant Y does (`ai::conquest`; a book
        // that puppets, classic/docs/ai-conquest-payoff.md). A league book X plays on Y
        // instead, its decision of a captured city included (`ClassicMind::book_on`,
        // classic/docs/ai-books.md «Re-based»): `puppet-all` and `puppet-price` differ from
        // `pre-puppet`, a duel book's base, only by that decision, so `rush-siege@puppet-all`
        // plays as it did; `rush-siege@pre-puppet` is `rush-siege`.
        if let Some((name, on)) = variant.split_once('@') {
            let (v, on) = (Variant::from_name(name)?, Variant::from_name(on)?);
            if ClassicMind::rebases(v) {
                return ClassicMind::book_on(v, on);
            }
            return Some(ClassicMind::new(v).with_conquest(on.conquest()));
        }
        Variant::from_name(variant).map(ClassicMind::new)
    }

    fn settings(rules: &CompiledRuleset<Self::M>) -> AiSettings {
        AiSettings::for_rules(rules, PRINCE)
    }

    fn recorder(name: &str, seed: u64) -> Option<Box<dyn MatchRecorder<Self>>> {
        // The probe of lane A's defects (`bench::diplo_probe`, `AURELIA_DIPLO_PROBE`).
        if let Some(probe) = crate::bench::diplo_probe::DiploProbe::new(name, seed) {
            return Some(Box::new(probe));
        }
        // The check of the model of the rivals (`bench::rival_net`, `AURELIA_RIVAL_NET`).
        if let Some(check) = crate::bench::rival_net::RivalNetRecorder::new(name, seed) {
            return Some(Box::new(check));
        }
        let dir = std::env::var_os(classic_rules::ai::net_e2e::mind::DAGGER_ENV)?;
        let windows = std::env::var(DAGGER_WINDOWS_ENV).ok().filter(|w| !w.is_empty()).map(|w| {
            let (k, m) = w.split_once(':').unwrap_or_else(|| panic!("{DAGGER_WINDOWS_ENV}=K:M, not {w}"));
            let number = |v: &str| v.trim().parse::<usize>().ok().filter(|n| *n > 0).unwrap_or_else(|| panic!("{DAGGER_WINDOWS_ENV}=K:M (positive), not {w}"));
            (number(k), number(m))
        });
        Some(Box::new(DaggerRecorder { dir: dir.into(), name: name.to_owned(), seed, windows, summaries: Vec::new(), truths: Default::default() }))
    }

    fn new_game(rules: &CompiledRuleset<Self::M>, map: &crate::bench::suite::MapSpec, seed: u64, rounds: u32) -> GameState<Self::M> {
        let script = MapScript::from_key(map.script).unwrap_or_else(|| panic!("unknown map script {}", map.script));
        let size = size_from_name(map.size).unwrap_or_else(|| panic!("unknown world size {}", map.size));
        let spec = GameSpec::new(script, size, seed).max_rounds(rounds);
        let (config, options) = spec.scenario(rules);
        classic_rules::scenario::generate_new_game_with(rules, &config, None, options)
    }

    fn new_game_at(rules: &CompiledRuleset<Self::M>, map: &crate::bench::suite::MapSpec, seed: u64, rounds: u32, speed: &str) -> GameState<Self::M> {
        let script = MapScript::from_key(map.script).unwrap_or_else(|| panic!("unknown map script {}", map.script));
        let size = size_from_name(map.size).unwrap_or_else(|| panic!("unknown world size {}", map.size));
        let spec = GameSpec::new(script, size, seed).max_rounds(rounds).speed(speed);
        let (config, options) = spec.scenario(rules);
        classic_rules::scenario::generate_new_game_with(rules, &config, None, options)
    }

    fn score(game: &GameState<Self::M>, rules: &CompiledRuleset<Self::M>, player: PlayerId) -> i64 {
        classic_rules::world::victory::score(game.view().auth, rules, player)
    }

    fn economy(game: &GameState<Self::M>, rules: &CompiledRuleset<Self::M>, player: PlayerId) -> Economy {
        let obs = game.ai_observation(rules, player);
        let wonder = |b: &classic_rules::ids::BuildingKindId| rules.buildings.get(b.0 as usize).is_some_and(|d| d.wonder);
        Economy {
            wonders: obs.own_cities.iter().flat_map(|c| &c.buildings).filter(|b| wonder(b)).count() as u32,
            gold: obs.economy.income.gold,
            production: obs.own_cities.iter().map(|c| c.income.production).sum(),
            science: obs.economy.income.science,
            food: obs.own_cities.iter().map(|c| c.income.food_surplus).sum(),
            culture: obs.economy.income.culture,
            faith: obs.economy.income.faith,
            buildings: side_buildings(rules, obs.own_cities.iter().flat_map(|c| &c.buildings)),
            treasury: obs.economy.treasury,
            happiness: obs.economy.happiness.excess(),
            golden_ages: game.view().auth.players().economy_of(player).map_or(0, |e| u32::from(e.golden_ages)),
            trade_routes: game.view().auth.module().trade().owned_by(player).count() as u32,
            allies: obs.city_states.iter().filter(|c| c.ally == Some(player)).count() as u32,
            delivered: 0,
            religion: u32::from(obs.faith.overview.religion.is_some()),
            faithful_cities: obs
                .faith
                .cities
                .iter()
                .filter(|c| c.owner == player && obs.faith.overview.religion.is_some_and(|r| c.majority == Some(classic_rules::world::religion::Creed::Religion(r))))
                .count() as u32,
            followers: obs
                .faith
                .overview
                .religions
                .iter()
                .find(|r| Some(r.religion) == obs.faith.overview.religion)
                .map_or(0, |r| r.counts.followers),
            demographics: classic_rules::derived::standing::Measure::ALL
                .map(|m| obs.public.row(m).map_or_else(Demo::default, |d| Demo { value: d.value, rank: d.rank as u32 })),
        }
    }

    fn victory(game: &GameState<Self::M>) -> Option<(PlayerId, &'static str)> {
        game.outcome().map(|o| (o.winner, victory_name(o.kind)))
    }

    /// The parts the science victory counts (`systems::victory::spaceship_parts`).
    fn spaceship_parts(rules: &CompiledRuleset<Self::M>) -> u32 {
        classic_rules::world::victory::spaceship_parts(rules).iter().map(|(_, n)| u32::from(*n)).sum()
    }

    /// The projects the parts need first (Apollo) and the parts installed,
    /// as the rules count them.
    fn spaceship(game: &GameState<Self::M>, rules: &CompiledRuleset<Self::M>, player: PlayerId) -> crate::bench::play::SpaceProgress {
        let victory = game.view().auth.module().victory();
        let prerequisites = classic_rules::ai::victory::prerequisite_projects(rules);
        let parts = classic_rules::world::victory::spaceship_parts(rules);
        let kinds = classic_rules::ai::victory::part_kinds(rules);
        let auth = game.view().auth;
        let built = parts
            .iter()
            .map(|(p, needed)| {
                let walking = auth.units().iter_cores().filter(|u| u.owner == player && kinds.iter().any(|(k, q, _)| *k == u.kind && q == p)).count() as u32;
                (u32::from(victory.project_count(player, *p)) + walking).min(u32::from(*needed))
            })
            .sum();
        crate::bench::play::SpaceProgress {
            project_done: !prerequisites.is_empty() && prerequisites.iter().all(|p| victory.project_count(player, *p) > 0),
            parts: parts.iter().map(|(p, needed)| u32::from(victory.project_count(player, *p).min(*needed))).sum(),
            built,
        }
    }

    /// `t` the key technologies known (rocketry, robotics, satellites, advanced ballistics, particle physics,
    /// nanotechnology), `e` the era, `a` the prerequisite projects done, `i` parts installed, `q` space items
    /// queued / at the head of a queue, `w` the part units (kind letter + hexes to the capital), `c` the
    /// capital's tile and whether it is the original one, `n` cities.
    fn space_tick(game: &GameState<Self::M>, rules: &CompiledRuleset<Self::M>, player: PlayerId) -> String {
        use kernel::ids::ProductionItem;
        const KEY: [&str; 6] = ["rocketry", "robotics", "satellites", "advanced-ballistics", "particle-physics", "nanotechnology"];
        let auth = game.view().auth;
        let known = auth.players().techs_of(player);
        let techs: String = KEY
            .iter()
            .map(|k| if known.iter().any(|t| rules.technologies.get(t.0 as usize).is_some_and(|d| d.key.rsplit('/').next() == Some(*k))) { '1' } else { '0' })
            .collect();
        let era = kernel::world::player::player_era(auth, rules, player);
        let victory = auth.module().victory();
        let prerequisites = classic_rules::ai::victory::prerequisite_projects(rules);
        let apollo = !prerequisites.is_empty() && prerequisites.iter().all(|p| victory.project_count(player, *p) > 0);
        let parts = classic_rules::world::victory::spaceship_parts(rules);
        let installed: u32 = parts.iter().map(|(p, needed)| u32::from(victory.project_count(player, *p).min(*needed))).sum();
        let kinds = classic_rules::ai::victory::part_kinds(rules);
        let space_item = |item: &ProductionItem| match item {
            ProductionItem::Unit(k) => kinds.iter().any(|(kind, _, _)| kind == k),
            ProductionItem::Project(p) => prerequisites.contains(p),
            ProductionItem::Building(_) => false,
        };
        let cities: Vec<_> = auth.cities().iter().map(|(_, c)| c).filter(|c| c.owner == player).collect();
        let queued = cities.iter().flat_map(|c| c.queue.iter()).filter(|i| space_item(i)).count();
        let heads = cities.iter().filter(|c| c.queue.first().is_some_and(&space_item)).count();
        let capital = classic_rules::combat::city::capital_of(auth, rules, player).and_then(|c| auth.cities().get(c));
        let dims = auth.map().dimensions;
        let walking: Vec<String> = auth
            .units()
            .iter_cores()
            .filter(|u| u.owner == player && kinds.iter().any(|(k, _, _)| *k == u.kind))
            .map(|u| {
                let key = rules.units.get(u.kind.0 as usize).map_or("", |d| d.key.as_str());
                let letter = key.rsplit('/').next().unwrap_or("").trim_start_matches("ss-").chars().next().unwrap_or('?').to_ascii_uppercase();
                let hexes = capital.and_then(|c| dims.distance(c.tile, u.tile)).map_or_else(|| "?".to_owned(), |d| d.to_string());
                format!("{letter}{hexes}")
            })
            .collect();
        // While a part waits outside: every unit on the capital's tile and on the tiles next to the waiting parts.
        let short = |kind: kernel::ids::UnitKindId| rules.units.get(kind.0 as usize).map_or("?", |d| d.key.rsplit('/').next().unwrap_or("?"));
        let mut crowd = String::new();
        if let Some(c) = capital {
            if walking.iter().any(|w| !w.ends_with('0') || w.len() > 2) {
                let here: Vec<String> = auth.units().iter_cores().filter(|u| u.tile == c.tile && !kinds.iter().any(|(k, _, _)| *k == u.kind)).map(|u| format!("{}:{}", u.owner.0, short(u.kind))).collect();
                // The civilians (no strength) within two tiles of the capital: who stands at its gate.
                let near: Vec<String> = auth
                    .units()
                    .iter_cores()
                    .filter(|u| rules.units.get(u.kind.0 as usize).is_some_and(|d| d.strength == 0) && !kinds.iter().any(|(k, _, _)| *k == u.kind))
                    .filter_map(|u| dims.distance(c.tile, u.tile).filter(|d| (1..=2).contains(d)).map(|d| format!("{}:{}@{d}", u.owner.0, short(u.kind))))
                    .collect();
                crowd = format!(" k[{}] g[{}]", here.join(","), near.join(","));
            }
        }
        let capital = capital.map_or_else(|| "none".to_owned(), |c| format!("{}:{}", c.tile.0, u8::from(c.original_capital && c.original_owner == player)));
        format!(
            "t{techs} e{era} a{} i{installed} q{queued}/{heads} w[{}] c{capital} n{}{crowd}",
            u8::from(apollo),
            walking.join(","),
            cities.len()
        )
    }

    /// The nation of the leader (its content key's name).
    fn leader(game: &GameState<Self::M>, rules: &CompiledRuleset<Self::M>, player: PlayerId) -> String {
        let nation = game.view().auth.players().nation_of(player);
        let key = rules.nations.get(usize::from(nation)).map_or("", |n| n.key.as_str());
        key.rsplit('/').next().unwrap_or(key).to_owned()
    }

    fn goal_name(goal: u8) -> Option<&'static str> {
        classic_rules::ai::grand_strategy::VictoryPath::from_code(goal).map(|p| p.name())
    }

    fn is_pillage(command: &classic_rules::Command) -> bool {
        matches!(command, classic_rules::Command::Pillage { .. })
    }

    fn is_combat(command: &classic_rules::Command) -> bool {
        use classic_rules::Command as C;
        matches!(
            command,
            C::AttackUnit { .. } | C::AttackCity { .. } | C::RangedAttack { .. } | C::CityStrike { .. } | C::AirSweep { .. } | C::Nuke { .. }
        )
    }

    /// Every nation's original capital but the player's: what its
    /// domination victory must take.
    fn rival_capitals(game: &GameState<Self::M>, rules: &CompiledRuleset<Self::M>, player: PlayerId) -> Vec<(PlayerId, bool)> {
        let auth = game.view().auth;
        let view = classic_rules::knowledge::view::PlayerView::new(auth, game.derived(), rules, player);
        let players = auth.players();
        let mut capitals: Vec<(PlayerId, bool)> = auth
            .cities()
            .iter()
            .map(|(_, c)| c)
            .filter(|c| c.original_capital && c.original_owner != player && players.is_major(c.original_owner))
            .map(|c| (c.original_owner, view.is_known(c.tile)))
            .collect();
        capitals.sort();
        capitals
    }

    fn order_inputs(game: &GameState<Self::M>, rules: &CompiledRuleset<Self::M>, player: PlayerId) -> (u32, Vec<(kernel::ids::CityHandle, u32)>) {
        let obs = game.ai_observation(rules, player);
        (obs.speed.pace_percent, obs.own_cities.iter().map(|c| (c.handle, c.income.production.max(0) as u32)).collect())
    }

    fn city_blow(command: &classic_rules::Command) -> Option<CityBlow> {
        match command {
            classic_rules::Command::AttackCity { city, .. } => Some(CityBlow::City(*city)),
            classic_rules::Command::RangedAttack { target, .. } => Some(CityBlow::Tile(*target)),
            _ => None,
        }
    }

    fn by_agreement(command: &classic_rules::Command) -> bool {
        matches!(command, classic_rules::Command::RespondDeal { accept: true, .. })
    }

    /// `HurryProduction` buys the head of a queue, `PurchaseTile` a tile.
    fn purchase(command: &classic_rules::Command) -> Option<crate::bench::play::Purchase> {
        match command {
            classic_rules::Command::HurryProduction { .. } => Some(crate::bench::play::Purchase::Item),
            classic_rules::Command::PurchaseTile { .. } => Some(crate::bench::play::Purchase::Tile),
            _ => None,
        }
    }

    /// `ClassicChange::RebelsRose`: the nation the rebels rose against, and how many.
    fn rebellions(changes: &ChangeJournal<Self::M>) -> Vec<(PlayerId, u32)> {
        changes
            .module_changes
            .iter()
            .filter_map(|c| match *c {
                classic_rules::classic::ClassicChange::RebelsRose { player, count, .. } => Some((player, u32::from(count))),
                _ => None,
            })
            .collect()
    }

    /// The happiness snapshot of the player's last turn is at or below
    /// `REBELLION_THRESHOLD` (-21).
    fn rebellious(game: &GameState<Self::M>, player: PlayerId) -> bool {
        use classic_rules::derived::happiness::{excess_snapshot, uprising_possible};
        uprising_possible(excess_snapshot(game.view().auth, player))
    }

    /// Cities ceded by an accepted deal (the giver), gold per turn taken on by one (the payer:
    /// a gold-per-turn trade of the pair that runs the speed's whole deal from this round), and
    /// deals broken by an unpaid gold per turn (the debtor, `ClassicChange::DealBroken`).
    fn deals(game: &GameState<Self::M>, rules: &CompiledRuleset<Self::M>, command: &classic_rules::Command, changes: &ChangeJournal<Self::M>) -> Vec<(PlayerId, crate::bench::play::DealEvent)> {
        use crate::bench::play::DealEvent;
        use classic_rules::classic::ClassicChange;
        let mut out = Vec::new();
        for change in &changes.module_changes {
            if let ClassicChange::DealBroken { debtor, .. } = *change {
                out.push((debtor, DealEvent::GoldPerTurnBroken));
            }
        }
        if !matches!(command, classic_rules::Command::RespondDeal { accept: true, .. }) {
            return out;
        }
        for change in &changes.city_changes {
            if let kernel::transaction::journal::CityChange::OwnerChanged { from, .. } = change {
                out.push((*from, DealEvent::CityCeded));
            }
        }
        let auth = game.view().auth;
        let until = auth.turn().round + auth.settings().speed(rules).deal_rounds;
        for change in &changes.module_changes {
            let ClassicChange::OfferAnswered { from, to, accepted: true, .. } = *change else { continue };
            let pair = |a: PlayerId, b: PlayerId| (a, b) == (from, to) || (a, b) == (to, from);
            for trade in auth.module().relations().trades() {
                let gold = matches!(trade.flow, classic_rules::world::relations::TradeFlow::GoldPerTurn(_));
                if gold && trade.until == until && pair(trade.from, trade.to) {
                    out.push((trade.from, DealEvent::GoldPerTurn));
                }
            }
        }
        out
    }
}

/// Counts buildings by the yields they add (`classic_rules::ai::taste::Sides`)
/// and by the happiness they add.
fn side_buildings<'b>(rules: &CompiledRuleset<classic_rules::classic::Classic>, buildings: impl Iterator<Item = &'b classic_rules::ids::BuildingKindId>) -> SideBuildings {
    use classic_rules::ai::taste::Sides;
    let mut out = SideBuildings::default();
    for kind in buildings {
        let sides = Sides::of_building(rules, *kind);
        out.science += u32::from(sides.has(Sides::SCIENCE));
        out.culture += u32::from(sides.has(Sides::CULTURE));
        out.gold += u32::from(sides.has(Sides::GOLD));
        out.faith += u32::from(sides.has(Sides::FAITH));
        let happy = classic_rules::ai::happiness::building_happiness(rules, *kind) > 0;
        out.happiness += u32::from(happy && !sides.has(Sides::WONDERS));
    }
    out
}
