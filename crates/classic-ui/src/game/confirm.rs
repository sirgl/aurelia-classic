//! A war or a denunciation waits for the player's yes, as in the original game: a strike at a
//! unit or city of a nation at peace (the right click or the Attack order), the war button and
//! the denunciation button of the diplomacy screen open a card that says what the act costs;
//! nothing happens until the player confirms it (`GameRequest::Confirm(true)`). No, Esc
//! (`Session::cancel_aim`) or a new game leave everything as it was. Disbanding a unit (its
//! Disband order) and selling a building (the city screen) wait for the yes the same way, as in
//! Unciv (UiOnlyUnitActions.getDisbandAction, ConstructionInfoTable.sellBuildingClicked): the
//! card tells the gold.
use classic_rules::ids::{BuildingKindId, CityHandle, PlayerId, UnitHandle};
use classic_rules::Command;
use i18n::{t, tf};

use super::names;
use super::session::Session;
use crate::ui::model::{ConfirmCard, NoticeKind};

/// What waits for the yes.
#[derive(Debug, Clone)]
pub(crate) enum Confirmation {
    /// A strike at a nation at peace: war on `enemy`, then `strike`.
    Attack { enemy: PlayerId, strike: Command },
    /// War on the nation from the diplomacy screen.
    War(PlayerId),
    /// The denunciation of the nation from the diplomacy screen.
    Denounce(PlayerId),
    /// Disband the player's unit.
    Disband(UnitHandle),
    /// Sell the building of the player's city.
    SellBuilding { city: CityHandle, building: BuildingKindId },
}

impl Confirmation {
    fn command(&self, me: PlayerId) -> Command {
        match *self {
            Confirmation::Attack { enemy, .. } | Confirmation::War(enemy) => Command::DeclareWar { player: me, enemy },
            Confirmation::Denounce(target) => Command::Denounce { player: me, target },
            Confirmation::Disband(unit) => Command::DisbandUnit { unit },
            Confirmation::SellBuilding { city, building } => Command::SellBuilding { player: me, city, building },
        }
    }
}

impl Session {
    /// Open the card for `what` when the rules would allow it; else say why, as the button does.
    pub(super) fn ask_first(&mut self, what: Confirmation) {
        if let Err(error) = self.check(&what.command(self.me())) {
            self.message = Some(names::reason(self.rules(), &error));
            return;
        }
        self.confirm = Some(what);
    }

    /// The card of what waits for the yes.
    pub(super) fn confirm_card(&self) -> Option<ConfirmCard> {
        let what = self.confirm.as_ref()?;
        let cancel = t!("classic.game.diplomacy.confirm-cancel").to_string();
        let card = match what {
            Confirmation::Attack { enemy, .. } => {
                let name = self.player_name(*enemy);
                ConfirmCard {
                    title: tf!("classic.game.diplomacy.confirm-war-title", name = name.as_str()),
                    line: tf!("classic.game.diplomacy.confirm-attack-line", name = name.as_str()),
                    confirm: t!("classic.game.diplomacy.confirm-attack").to_string(),
                    cancel,
                }
            }
            Confirmation::War(enemy) => {
                let name = self.player_name(*enemy);
                ConfirmCard {
                    title: tf!("classic.game.diplomacy.confirm-war-title", name = name.as_str()),
                    line: tf!("classic.game.diplomacy.confirm-war-line", name = name.as_str()),
                    confirm: t!("classic.game.diplomacy.confirm-war").to_string(),
                    cancel,
                }
            }
            Confirmation::Denounce(target) => {
                let name = self.player_name(*target);
                ConfirmCard {
                    title: tf!("classic.game.diplomacy.confirm-denounce-title", name = name.as_str()),
                    line: tf!("classic.game.diplomacy.confirm-denounce-line", name = name.as_str()),
                    confirm: t!("classic.game.diplomacy.confirm-denounce").to_string(),
                    cancel,
                }
            }
            Confirmation::Disband(unit) => {
                let auth = self.game().view().auth;
                let kind = auth.units().get(*unit)?.core.kind;
                let name = names::unit(self.rules(), kind);
                let gold = classic_rules::derived::disband::disband_status(auth, self.rules(), *unit).unwrap_or(0);
                ConfirmCard {
                    title: tf!("classic.game.orders.confirm-disband-title", name = name.as_str()),
                    line: if gold > 0 {
                        tf!("classic.game.orders.confirm-disband-gold", gold = gold)
                    } else {
                        t!("classic.game.orders.confirm-disband-line").to_string()
                    },
                    confirm: t!("classic.game.orders.confirm-disband").to_string(),
                    cancel,
                }
            }
            Confirmation::SellBuilding { city, building } => {
                let core = self.game().view().auth.cities().get(*city)?;
                let def = self.rules().buildings.get(building.0 as usize)?;
                let name = names::named("building", &def.name, &def.key);
                let gold = classic_rules::derived::building_sale::sale_gold(self.rules(), *building);
                let weakens = classic_rules::derived::building_sale::sale_weakens_defence(self.game().view().auth, self.rules(), *city, *building);
                let mut line = tf!("classic.game.city.confirm-sell-line", city = core.name.as_str(), gold = gold);
                if weakens {
                    line = format!("{line} {}", t!("classic.game.city.confirm-sell-defence"));
                }
                ConfirmCard {
                    title: tf!("classic.game.city.confirm-sell-title", name = name.as_str()),
                    line,
                    confirm: t!("classic.game.city.confirm-sell").to_string(),
                    cancel,
                }
            }
        };
        Some(card)
    }

    /// The player's answer: yes does what waited, no leaves it.
    pub(super) fn answer_confirm(&mut self, yes: bool) {
        let Some(what) = self.confirm.take() else {
            return;
        };
        if !yes {
            return;
        }
        // The yes is to what the card showed: the world may have moved since (a unit gone, the
        // target out of reach, the war already declared), and a war without its strike, or a
        // refused one, must not follow from an old yes.
        let allowed = match &what {
            Confirmation::Attack { enemy, strike } => self.legal_after_war(*enemy, strike),
            other => self.check(&other.command(self.me())),
        };
        if let Err(error) = allowed {
            self.message = Some(names::reason(self.rules(), &error));
            return;
        }
        match what {
            Confirmation::Attack { enemy, strike } => self.declare_war_and_strike(enemy, strike),
            Confirmation::War(enemy) => self.declare_war(enemy),
            Confirmation::Denounce(target) => self.denounce(target),
            Confirmation::Disband(unit) => self.disband(unit),
            Confirmation::SellBuilding { city, building } => self.sell_building(city, building),
        }
    }

    /// The yes to the Disband card: the unit goes, the notice tells the gold.
    fn disband(&mut self, unit: UnitHandle) {
        let auth = self.game().view().auth;
        let Some(kind) = auth.units().get(unit).map(|u| u.core.kind) else {
            return;
        };
        let gold = classic_rules::derived::disband::disband_status(auth, self.rules(), unit).unwrap_or(0);
        let name = names::unit(self.rules(), kind);
        if self.submit(Command::DisbandUnit { unit }) {
            let text = if gold > 0 {
                tf!("classic.game.orders.disbanded-gold", name = name.as_str(), gold = gold)
            } else {
                tf!("classic.game.orders.disbanded", name = name.as_str())
            };
            self.notify(NoticeKind::Production, text);
        }
    }

    /// The yes to the Sell card: the building goes, the notice tells the gold.
    fn sell_building(&mut self, city: CityHandle, building: BuildingKindId) {
        let Some(def) = self.rules().buildings.get(building.0 as usize) else {
            return;
        };
        let name = names::named("building", &def.name, &def.key);
        let gold = classic_rules::derived::building_sale::sale_gold(self.rules(), building);
        if self.submit(Command::SellBuilding { player: self.me(), city, building }) {
            self.notify(NoticeKind::Production, tf!("classic.game.city.sold", name = name.as_str(), gold = gold));
        }
    }
}
