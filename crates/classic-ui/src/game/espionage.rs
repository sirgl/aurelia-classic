//! Espionage overview: the player's spies, the cities they may be sent to (its own for
//! counter-intelligence, the known cities of the nations and city-states it met,
//! foreign capitals as a diplomat), the technologies its steal rights allow, and coups.
use classic_rules::ClassicGame;
use classic_rules::ids::{CityHandle, PlayerId, TechId};
use classic_rules::knowledge::observation::SpyInfo;
use classic_rules::world::espionage::SpyState;
use classic_rules::world::player::PlayerKind;
use classic_rules::Command;

use i18n::{t, tf};

use super::names;
use super::session::Session;
use crate::ui::model::{
    Availability, EspionageView, GameRequest, NoticeKind, SpyCard, SpyMove, SpyTarget, StealCard,
};
use classic_rules::world::player::ClassicKind;

impl Session {
    pub fn espionage_view(&mut self) -> EspionageView {
        let observation = self.game().ai_observation(self.rules(), self.me());
        let spies: Vec<SpyInfo> = observation.spies.clone();
        let steal: Vec<(PlayerId, Vec<TechId>)> = observation.steal.clone();
        let targets = self.spy_targets();
        let cards = spies.iter().map(|s| self.spy_card(s, &targets)).collect();
        let steal = steal
            .iter()
            .map(|(from, techs)| self.steal_card(*from, techs))
            .collect();
        EspionageView {
            spies: cards,
            targets,
            steal,
        }
    }

    /// Own cities, then the known cities of every player met (capitals first).
    fn spy_targets(&self) -> Vec<SpyTarget> {
        let me = self.me();
        let view = self.game().player_view(self.rules(), me);
        let observation = self.game().ai_observation(self.rules(), me);
        let met: Vec<PlayerId> = observation.relations.iter().map(|r| r.other).collect();
        let auth = self.game().view().auth;
        let mut out = Vec::new();
        for (handle, core) in auth.cities().iter() {
            let own = core.owner == me;
            let known = met.contains(&core.owner) && view.is_known(core.tile);
            if !own && !known {
                continue;
            }
            let role = self.target_role(core);
            out.push(SpyTarget {
                city: handle,
                name: core.name.clone(),
                owner: self.player_name(core.owner),
                role,
            });
        }
        out.sort_by_key(|t| (role_order(t.role), t.owner.clone(), t.name.clone()));
        out
    }

    fn target_role(&self, core: &classic_rules::world::city::CityCore) -> &'static str {
        let kind = self.game().view().auth.players().kind_of(core.owner);
        if core.owner == self.me() {
            return t!("classic.game.espionage.role-own");
        }
        if matches!(kind, PlayerKind::Module(ClassicKind::CityState(_))) {
            return t!("classic.game.espionage.role-city-state");
        }
        if core.original_capital && core.original_owner == core.owner {
            return t!("classic.game.espionage.role-capital");
        }
        t!("classic.game.espionage.role-city")
    }

    fn spy_card(&mut self, info: &SpyInfo, targets: &[SpyTarget]) -> SpyCard {
        let player = self.me();
        let spy = info.index;
        let city = info.city.map(|c| self.spy_city(c));
        let moves = targets.iter().map(|t| self.spy_move(spy, t)).collect();
        SpyCard {
            index: spy,
            name: self.spy_name(info.name),
            rank: rank_name(info.rank),
            state: state_name(info.state),
            city,
            diplomat: info.diplomat,
            coup_chance: info.coup_chance,
            coup: if info.coup_chance.is_none() {
                Availability::Blocked(super::reasons::no_coup().into())
            } else {
                self.availability(&Command::StageCoup { player, spy })
            },
            home: self.spy_availability(
                spy,
                None,
                false,
                &Command::MoveSpy { player, spy, city: None, diplomat: false },
            ),
            moves,
        }
    }

    fn spy_move(&mut self, spy: u8, target: &SpyTarget) -> SpyMove {
        let command = |diplomat| Command::MoveSpy {
            player: self.me(),
            spy,
            city: Some(target.city),
            diplomat,
        };
        let (as_spy, as_diplomat) = (command(false), command(true));
        let diplomat = if target.role == t!("classic.game.espionage.role-capital") {
            Some(self.spy_availability(spy, Some(target.city), true, &as_diplomat))
        } else {
            None
        };
        SpyMove {
            spy: self.spy_availability(spy, Some(target.city), false, &as_spy),
            diplomat,
        }
    }

    /// The verdict on a spy's move, the rules' typed reason first.
    fn spy_availability(&mut self, spy: u8, city: Option<CityHandle>, diplomat: bool, command: &Command) -> Availability {
        let auth = self.game().view().auth;
        match classic_rules::world::espionage::move_problem(auth, self.me(), usize::from(spy), city, diplomat) {
            Some(block) => Availability::Blocked(super::reasons::spy(block).into()),
            None => self.availability(command),
        }
    }

    /// The name of one of the player's spies (its nation's list).
    pub(super) fn spy_name(&self, name: u8) -> String {
        let nation = self.game().view().auth.players().nation_of(self.me());
        classic_rules::world::espionage::spy_name(self.rules(), nation, name)
    }

    /// The name of the player's `index`-th spy slot, as it stands now.
    fn spy_slot_name(&self, spy: u8) -> String {
        let auth = self.game().view().auth;
        match auth.module().espionage().spies_of(self.me()).get(usize::from(spy)) {
            Some(slot) => self.spy_name(slot.name),
            None => tf!("classic.game.espionage.spy-n", n = u32::from(spy) + 1),
        }
    }

    /// The names of the player's living spies in the city on `tile` (the banner shows them).
    pub(super) fn spies_in(&self, tile: classic_rules::ids::TileId) -> Vec<String> {
        let auth = self.game().view().auth;
        auth.module()
            .espionage()
            .spies_of(self.me())
            .iter()
            .filter(|s| s.state != SpyState::Dead)
            .filter(|s| s.city.and_then(|c| auth.cities().get(c)).is_some_and(|c| c.tile == tile))
            .map(|s| self.spy_name(s.name))
            .collect()
    }

    /// Name, rank and state of each own spy (what the turn report compares).
    pub(super) fn spy_marks(&self) -> Vec<(u8, u8, SpyState)> {
        let auth = self.game().view().auth;
        auth.module().espionage().spies_of(self.me()).iter().map(|s| (s.name, s.rank, s.state)).collect()
    }

    /// A spy killed, a new one in its place, a promotion.
    pub(super) fn report_spies(&mut self, before: &[(u8, u8, SpyState)]) {
        let now = self.spy_marks();
        let mut notes = Vec::new();
        for (old, new) in before.iter().zip(&now) {
            let name = self.spy_name(new.0);
            if old.2 != SpyState::Dead && new.2 == SpyState::Dead {
                notes.push(tf!("classic.game.espionage.killed", name = self.spy_name(old.0)));
            } else if old.0 != new.0 {
                notes.push(tf!("classic.game.espionage.joins", name = name));
            } else if new.1 > old.1 {
                notes.push(tf!("classic.game.espionage.promoted", name = name, rank = rank_name(new.1)));
            }
        }
        for text in notes {
            self.notify(NoticeKind::Diplomacy, text);
        }
    }

    /// "Thebes (Egypt)".
    fn spy_city(&self, city: CityHandle) -> String {
        let auth = self.game().view().auth;
        match auth.cities().get(city) {
            Some(core) => format!("{} ({})", core.name, self.player_name(core.owner)), // i18n-ok: a city and its owner
            None => t!("classic.game.espionage.unknown-city").into(),
        }
    }

    fn steal_card(&self, from: PlayerId, techs: &[TechId]) -> StealCard {
        StealCard {
            from,
            nation: self.player_name(from),
            techs: techs
                .iter()
                .map(|t| (*t, names::tech(self.rules(), *t)))
                .collect(),
        }
    }

    pub(super) fn espionage_request(&mut self, request: GameRequest) {
        let player = self.me();
        let (command, text) = match request {
            GameRequest::MoveSpy {
                spy,
                city,
                diplomat,
            } => {
                let text = match city {
                    Some(c) => tf!("classic.game.espionage.travels", name = self.spy_slot_name(spy), city = self.spy_city(c)),
                    None => tf!("classic.game.espionage.returns", name = self.spy_slot_name(spy)),
                };
                let command = Command::MoveSpy {
                    player,
                    spy,
                    city,
                    diplomat,
                };
                (command, text)
            }
            GameRequest::StealTechnology { from, tech } => (
                Command::StealTechnology { player, from, tech },
                tf!("classic.game.espionage.stolen",
                    tech = names::tech(self.rules(), tech),
                    name = self.player_name(from)
                ),
            ),
            GameRequest::StageCoup(spy) => (
                Command::StageCoup { player, spy },
                tf!("classic.game.espionage.coup", name = self.spy_slot_name(spy)),
            ),
            _ => return,
        };
        if self.submit(command) {
            self.notify(NoticeKind::Diplomacy, text);
        }
    }
}

fn role_order(role: &str) -> u8 {
    if role == t!("classic.game.espionage.role-own") {
        0
    } else if role == t!("classic.game.espionage.role-capital") {
        1
    } else if role == t!("classic.game.espionage.role-city") {
        2
    } else {
        3
    }
}

fn rank_name(rank: u8) -> &'static str {
    match rank {
        0 => t!("classic.game.espionage.rank-recruit"),
        1 => t!("classic.game.espionage.rank-agent"),
        _ => t!("classic.game.espionage.rank-special-agent"),
    }
}

fn state_name(state: SpyState) -> &'static str {
    match state {
        SpyState::Unassigned => t!("classic.game.espionage.state-unassigned"),
        SpyState::Travelling => t!("classic.game.espionage.state-travelling"),
        SpyState::Surveillance => t!("classic.game.espionage.state-surveillance"),
        SpyState::GatheringIntel => t!("classic.game.espionage.state-gathering-intel"),
        SpyState::RigElection => t!("classic.game.espionage.state-rig-election"),
        SpyState::CounterIntel => t!("classic.game.espionage.state-counter-intel"),
        SpyState::MakingIntroductions => t!("classic.game.espionage.state-making-introductions"),
        SpyState::Schmooze => t!("classic.game.espionage.state-schmooze"),
        SpyState::Dead => t!("classic.game.espionage.state-dead"),
    }
}
