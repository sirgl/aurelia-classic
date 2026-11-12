//! A war or a denunciation waits for the player's yes, as in the original game: a strike at a
//! unit or city of a nation at peace (the right click or the Attack order), the war button and
//! the denunciation button of the diplomacy screen open a card that says what the act costs;
//! nothing happens until the player confirms it (`GameRequest::Confirm(true)`). No, Esc
//! (`Session::cancel_aim`) or a new game leave everything as it was.
use classic_rules::ids::PlayerId;
use classic_rules::Command;
use i18n::{t, tf};

use super::names;
use super::session::Session;
use crate::ui::model::ConfirmCard;

/// What waits for the yes.
#[derive(Debug, Clone)]
pub(crate) enum Confirmation {
    /// A strike at a nation at peace: war on `enemy`, then `strike`.
    Attack { enemy: PlayerId, strike: Command },
    /// War on the nation from the diplomacy screen.
    War(PlayerId),
    /// The denunciation of the nation from the diplomacy screen.
    Denounce(PlayerId),
}

impl Confirmation {
    fn command(&self, me: PlayerId) -> Command {
        match *self {
            Confirmation::Attack { enemy, .. } | Confirmation::War(enemy) => Command::DeclareWar { player: me, enemy },
            Confirmation::Denounce(target) => Command::Denounce { player: me, target },
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
        match what {
            Confirmation::Attack { enemy, strike } => self.declare_war_and_strike(enemy, strike),
            Confirmation::War(enemy) => self.declare_war(enemy),
            Confirmation::Denounce(target) => self.denounce(target),
        }
    }
}
