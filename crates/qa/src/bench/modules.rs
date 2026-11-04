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
}

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
    /// At the start of each player's turn (the world before its first command).
    fn turn_start(&mut self, game: &GameState<R::M>, rules: &CompiledRuleset<R::M>, player: PlayerId);
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

    fn turn_start(&mut self, game: &GameState<classic_rules::classic::Classic>, rules: &CompiledRuleset<classic_rules::classic::Classic>, player: PlayerId) {
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
        crate::bench::play::SpaceProgress {
            project_done: !prerequisites.is_empty() && prerequisites.iter().all(|p| victory.project_count(player, *p) > 0),
            parts: parts.iter().map(|(p, needed)| u32::from(victory.project_count(player, *p).min(*needed))).sum(),
        }
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

    fn by_agreement(command: &classic_rules::Command) -> bool {
        matches!(command, classic_rules::Command::RespondDeal { accept: true, .. })
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
