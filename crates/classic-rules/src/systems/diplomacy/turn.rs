//! The diplomacy step of a player's end turn (after Science): the ties with nations that are
//! gone, research agreements (accumulate, pay out at the end),
//! open borders that expire, offers to it that lapse, the decay of its
//! warmonger memory.

use crate::ids::PlayerId;
use crate::rules::CompiledRuleset;
use crate::systems::diplomacy::{
    warmonger_era_percent, MEDIAN_TECH_PERCENT, RESEARCH_AGREEMENT_POOL_DIVISOR, WARMONGER_ATTACKED_WEIGHT,
    WARMONGER_DECAY,
};
use crate::transaction::transaction::WorldTransaction;
use crate::world::relations::TradeFlow;
use crate::classic::ClassicChange;

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
        self.close_dead_ties();
        self.research_agreements(player, science.max(0) as u32, round, rules);
        self.expire_open_borders(player, round, rules);
        self.decay_warmonger(player, rules);
        let relations = &mut self.world.module.relations;
        relations
            .offers
            .retain(|o| !(o.to == player && round >= o.expires));
        relations.trades.retain(|t| round < t.until);
    }

    /// Every tie with a nation that is gone ends, both ways: its trades pay nothing (a dead
    /// payer's step never runs again, a dead receiver cannot be paid) and the living party must
    /// not count them. Runs at every turn end and at the settlement of the round.
    pub(crate) fn close_dead_ties(&mut self) {
        let players = &self.world.players;
        let gone = |p: PlayerId| !players.is_alive(p);
        let pairs: Vec<(PlayerId, PlayerId)> = self
            .world
            .module
            .relations
            .trades
            .iter()
            .filter(|t| gone(t.from) || gone(t.to))
            .map(|t| (t.from, t.to))
            .collect();
        for (a, b) in pairs {
            self.end_ties(a, b);
        }
    }

    /// The gold per turn of every running deal, settled once a round after every player's
    /// income is in the treasury (Civ V counts the deals in the net income of the round, so no
    /// order of the players decides who can pay). A payment is made in full from the payer's
    /// treasury or not at all; the payments run until none more can be made, so a payer whose
    /// instalment is covered by what it receives pays once that has arrived. What no treasury
    /// can carry breaks its deal (`break_deal`), as in the original game, where a deal the payer
    /// can no longer honour is cancelled.
    pub(crate) fn settle_gold_per_turn(&mut self, rules: &CompiledRuleset) {
        if self.world.module.relations.is_pristine() {
            return;
        }
        self.close_dead_ties();
        let round = self.world.turn.round;
        let mut due: Vec<(PlayerId, PlayerId, u32, u32)> = self
            .world
            .module
            .relations
            .trades
            .iter()
            .filter(|t| round < t.until)
            .filter_map(|t| match t.flow {
                TradeFlow::GoldPerTurn(gold) => Some((t.from, t.to, gold, t.until)),
                TradeFlow::Resource { .. } => None,
            })
            .collect();
        loop {
            let before = due.len();
            let mut i = 0;
            while i < due.len() {
                let (from, to, gold, _) = due[i];
                if self.world.players.spend(from, gold) {
                    self.world.players.earn(to, gold);
                    due.remove(i);
                } else {
                    i += 1;
                }
            }
            if due.len() == before {
                break;
            }
        }
        for (from, to, gold, until) in due {
            let running = self.world.module.relations.trades.iter().any(|t| {
                (t.from, t.to, t.until, t.flow) == (from, to, until, TradeFlow::GoldPerTurn(gold))
            });
            if running {
                self.break_deal(from, to, until, gold, rules);
            }
        }
    }

    /// `debtor` did not pay `gold` it owes `creditor` this turn: every trade of their deal ends
    /// (both ways: the trades of one accepted deal are the pair's that end at the same round,
    /// `deals::add_trade`), the creditor is told (`ClassicChange::DealBroken`) and remembers
    /// it. The memory is the one the rules keep of a player's breaches, the warmonger memory
    /// (`systems::diplomacy`; the AI reads it as trust, `ai::diplomacy::trusted`): a broken
    /// deal weighs what a declaration of war weighs (`WARMONGER_ATTACKED_WEIGHT`, scaled by the
    /// game's era), but only in the memory of the one it wronged.
    pub(crate) fn break_deal(&mut self, debtor: PlayerId, creditor: PlayerId, until: u32, gold: u32, rules: &CompiledRuleset) {
        let pair = |a: PlayerId, b: PlayerId| (a, b) == (debtor, creditor) || (a, b) == (creditor, debtor);
        self.world.module.relations.trades.retain(|t| !(pair(t.from, t.to) && t.until == until));
        self.journal.module_changes.push(ClassicChange::DealBroken { debtor, creditor, gold });
        if self.world.players.is_major(creditor) && self.world.players.is_alive(creditor) {
            let weight = WARMONGER_ATTACKED_WEIGHT * warmonger_era_percent(self.world, rules) / 100;
            self.change_warmonger(creditor, debtor, weight);
        }
        self.relations_changed(debtor);
        self.relations_changed(creditor);
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
