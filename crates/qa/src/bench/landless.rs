//! The landless seat of a tuning game (`ai-tournament --landless-seat N`, a measurement): at the start of each game
//! the settlers (founder units) of one nation's seat are removed, its other start units stay, and the report says in
//! how many games that seat owns a city at the end of round 100 and 200. A seat whose every unit is a founder keeps
//! them (it would have no unit and would die at once) and the report counts those games apart. Without the option
//! nothing here runs and no game, hash or report changes.

use kernel::ids::PlayerId;
use kernel::module::RulesModule;
use kernel::rules::CompiledRuleset;
use kernel::world::state::GameState;

/// The rounds at the end of which the seat's cities are counted.
pub const CHECK_ROUNDS: [u32; 2] = [100, 200];

/// What was done to the seat of one game and what it owned.
#[derive(Debug, Clone, Default)]
pub struct LandlessGame {
    /// The index of the seat among the major players.
    pub seat: usize,
    /// Founder units removed at the start.
    pub removed: u32,
    /// The seat had no unit besides its founders: kept as is.
    pub kept: bool,
    /// Whether the seat owned a city at the end of each of `CHECK_ROUNDS`; None: the game did not reach the round.
    pub owned: [Option<bool>; 2],
    /// The seat founded or took a city at some time (a city of its own founding: `SeatResult::founded`).
    pub founded: bool,
    /// Diagnostic lines of the seat per round (only with AURELIA_LANDLESS_TRACE set).
    pub trace: Vec<String>,
}

/// Removes the founders of `player` unless they are all it has. Returns the log of the game.
pub fn strip<M: RulesModule>(game: &mut GameState<M>, rules: &CompiledRuleset<M>, seat: usize, player: PlayerId) -> LandlessGame {
    let (founders, total) = {
        let auth = game.view().auth;
        let all = auth.indexes().units_of(player).to_vec();
        let founders: Vec<_> = all.iter().copied().filter(|h| auth.units().get(*h).is_some_and(|u| rules.units[u.core.kind.0 as usize].founder)).collect();
        (founders, all.len())
    };
    let mut log = LandlessGame { seat, ..LandlessGame::default() };
    if founders.is_empty() {
        return log;
    }
    if founders.len() == total {
        log.kept = true;
        return log;
    }
    log.removed = founders.len() as u32;
    for h in founders {
        game.debug_remove_unit(rules, h);
    }
    log
}

impl LandlessGame {
    /// After a round: whether the seat owns a city at the end of a checked round.
    pub fn after_round<M: RulesModule>(&mut self, game: &GameState<M>, player: PlayerId, now: u32) {
        if std::env::var_os("AURELIA_LANDLESS_TRACE").is_some() {
            let auth = game.view().auth;
            let units: Vec<String> = auth.indexes().units_of(player).iter().filter_map(|h| auth.units().get(*h)).map(|u| format!("k{}@{}h{}", u.core.kind.0, u.core.tile.0, u.core.hp)).collect();
            let wars: Vec<u8> = (0..auth.players().count() as u8).filter(|p| PlayerId(*p) != player && auth.diplomacy().at_war(player, PlayerId(*p))).collect();
            self.trace.push(format!("r{now} units [{}] wars {:?}", units.join(" "), wars));
        }
        if let Some(at) = CHECK_ROUNDS.iter().position(|r| *r == now) {
            self.owned[at] = Some(!game.view().auth.indexes().cities_of(player).is_empty());
        }
    }
}

/// The report's lines for the games of a run (None: no game had a landless seat).
pub fn summary(games: &[Option<LandlessGame>]) -> Option<String> {
    let all: Vec<&LandlessGame> = games.iter().flatten().collect();
    if all.is_empty() {
        return None;
    }
    let seat = all[0].seat;
    let kept = all.iter().filter(|g| g.kept).count();
    let stripped = all.iter().filter(|g| g.removed > 0).count();
    let mut out = format!("landless seat {seat} (index among the major players, from 0): {} games; settlers removed in {stripped}, kept because the seat had no other unit in {kept}.\n", all.len());
    for (i, round) in CHECK_ROUNDS.iter().enumerate() {
        let reached: Vec<bool> = all.iter().filter_map(|g| g.owned[i]).collect();
        let owns = reached.iter().filter(|b| **b).count();
        if reached.is_empty() {
            out.push_str(&format!("- owns a city at the end of round {round}: no game reached it\n"));
        } else {
            out.push_str(&format!("- owns a city at the end of round {round}: {owns} of {} games that reached it ({:.1} %)\n", reached.len(), 100.0 * owns as f64 / reached.len() as f64));
        }
    }
    let founded = all.iter().filter(|g| g.founded).count();
    out.push_str(&format!("- founded a city at any time: {founded} of {} games ({:.1} %)\n", all.len(), 100.0 * founded as f64 / all.len() as f64));
    Some(out)
}
