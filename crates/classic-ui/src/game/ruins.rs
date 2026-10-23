//! Ancient Ruins in the game: the card with the three rewards for the player's unit standing
//! on them, the claim, and the notice of what the ruins gave and where. An AI player's unit is
//! paid by the rules when it enters; a human claims (`Command::ClaimRuins`).
use classic_rules::classic::ClassicChange;
use classic_rules::derived::ruins as place;
use classic_rules::ruins::{Reward, CULTURE, EXPERIENCE, FAITH, GOLD};
use classic_rules::transaction::journal::AppliedCommand;
use classic_rules::Command;
use i18n::{t, tf};

use super::session::Session;
use crate::ui::model::{GameRequest, NoticeKind, RuinsChoice, RuinsOption};

/// The name of a reward on its button.
pub(super) fn reward_label(reward: Reward) -> &'static str {
    match reward {
        Reward::Citizen => t!("classic.game.ruins.label-citizen"),
        Reward::Culture => t!("classic.game.ruins.label-culture"),
        Reward::Faith => t!("classic.game.ruins.label-faith"),
        Reward::Gold => t!("classic.game.ruins.label-gold"),
        Reward::Experience => t!("classic.game.ruins.label-experience"),
        Reward::Map => t!("classic.game.ruins.label-map"),
    }
}

/// What a reward gives, in a short phrase (the line under the button and in the notice).
pub(super) fn reward_gift(reward: Reward) -> String {
    match reward {
        Reward::Citizen => t!("classic.game.ruins.gift-citizen").to_string(),
        Reward::Culture => tf!("classic.game.ruins.gift-culture", n = CULTURE),
        Reward::Faith => tf!("classic.game.ruins.gift-faith", n = FAITH),
        Reward::Gold => tf!("classic.game.ruins.gift-gold", n = GOLD),
        Reward::Experience => tf!("classic.game.ruins.gift-experience", n = EXPERIENCE),
        Reward::Map => t!("classic.game.ruins.gift-map").to_string(),
    }
}

/// The notice text of a reward.
pub(super) fn reward_text(reward: Reward) -> String {
    tf!("classic.game.ruins.notice", gift = reward_gift(reward))
}

impl Session {
    /// A notice for every ruin the player's units explored in `records`, centred on the ruins.
    pub(super) fn notify_ruins(&mut self, records: &[AppliedCommand]) {
        let me = self.me();
        for record in records {
            for change in &record.changes.module_changes {
                if let ClassicChange::RuinsExplored { player, tile, reward } = *change {
                    if player == me {
                        self.notify_at(NoticeKind::Production, reward_text(reward), Some(tile.0));
                    }
                }
            }
        }
    }

    /// The card of the ruins one of the player's units stands on (a human player only; the card
    /// stays shut after "later" until the unit leaves).
    pub(super) fn ruins_choice(&mut self) -> Option<RuinsChoice> {
        let me = self.me();
        let auth = self.game().view().auth;
        let site = if auth.settings().is_ai(me) { None } else { place::first_site(auth, self.rules(), me) };
        let Some((unit, tile)) = site else {
            self.ruins_later = None;
            return None;
        };
        if self.ruins_later == Some(tile) {
            return None;
        }
        let offers = place::offers_at(self.game().view().auth, tile);
        let options = offers
            .into_iter()
            .map(|reward| RuinsOption {
                reward,
                label: reward_label(reward),
                hint: reward_gift(reward),
                availability: self.availability(&Command::ClaimRuins { unit, reward }),
            })
            .collect();
        Some(RuinsChoice { tile, options })
    }

    pub(super) fn ruins_request(&mut self, request: GameRequest) {
        let me = self.me();
        let Some((unit, tile)) = place::first_site(self.game().view().auth, self.rules(), me) else {
            return;
        };
        match request {
            GameRequest::ClaimRuins(reward) => {
                self.submit(Command::ClaimRuins { unit, reward });
            }
            GameRequest::PostponeRuins => self.ruins_later = Some(tile),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use classic_rules::ids::{ImprovementId, TileId};

    use super::*;
    use crate::game::MapButton;
    use crate::ui::screens::{game_rules, prepare_match, MapSize, MatchRequest};

    /// A session whose selected unit stands next to ruins; the tile of the ruins.
    fn next_to_ruins() -> (Session, TileId) {
        let request = MatchRequest { civilization: "core:nation/rome".into(), seed: 4, size: MapSize::Small, players: 3, water: 25 };
        let mut session = Session::new(prepare_match(request).expect("world").game, game_rules());
        let (_, unit) = session.selected_unit().expect("the starting unit is selected");
        let from = unit.tile;
        let (game, rules) = session.harness_game();
        let map = game.view().auth.map();
        let site = map
            .dimensions
            .neighbors(from)
            .into_iter()
            .find(|t| !map.is_water(*t) && map.improvement(*t).flatten().is_none())
            .expect("a free land tile next to the start");
        let ruins = rules.improvements.iter().position(|i| i.key == "core:improvement/ancient-ruins").unwrap();
        game.debug_set_improvement_batch(&rules, &[(site, ImprovementId(ruins as u16))]);
        session.refresh_after_harness();
        (session, site)
    }

    fn ruins_stand(session: &Session, site: TileId) -> bool {
        session.game().view().auth.map().improvement(site).flatten().is_some()
    }

    #[test]
    fn walking_onto_ruins_opens_a_card_with_three_rewards_and_gives_nothing_yet() {
        let (mut session, site) = next_to_ruins();
        assert!(session.hud_model(None).ruins.is_none(), "no card away from the ruins");
        session.click(Some(site), MapButton::Primary);
        let model = session.hud_model(None);
        let card = model.ruins.expect("a card");
        assert_eq!(card.tile, site);
        assert_eq!(card.options.len(), 3);
        assert!(card.options.iter().all(|o| o.availability.is_ready()));
        assert!(model.notices.is_empty(), "nothing is given before the player picks");
        assert!(ruins_stand(&session, site), "the ruins wait");
    }

    #[test]
    fn picking_a_reward_gives_it_leaves_a_notice_with_a_place_and_removes_the_ruins() {
        for pick in 0..3 {
            let (mut session, site) = next_to_ruins();
            session.click(Some(site), MapButton::Primary);
            let card = session.hud_model(None).ruins.expect("a card");
            let reward = card.options[pick].reward;
            session.request(GameRequest::ClaimRuins(reward));
            let model = session.hud_model(None);
            let notice = model.notices.into_iter().next().expect("a notice");
            assert!(notice.text.starts_with("Ancient ruins: "), "{}", notice.text);
            assert_eq!(notice.tile, Some(site.0));
            assert!(!ruins_stand(&session, site), "the ruins are gone");
            assert!(model.ruins.is_none(), "the card closes after the pick");
        }
    }

    #[test]
    fn later_closes_the_card_and_the_ruins_stay() {
        let (mut session, site) = next_to_ruins();
        session.click(Some(site), MapButton::Primary);
        assert!(session.hud_model(None).ruins.is_some());
        session.request(GameRequest::PostponeRuins);
        assert!(session.hud_model(None).ruins.is_none(), "the card stays shut while the unit stands there");
        assert!(ruins_stand(&session, site), "the ruins stay for later");
    }

    #[test]
    fn every_reward_has_a_name_and_notice_text_in_both_languages() {
        for reward in classic_rules::ruins::REWARDS {
            assert!(!reward_label(reward).starts_with("classic."), "{reward:?}");
            assert!(reward_text(reward).starts_with("Ancient ruins: "), "{}", reward_text(reward));
            i18n::with_language("ru", || {
                crate::game::reasons::only_cyrillic(reward_label(reward));
                crate::game::reasons::only_cyrillic(&reward_gift(reward).replace(char::is_numeric, "").replace('+', ""));
                assert!(reward_text(reward).starts_with("\u{414}\u{440}\u{435}\u{432}\u{43d}\u{438}\u{435} \u{440}\u{443}\u{438}\u{43d}\u{44b}: ")); // Russian: "Ancient ruins: "
            });
        }
    }
}
