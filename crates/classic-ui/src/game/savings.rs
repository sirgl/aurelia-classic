//! What culture and faith are saved up for, for the top bar: the next policy, the pantheon,
//! the Great Prophet. Prices and "can be taken now" come from the core helpers the policy and
//! religion screens use (`derived::culture`, `derived::religion`); nothing is recomputed here.
use classic_rules::derived::culture::{
    branch_statuses, next_policy_cost, policy_statuses, BranchBlock, PolicyBlock,
};
use classic_rules::derived::religion::{
    free_beliefs, pantheon_cost, pantheon_status, prophet_cost, reformation_status, religions_left,
    FaithBlock,
};
use classic_rules::rules::content::BeliefCategory;

use super::session::Session;
use crate::ui::model::{Goal, Saving};

impl Session {
    /// The next policy: ready when a policy, tenet or branch can be taken now (the end-turn gate's
    /// own test, `policy_pending`), else the price while some card waits only for culture.
    pub fn culture_saving(&self) -> Option<Saving> {
        if !self.has_city() {
            return None;
        }
        let (auth, rules, me) = (self.game().view().auth, self.rules(), self.me());
        let policies = policy_statuses(auth, rules, me);
        let branches = branch_statuses(auth, rules, me);
        let ready = policies.iter().any(Result::is_ok) || branches.iter().any(Result::is_ok);
        let waits = policies.iter().any(|s| matches!(s, Err(PolicyBlock::NotEnoughCulture)))
            || branches.iter().any(|s| matches!(s, Err(BranchBlock::NotEnoughCulture)));
        if !ready && !waits {
            return None;
        }
        Some(Saving {
            goal: Goal::Policy,
            have: auth.module().culture().of(me).culture,
            cost: next_policy_cost(auth, rules, me),
            ready,
        })
    }

    /// The next faith step: the pantheon while there is none; then the Great Prophet that comes by
    /// itself when the pool reaches its price (to found the religion, later to enhance it); a
    /// Reformation belief when one can be added. None when nothing is left to save for.
    pub fn faith_saving(&self) -> Option<Saving> {
        if !self.has_city() {
            return None;
        }
        let (auth, rules, me) = (self.game().view().auth, self.rules(), self.me());
        let state = auth.module().religion().player(me);
        let have = state.faith;
        if state.pantheon.is_none() && state.religion.is_none() {
            let belief = *free_beliefs(auth, rules, BeliefCategory::Pantheon).first()?;
            let (ready, waits) = match pantheon_status(auth, rules, me, belief) {
                Ok(_) => (true, true),
                Err(FaithBlock::NotEnoughFaith) => (false, true),
                Err(_) => (false, false),
            };
            return waits.then(|| Saving { goal: Goal::Pantheon, have, cost: pantheon_cost(auth, rules), ready });
        }
        if let Some(belief) = free_beliefs(auth, rules, BeliefCategory::Reformation).first() {
            if reformation_status(auth, rules, me, *belief).is_ok() {
                return Some(Saving { goal: Goal::Reformation, have, cost: 0, ready: true });
            }
        }
        let enhanced = state
            .religion
            .and_then(|r| auth.module().religion().religion(r))
            .is_some_and(|r| r.enhanced);
        let prophet_due = match state.religion {
            None => religions_left(auth) > 0,
            Some(_) => !enhanced,
        };
        prophet_due.then(|| Saving { goal: Goal::Prophet, have, cost: prophet_cost(auth, rules, me), ready: false })
    }
}
