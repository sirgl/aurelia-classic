//! Combat beyond the plain attack: the odds of the selected unit's attack on the hovered
//! tile (the core's combat preview), promotions, upgrades, siege set-up, pillage and
//! interception, and the decision on a captured city.
use classic_rules::PlayerQueries;
use classic_rules::combat::exchange::{CombatPreview, Verdict};
use classic_rules::command::CityDecision;
use classic_rules::derived::great_people::GreatPersonAction;
use classic_rules::ids::{CityHandle, PromotionId, TileId, UnitHandle};
use classic_rules::world::city::CityStatus;
use classic_rules::Command;

use i18n::{t, tf};

use super::names;
use super::session::Session;
use crate::ui::model::{
    Action, Availability, CaptureChoice, CaptureOption, CombatOdds, GameRequest, NoticeKind,
    Outlook, PromotionChoice,
};

/// The decisions on a captured city with their button text and hint.
fn decisions() -> [(CityDecision, &'static str, &'static str); 4] {
    [
        (CityDecision::Annex, t!("classic.game.combat.annex"), t!("classic.game.combat.annex-hint")),
        (CityDecision::Puppet, t!("classic.game.combat.puppet"), t!("classic.game.combat.puppet-hint")),
        (CityDecision::Raze, t!("classic.game.combat.raze"), t!("classic.game.combat.raze-hint")),
        (CityDecision::Liberate, t!("classic.game.combat.liberate"), t!("classic.game.combat.liberate-hint")),
    ]
}

impl Session {
    /// Odds of the selected unit's attack on the hovered tile: a visible unit or city
    /// of a civilization at war with the player, within the unit's reach.
    pub(super) fn combat_odds(&self, hover: Option<u32>) -> Option<CombatOdds> {
        if let Some(city) = self.city_strike {
            return self.city_strike_odds(city, TileId(hover?));
        }
        let (unit, core) = self.selected_unit()?;
        let tile = TileId(hover?);
        let (_, enemy) = self.attack_target(unit, tile)?;
        let auth = self.game().view().auth;
        if !auth.diplomacy().at_war(self.me(), enemy) {
            return None;
        }
        let view = self.game().player_view(self.rules(), self.me());
        let preview = view.attack_preview(unit, tile)?;
        let ranged = self
            .rules()
            .units
            .get(core.kind.0 as usize)
            .is_some_and(|d| d.ranged_strength > 0);
        Some(CombatOdds {
            attacker: names::unit(self.rules(), core.kind),
            defender: self.defender_name(tile),
            ranged,
            ..odds(&preview)
        })
    }

    /// Odds of the aiming city's strike on the hovered tile (the core's
    /// city strike preview).
    fn city_strike_odds(&self, city: CityHandle, tile: TileId) -> Option<CombatOdds> {
        let view = self.game().player_view(self.rules(), self.me());
        let preview = view.city_strike_preview(city, tile)?;
        let name = self.game().view().auth.cities().get(city)?.name.clone();
        Some(CombatOdds {
            attacker: name,
            defender: self.defender_name(tile),
            ranged: true,
            ..odds(&preview)
        })
    }

    /// Whether the own city could strike a visible enemy now (the banner's
    /// bow): its strike is ready and a target stands within range.
    pub(super) fn city_can_strike(&self, city: CityHandle) -> bool {
        let auth = self.game().view().auth;
        let Some(core) = auth.cities().get(city) else {
            return false;
        };
        let resisting = core.resistance_turns > 0 || core.status == CityStatus::AwaitingDecision;
        if core.owner != self.me() || core.strike_made || resisting {
            return false;
        }
        let view = self.game().player_view(self.rules(), self.me());
        let dims = auth.map().dimensions;
        let range = classic_rules::combat::city::CITY_ATTACK_RANGE;
        classic_rules::math::tiles_in_hex_radius(&dims, core.tile, range).into_iter().any(|tile| {
            let foreign = auth.indexes().units_on(tile).iter().any(|u| auth.units().get(*u).is_some_and(|v| v.core.owner != self.me()));
            foreign && view.city_strike_damage(city, tile).is_some()
        })
    }

    /// The city on the tile, else the first foreign unit there.
    fn defender_name(&self, tile: TileId) -> String {
        let auth = self.game().view().auth;
        if let Some(city) = auth.indexes().city_on_tile(tile) {
            if let Some(core) = auth.cities().get(city) {
                return core.name.clone();
            }
        }
        for handle in auth.indexes().units_on(tile) {
            let Some(view) = auth.units().get(*handle) else {
                continue;
            };
            if view.core.owner != self.me() {
                return names::unit(self.rules(), view.core.kind);
            }
        }
        t!("classic.game.combat.enemy").into()
    }

    /// Promotions the core offers and accepts for this unit now (`promotions_available`
    /// also lists ones of other unit roles; `Promote` validation filters them).
    pub(super) fn promotion_choices(&mut self, unit: UnitHandle) -> Vec<PromotionChoice> {
        let view = self.game().player_view(self.rules(), self.me());
        let offered = view.promotions_available(unit);
        let mut out = Vec::new();
        for id in offered {
            let command = Command::Promote {
                unit,
                promotion: id,
            };
            if self.check(&command).is_err() {
                continue;
            }
            out.push(PromotionChoice {
                id,
                key: self.rules().promotions[id.0 as usize].key.clone(),
                name: promotion_name(self.rules(), id),
            });
        }
        out
    }

    /// Combat orders a unit has only now: a promotion to take, an upgrade offer, a
    /// siege set-up, an improvement to pillage, interception.
    pub(super) fn situational_actions(&mut self, unit: UnitHandle) -> Vec<Action> {
        let promotable = !self.promotion_choices(unit).is_empty();
        let view = self.game().player_view(self.rules(), self.me());
        let mut out = Vec::new();
        if promotable {
            out.push(Action::Promote);
        }
        if view.upgrade_offer(unit).is_some() {
            out.push(Action::Upgrade);
        }
        if view.must_set_up_first(unit) {
            out.push(Action::SetUp);
        }
        if self.check(&Command::Pillage { unit }).is_ok() {
            out.push(Action::Pillage);
        }
        if self.check(&Command::Intercept { unit }).is_ok() {
            out.push(Action::Intercept);
        }
        // A sweeper keeps its button when it cannot sweep now (the button says why).
        if classic_rules::world::military::can_air_sweep(self.game().view().auth, self.rules(), unit) {
            out.push(Action::Sweep);
        }
        if self.check(&Command::PlunderTradeRoute { unit }).is_ok() {
            out.push(Action::Plunder);
        }
        if self.check(&Command::SellExoticGoods { unit }).is_ok() {
            out.push(Action::SellGoods);
        }
        let buy = Command::GreatPersonAction { unit, action: GreatPersonAction::BuyCityState };
        if self.check(&buy).is_ok() {
            out.push(Action::BuyCityState);
        }
        out
    }

    /// Availability and detail of the situational orders.
    pub(super) fn combat_action_state(
        &mut self,
        unit: UnitHandle,
        action: Action,
    ) -> (Availability, Option<String>) {
        match action {
            Action::Promote => {
                let count = self.promotion_choices(unit).len();
                (
                    Availability::Ready,
                    Some(tf!("classic.game.combat.promotions-to-choose", n = count)),
                )
            }
            Action::Upgrade => (
                self.availability(&Command::UpgradeUnit { unit }),
                self.upgrade_label(unit),
            ),
            Action::SetUp => (
                self.availability(&Command::SetUpRangedAttack { unit }),
                None,
            ),
            Action::Pillage => (self.availability(&Command::Pillage { unit }), None),
            Action::Sweep => (self.sweep_availability(unit), None),
            _ => (self.availability(&Command::Intercept { unit }), None),
        }
    }

    fn upgrade_label(&self, unit: UnitHandle) -> Option<String> {
        let view = self.game().player_view(self.rules(), self.me());
        let (kind, price) = view.upgrade_offer(unit)?;
        let name = names::unit(self.rules(), kind);
        Some(tf!("classic.game.combat.upgrade-offer", name = name, price = price))
    }

    /// The notice of an upgrade that just happened, from the offer taken before it.
    fn upgrade_done_label(&self, unit: UnitHandle) -> Option<String> {
        let view = self.game().player_view(self.rules(), self.me());
        let (kind, price) = view.upgrade_offer(unit)?;
        let name = names::unit(self.rules(), kind);
        Some(tf!("classic.game.combat.upgrade-done", name = name, price = price))
    }

    /// Situational orders of the selected unit.
    pub(super) fn combat_order(&mut self, unit: UnitHandle, action: Action) {
        let command = match action {
            // The panel opens the promotion list; the pick is a `Promote` request.
            Action::Promote => return,
            Action::Upgrade => Command::UpgradeUnit { unit },
            Action::SetUp => Command::SetUpRangedAttack { unit },
            Action::Pillage => Command::Pillage { unit },
            _ => Command::Intercept { unit },
        };
        let label = self.upgrade_done_label(unit);
        if self.submit(command) && action == Action::Upgrade {
            self.notify(NoticeKind::Combat, label.unwrap_or_default());
        }
    }

    /// The first own city that waits for the conqueror's decision.
    pub(super) fn capture_choice(&mut self) -> Option<CaptureChoice> {
        let me = self.me();
        let auth = self.game().view().auth;
        let (city, core) = auth
            .cities()
            .iter()
            .find(|(_, c)| c.owner == me && c.status == CityStatus::AwaitingDecision)?;
        let (tile, name) = (core.tile, core.name.clone());
        let founder = self.player_name(core.original_owner);
        let options = decisions()
            .iter()
            .map(|(decision, label, hint)| CaptureOption {
                decision: *decision,
                label,
                hint,
                availability: self.decision_availability(city, *decision),
            })
            .collect();
        Some(CaptureChoice {
            tile,
            city: name,
            founder,
            options,
        })
    }

    fn decision_availability(&mut self, city: CityHandle, decision: CityDecision) -> Availability {
        let command = Command::DecideCity {
            player: self.me(),
            city,
            decision,
        };
        self.availability(&command)
    }

    pub(super) fn combat_request(&mut self, request: GameRequest) {
        match request {
            GameRequest::Promote(promotion) => self.promote(promotion),
            GameRequest::DecideCity { tile, decision } => self.decide_city(tile, decision),
            _ => {}
        }
    }

    fn promote(&mut self, promotion: PromotionId) {
        let Some((unit, core)) = self.selected_unit() else {
            return;
        };
        let name = names::unit(self.rules(), core.kind);
        let text = tf!("classic.game.combat.promoted",
            name = name,
            promotion = promotion_name(self.rules(), promotion)
        );
        if self.submit(Command::Promote { unit, promotion }) {
            self.notify(NoticeKind::Combat, text);
        }
    }

    fn decide_city(&mut self, tile: TileId, decision: CityDecision) {
        let Some(city) = self.own_city_at(tile) else {
            return;
        };
        let name = self
            .game()
            .view()
            .auth
            .cities()
            .get(city)
            .map(|c| c.name.clone())
            .unwrap_or_default();
        let command = Command::DecideCity {
            player: self.me(),
            city,
            decision,
        };
        if self.submit(command) {
            let text = tf!("classic.game.combat.decided", city = name, result = decision_done(decision));
            self.notify(NoticeKind::Combat, text);
        }
    }
}

fn decision_done(decision: CityDecision) -> &'static str {
    match decision {
        CityDecision::Annex => t!("classic.game.combat.done-annexed"),
        CityDecision::Puppet => t!("classic.game.combat.done-puppet"),
        CityDecision::Raze => t!("classic.game.combat.done-raze"),
        CityDecision::Liberate => t!("classic.game.combat.done-liberated"),
    }
}

pub(super) fn promotion_name(rules: &classic_rules::CompiledRuleset, id: PromotionId) -> String {
    let Some(def) = rules.promotions.get(id.0 as usize) else {
        return t!("classic.game.combat.promotion").into();
    };
    names::named("promotion", &def.name, &def.key)
}

/// The preview as the odds panel shows it (names are filled in by the caller).
fn odds(preview: &CombatPreview) -> CombatOdds {
    let (verdict, outlook) = verdict_text(preview.verdict);
    CombatOdds {
        attacker: String::new(),
        defender: String::new(),
        attacker_strength: strength_text(preview.attacker_strength),
        defender_strength: strength_text(preview.defender_strength),
        to_defender: preview.to_defender,
        to_attacker: preview.to_attacker,
        attacker_hp: preview.attacker_hp,
        defender_hp: preview.defender_hp,
        verdict,
        outlook,
        ranged: false,
    }
}

/// Strength in hundredths as "12.5".
fn strength_text(hundredths: i64) -> String {
    let tenths = (hundredths + 5) / 10;
    i18n::number(tenths as f64 / 10.0, 1, 1)
}

fn verdict_text(verdict: Verdict) -> (&'static str, Outlook) {
    match verdict {
        Verdict::DecisiveVictory => (t!("classic.game.combat.verdict-decisive-victory"), Outlook::Favorable),
        Verdict::MajorVictory => (t!("classic.game.combat.verdict-major-victory"), Outlook::Favorable),
        Verdict::MinorVictory => (t!("classic.game.combat.verdict-minor-victory"), Outlook::Favorable),
        Verdict::Stalemate => (t!("classic.game.combat.verdict-stalemate"), Outlook::Even),
        Verdict::MinorDefeat => (t!("classic.game.combat.verdict-minor-defeat"), Outlook::Unfavorable),
        Verdict::MajorDefeat => (t!("classic.game.combat.verdict-major-defeat"), Outlook::Unfavorable),
        Verdict::DecisiveDefeat => (t!("classic.game.combat.verdict-decisive-defeat"), Outlook::Unfavorable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strengths_read_in_tenths() {
        assert_eq!(strength_text(800), "8.0");
        assert_eq!(strength_text(1250), "12.5");
        assert_eq!(strength_text(1234), "12.3");
    }
}
