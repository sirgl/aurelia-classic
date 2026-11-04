//! The diplomacy step of a player's end turn (after Science): gold per
//! turn it owes, research agreements (accumulate, pay out at the end),
//! open borders that expire, offers to it that lapse, the decay of its
//! warmonger memory.

use crate::ids::PlayerId;
use crate::rules::CompiledRuleset;
use crate::systems::diplomacy::{
    warmonger_era_percent, MEDIAN_TECH_PERCENT, RESEARCH_AGREEMENT_POOL_DIVISOR, WARMONGER_DECAY,
};
use crate::transaction::transaction::WorldTransaction;
use crate::world::relations::TradeFlow;

impl<'a> WorldTransaction<'a> {
    /// `science`: the player's science of this turn (research agreements
    /// accumulate it).
    pub(crate) fn diplomacy_turn(
        &mut self,
        player: PlayerId,
        science: i32,
        rules: &CompiledRuleset,
    ) {
        if self.world.module.relations.is_pristine() {
            return;
        }
        let round = self.world.turn.round;
        self.pay_gold_per_turn(player, round);
        self.research_agreements(player, science.max(0) as u32, round, rules);
        self.expire_open_borders(player, round, rules);
        self.decay_warmonger(player, rules);
        let relations = &mut self.world.module.relations;
        relations
            .offers
            .retain(|o| !(o.to == player && round >= o.expires));
        relations.trades.retain(|t| round < t.until);
    }

    fn pay_gold_per_turn(&mut self, player: PlayerId, round: u32) {
        let owed: Vec<(PlayerId, u32)> = self
            .world
            .module
            .relations
            .trades
            .iter()
            .filter(|t| t.from == player && round < t.until)
            .filter_map(|t| match t.flow {
                TradeFlow::GoldPerTurn(gold) => Some((t.to, gold)),
                TradeFlow::Resource { .. } => None,
            })
            .collect();
        for (to, gold) in owed {
            let missing = self.world.players.drain(player, gold);
            self.world.players.earn(to, gold - missing);
        }
    }

    /// Accumulate this turn's science for every running agreement; at the
    /// end pay min(both sides) / 3 × (50 + ResearchAgreementPercent) % as
    /// research (§5.8: Porcelain Tower, Scientific Revolution).
    fn research_agreements(&mut self, player: PlayerId, science: u32, round: u32, rules: &CompiledRuleset) {
        let players = self.world.module.relations.players;
        for q in 0..players {
            let other = PlayerId(q as u8);
            let relations = &mut self.world.module.relations;
            let Some(i) = relations.slot(player, other) else {
                continue;
            };
            let until = relations.research_until[i];
            if until == 0 {
                continue;
            }
            if round < until {
                relations.research_points[i] = relations.research_points[i].saturating_add(science);
                continue;
            }
            let theirs = relations.research_points(other, player);
            let pooled = relations.research_points[i].min(theirs) / RESEARCH_AGREEMENT_POOL_DIVISOR;
            relations.research_until[i] = 0;
            let bonus = pooled * research_agreement_percent(self.world, rules, player) / 100;
            self.world.players.earn_science(player, bonus);
            self.relations_changed(player);
        }
    }

    fn expire_open_borders(&mut self, player: PlayerId, round: u32, rules: &CompiledRuleset) {
        let players = self.world.module.relations.players;
        for q in 0..players {
            let guest = PlayerId(q as u8);
            let relations = &mut self.world.module.relations;
            let Some(i) = relations.slot(player, guest) else {
                continue;
            };
            let until = relations.open_until[i];
            if until == 0 || round < until {
                continue;
            }
            relations.open_until[i] = 0;
            self.world.diplomacy.set_open(player, guest, false);
            self.expel_units(player, guest, rules);
            self.relations_changed(player);
        }
    }

    /// −5 a turn scaled by the game's era, never below 0.
    fn decay_warmonger(&mut self, observer: PlayerId, rules: &CompiledRuleset) {
        let decay = WARMONGER_DECAY * warmonger_era_percent(self.world, rules) / 100;
        let players = self.world.module.relations.players;
        for q in 0..players {
            let relations = &mut self.world.module.relations;
            if let Some(i) = relations.slot(observer, PlayerId(q as u8)) {
                relations.warmonger[i] = (relations.warmonger[i] + decay).max(0);
            }
        }
    }
}

/// Percent of the pooled science a research agreement pays: the median tech
/// percent (50) plus the player's `ResearchAgreementPercent`.
fn research_agreement_percent(
    auth: &crate::world::state::AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
) -> u32 {
    let stats = &crate::effects::player::player_modifiers(auth, rules, player).stats;
    let extra = stats.get(crate::rules::stats::Stat::Module(crate::rules::stats::ClassicStat::ResearchAgreementPercent));
    (MEDIAN_TECH_PERCENT as i32 + extra).max(0) as u32
}
