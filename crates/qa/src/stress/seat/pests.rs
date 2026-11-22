//! The scripted pests of wave 3 (group P11 of work/human-playbook-triage.md): missionaries and
//! prophets that convert the victim's cities (P11), spies that steal its technologies (P12).

use std::collections::BTreeMap;

use classic_rules::ids::{CityHandle, PlayerId, TileId, UnitHandle};
use classic_rules::world::religion::Creed;
use classic_rules::{Command, CompiledRuleset, GameState};

use super::{distance, handle, orders_one_of, Seat};

// ---------------------------------------------------------------------------
// P11 faith

pub(super) struct Faith {
    seat: PlayerId,
    victim: PlayerId,
    units: Vec<u64>,
    spreads: u32,
}

impl Faith {
    pub(super) fn new(seat: PlayerId, victim: PlayerId, units: Vec<u64>) -> Self {
        Faith { seat, victim, units, spreads: 0 }
    }

    /// The victim's cities that do not follow the seat's religion, nearest to `from` first.
    fn targets(&self, game: &GameState, from: TileId) -> Vec<(CityHandle, TileId)> {
        let auth = game.view().auth;
        let Some(religion) = auth.module().religion().player(self.seat).religion else { return Vec::new() };
        let mut cities: Vec<(i32, u64, CityHandle, TileId)> = auth
            .indexes()
            .cities_of(self.victim)
            .iter()
            .filter_map(|h| auth.cities().get(*h).map(|c| (*h, c)))
            .filter(|(h, _)| classic_rules::derived::religion::city_majority(auth, *h) != Some(Creed::Religion(religion)))
            .map(|(h, c)| (distance(game, from, c.tile), c.id.0, h, c.tile))
            .collect();
        cities.sort_unstable_by_key(|(d, id, ..)| (*d, *id));
        cities.into_iter().map(|(.., h, t)| (h, t)).collect()
    }

    /// One unit's turn: spread where it stands, else walk next to the nearest city to convert and
    /// spread there.
    fn preach(&mut self, game: &mut GameState, rules: &CompiledRuleset, unit: UnitHandle) {
        if game.execute(rules, Command::SpreadReligion { unit }).is_ok() {
            self.spreads += 1;
            return;
        }
        let Some(here) = game.view().auth.units().get(unit).map(|u| u.core.tile) else { return };
        let Some(&(_, city)) = self.targets(game, here).first() else { return };
        let map = game.view().auth.map();
        let mut beside: Vec<(i32, u32)> = map
            .dimensions
            .neighbors(city)
            .into_iter()
            .filter(|t| !map.is_water(*t) && game.view().auth.indexes().units_on(*t).is_empty())
            .map(|t| (distance(game, here, t), t.0))
            .collect();
        beside.sort_unstable();
        for (_, tile) in beside.into_iter().take(3) {
            if game.execute(rules, Command::MarchTo { unit, destination: TileId(tile) }).is_ok() {
                break;
            }
        }
        if game.execute(rules, Command::SpreadReligion { unit }).is_ok() {
            self.spreads += 1;
        }
    }
}

impl Seat for Faith {
    fn player(&self) -> PlayerId {
        self.seat
    }

    fn play(&mut self, game: &mut GameState, rules: &CompiledRuleset) {
        for id in self.units.clone() {
            let Some(unit) = handle(game, id) else { continue };
            if game.view().auth.units().get(unit).is_some_and(|u| u.core.owner == self.seat) {
                self.preach(game, rules, unit);
            }
        }
    }

    fn counts(&self) -> BTreeMap<String, f64> {
        BTreeMap::from([("script_spreads".to_owned(), f64::from(self.spreads))])
    }

    fn holds(&self, game: &GameState, command: &Command) -> bool {
        orders_one_of(game, command, &self.units)
    }
}

// ---------------------------------------------------------------------------
// P12 spies

pub(super) struct Spies {
    seat: PlayerId,
    victim: PlayerId,
    stolen: u32,
}

impl Spies {
    pub(super) fn new(seat: PlayerId, victim: PlayerId) -> Self {
        Spies { seat, victim, stolen: 0 }
    }

    /// The victim's cities, the largest first (ties: the lower id): where its science is.
    fn targets(&self, game: &GameState) -> Vec<CityHandle> {
        let auth = game.view().auth;
        let mut cities: Vec<(u32, u64, CityHandle)> = auth.indexes().cities_of(self.victim).iter().filter_map(|h| auth.cities().get(*h).map(|c| (c.population, c.id.0, *h))).collect();
        cities.sort_unstable_by_key(|(pop, id, _)| (std::cmp::Reverse(*pop), *id));
        cities.into_iter().map(|(.., h)| h).collect()
    }

    /// Every steal right spent on a technology of the victim the rules let the seat take.
    fn steal(&mut self, game: &mut GameState, rules: &CompiledRuleset) {
        while game.view().auth.module().espionage().steal_rights(self.seat, self.victim) > 0 {
            let theirs: Vec<classic_rules::ids::TechId> = game.view().auth.players().techs_of(self.victim).to_vec();
            let mine = game.view().auth.players().techs_of(self.seat).to_vec();
            let taken = theirs.into_iter().filter(|t| !mine.contains(t)).any(|tech| game.execute(rules, Command::StealTechnology { player: self.seat, from: self.victim, tech }).is_ok());
            if !taken {
                return;
            }
            self.stolen += 1;
        }
    }
}

impl Seat for Spies {
    fn player(&self) -> PlayerId {
        self.seat
    }

    fn play(&mut self, game: &mut GameState, rules: &CompiledRuleset) {
        self.steal(game, rules);
        let cities = self.targets(game);
        if cities.is_empty() {
            return;
        }
        let count = game.view().auth.module().espionage().spies_of(self.seat).len();
        for spy in 0..count {
            // One spy a city, the largest cities first: each gathers its own intel.
            let city = cities[spy % cities.len()];
            let at = game.view().auth.module().espionage().spies_of(self.seat).get(spy).and_then(|s| s.city);
            if at != Some(city) {
                let _ = game.execute(rules, Command::MoveSpy { player: self.seat, spy: spy as u8, city: Some(city), diplomat: false });
            }
        }
    }

    fn counts(&self) -> BTreeMap<String, f64> {
        BTreeMap::from([("script_stolen".to_owned(), f64::from(self.stolen))])
    }

    /// The seat's spies are the script's: its AI may not move them or stage a coup with them.
    fn holds(&self, _game: &GameState, command: &Command) -> bool {
        match command {
            Command::MoveSpy { player, .. } | Command::StageCoup { player, .. } | Command::StealTechnology { player, .. } => *player == self.seat,
            _ => false,
        }
    }
}
