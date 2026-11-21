//! The probe of lane A's defects (classic/docs/ai-strategist-defects.md): what each nation's seat does
//! with the city-states and how its spaceship waits for the copies of its parts, read from the world at
//! the start of every round. One JSON line per seat at the end of a match, appended to the file
//! `AURELIA_DIPLO_PROBE` names. Reads only: the result hash of a match does not move.
//!
//! | field | how |
//! |---|---|
//! | `cs_wars` | city-states the nation declared war on (`MajorRecord::minors_attacked` at the end) |
//! | `cs_cities` | cities founded by a city-state the nation holds at the end |
//! | `tributes` | rounds its `last_bullied` with a city-state changed (gold or worker) |
//! | `quests_given`, `quests_done` | quests for the nation that appeared; those gone before their end while its influence with the city-state rose by at least the reward less 2 |
//! | `allies`, `friends`, `allies_132` | city-states allied and friends at the end, allies at round 132 |
//! | `parts_round` | the first round every prerequisite project of the parts was done |
//! | `parts_lack` | at that round fewer free copies of a part's resource than one part needs |
//! | `blocked` | rounds from then, while a part is missing, with fewer free copies than one part needs |

use std::collections::BTreeMap;
use std::io::Write;
use std::sync::{Mutex, OnceLock};

use classic_rules::world::city_states as cs;
use classic_rules::world::city_states::Quest;
use classic_rules::world::player::ClassicPlayers;
use kernel::ai::state::AiStates;
use kernel::ids::PlayerId;
use kernel::rules::CompiledRuleset;
use kernel::world::state::GameState;

use crate::bench::modules::{ClassicBench, MatchRecorder};

/// The environment variable that turns the probe on (the JSONL file).
pub const ENV: &str = "AURELIA_DIPLO_PROBE";

/// The round the allies are counted at besides the end (Quick: Standard 200).
const MID_ROUND: u32 = 132;

type Classic = classic_rules::classic::Classic;

fn sink() -> Option<&'static Mutex<std::fs::File>> {
    static SINK: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
    SINK.get_or_init(|| {
        let path = std::env::var(ENV).ok().filter(|p| !p.is_empty())?;
        let file = std::fs::OpenOptions::new().create(true).append(true).open(path).ok()?;
        Some(Mutex::new(file))
    })
    .as_ref()
}

/// One seat's counts.
#[derive(Debug, Clone, Default)]
struct Seat {
    tributes: u32,
    quests_given: u32,
    quests_done: u32,
    allies_mid: Option<u32>,
    parts_round: Option<u32>,
    parts_lack: bool,
    blocked: u32,
}

/// What the probe remembers from the last round: the quests and the influence of each seat.
pub struct DiploProbe {
    name: String,
    seed: u64,
    seats: BTreeMap<u8, Seat>,
    quests: Vec<Quest>,
    influence: BTreeMap<(u8, u8), i32>,
    bullied: BTreeMap<(u8, u8), Option<u32>>,
}

impl DiploProbe {
    pub fn new(name: &str, seed: u64) -> Option<Self> {
        sink()?;
        Some(Self { name: name.to_owned(), seed, seats: BTreeMap::new(), quests: Vec::new(), influence: BTreeMap::new(), bullied: BTreeMap::new() })
    }
}

fn majors(game: &GameState<Classic>) -> Vec<PlayerId> {
    let players = game.view().auth.players();
    (0..players.count()).map(|p| PlayerId(p as u8)).filter(|p| players.is_major(*p)).collect()
}

fn city_states(game: &GameState<Classic>) -> Vec<PlayerId> {
    let players = game.view().auth.players();
    (0..players.count()).map(|p| PlayerId(p as u8)).filter(|p| players.is_city_state(*p)).collect()
}

impl DiploProbe {
    /// Quests and tributes since the last round.
    fn city_state_round(&mut self, game: &GameState<Classic>) {
        let auth = game.view().auth;
        let state = auth.module().city_states();
        let round = auth.turn().round;
        let majors = majors(game);
        let minors = city_states(game);
        let now: Vec<Quest> = state.quests().to_vec();
        for quest in &now {
            let new = !self.quests.iter().any(|q| same(q, quest));
            if new && majors.contains(&quest.major) {
                self.seats.entry(quest.major.0).or_default().quests_given += 1;
            }
        }
        for quest in &self.quests {
            let gone = !now.iter().any(|q| same(q, quest));
            let in_time = quest.end.is_none_or(|end| round <= end);
            if !gone || !in_time || !majors.contains(&quest.major) {
                continue;
            }
            let before = self.influence.get(&(quest.city_state.0, quest.major.0)).copied().unwrap_or(0);
            let after = state.influence(quest.city_state, quest.major);
            if after - before >= cs::quest_reward(quest.kind) - 2 && cs::quest_reward(quest.kind) > 0 {
                self.seats.entry(quest.major.0).or_default().quests_done += 1;
            }
        }
        self.quests = now;
        for major in &majors {
            for minor in &minors {
                let key = (minor.0, major.0);
                self.influence.insert(key, state.influence(*minor, *major));
                let last = state.standing(*minor, *major).last_bullied;
                let before = self.bullied.insert(key, last);
                if before.is_some_and(|b| b != last) && last.is_some() {
                    self.seats.entry(major.0).or_default().tributes += 1;
                }
            }
        }
        if round == MID_ROUND {
            for major in &majors {
                let allies = minors.iter().filter(|m| cs::is_ally(auth, **m, *major)).count() as u32;
                self.seats.entry(major.0).or_default().allies_mid = Some(allies);
            }
        }
    }

    /// The parts' wait for copies.
    fn supply_round(&mut self, game: &GameState<Classic>, rules: &CompiledRuleset<Classic>) {
        let auth = game.view().auth;
        let victory = auth.module().victory();
        let round = auth.turn().round;
        let prerequisites = classic_rules::ai::victory::prerequisite_projects(rules);
        let kinds = classic_rules::ai::victory::part_kinds(rules);
        if prerequisites.is_empty() || kinds.is_empty() {
            return;
        }
        for major in majors(game) {
            if !auth.players().is_alive(major) || !prerequisites.iter().all(|p| victory.project_count(major, *p) > 0) {
                continue;
            }
            let missing = kinds.iter().any(|(kind, project, needed)| {
                let walking = auth.units().iter_cores().filter(|u| u.owner == major && u.kind == *kind).count() as u16;
                victory.project_count(major, *project) + walking < *needed
            });
            let ledger = classic_rules::derived::resources::resource_ledger(auth, rules, major);
            let short = kinds.iter().filter_map(|(kind, _, _)| rules.units.get(kind.0 as usize)).any(|def| def.required_resources.iter().any(|(r, n)| ledger.available(*r) < i32::from(*n)));
            let seat = self.seats.entry(major.0).or_default();
            if seat.parts_round.is_none() {
                seat.parts_round = Some(round);
                seat.parts_lack = short;
            }
            if missing && short {
                seat.blocked += 1;
            }
        }
    }
}

/// The same quest (a quest has no id: its city-state, nation, kind and start).
fn same(a: &Quest, b: &Quest) -> bool {
    a.city_state == b.city_state && a.major == b.major && a.kind == b.kind && a.start == b.start
}

impl MatchRecorder<ClassicBench> for DiploProbe {
    fn round_start(&mut self, game: &GameState<Classic>, rules: &CompiledRuleset<Classic>) {
        self.city_state_round(game);
        self.supply_round(game, rules);
    }

    fn turn_start(&mut self, _game: &GameState<Classic>, _rules: &CompiledRuleset<Classic>, _player: PlayerId, _states: &AiStates) {}

    fn finish(self: Box<Self>, game: &GameState<Classic>, rules: &CompiledRuleset<Classic>, minds: &[&classic_rules::ai::driver::ClassicMind]) {
        let Some(sink) = sink() else { return };
        let auth = game.view().auth;
        let state = auth.module().city_states();
        let minors = city_states(game);
        let mut lines = Vec::new();
        for (index, major) in majors(game).into_iter().enumerate() {
            let variant = minds.get(index).map_or("?", |m| m.variant().name());
            let seat = self.seats.get(&major.0).cloned().unwrap_or_default();
            let allies = minors.iter().filter(|m| cs::is_ally(auth, **m, major)).count();
            let friends = minors.iter().filter(|m| cs::is_friends(auth, rules, **m, major)).count();
            let cs_cities = auth.cities().iter().filter(|(_, c)| c.owner == major && auth.players().is_city_state(c.original_owner)).count();
            let line = serde_json::json!({
                "game": self.name, "seed": self.seed, "player": major.0, "variant": variant,
                "alive": auth.players().is_alive(major),
                "cs_wars": state.major(major).minors_attacked, "cs_cities": cs_cities, "tributes": seat.tributes,
                "quests_given": seat.quests_given, "quests_done": seat.quests_done,
                "allies": allies, "friends": friends, "allies_132": seat.allies_mid,
                "parts_round": seat.parts_round, "parts_lack": seat.parts_lack, "blocked": seat.blocked,
            });
            lines.push(line.to_string());
        }
        let Ok(mut file) = sink.lock() else { return };
        for line in lines {
            let _ = writeln!(file, "{line}");
        }
    }
}
