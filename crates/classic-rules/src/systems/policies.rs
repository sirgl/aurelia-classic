//! Culture pool and social policies (classic/docs/classic/rules.md): unlocking a
//! branch (its opener comes with it), adopting policies and tenets,
//! completing a branch (its finisher is free), choosing an ideology, free
//! policies and free buildings granted by effects, and the culture each
//! turn adds to the pool. The formulas live in `derived::culture` (one set
//! for validation, commit, HUD and AI); this module only mutates.
//!
//! Journal: ClassicChange::PolicyAdopted (city yields of the player may
//! change). Fires OnAdopt of every adopted policy.

use crate::command::CommandError;
use crate::derived::culture::{
    branch_complete, branch_status, ideology_status, policy_status, Payment,
};
use crate::ids::{BuildingKindId, CityHandle, PlayerId, PolicyId};
use crate::rules::content::{PolicyBranchId, PolicyRole};
use crate::rules::CompiledRuleset;
use crate::systems::effects::TriggerEvent;
use crate::classic::ClassicChange;
use crate::transaction::journal::CityChange;
use crate::transaction::transaction::WorldTransaction;
use crate::world::city::CityWrite;
use crate::world::culture::RECENT_TURNS;

/// How a policy enters the player's list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Acquired {
    /// Paid with culture (or the unlock of its branch was): raises the
    /// cost of the next policy.
    Paid,
    /// Free policy, free tenet or finisher: does not raise the cost.
    Free,
}

impl<'a> WorldTransaction<'a> {
    /// Unlock a branch: pay one policy, adopt its opener.
    pub(crate) fn unlock_policy_branch(
        &mut self,
        player: PlayerId,
        branch: PolicyBranchId,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let payment = branch_status(self.world, rules, player, branch)
            .map_err(|b| CommandError::RuleViolation(b.reason().into()))?;
        let acquired = self.pay(player, payment);
        if let Some(state) = self.world.module.culture.player_mut(player) {
            state.branches.push(branch);
        }
        match rules.module.policy_branches[branch.0 as usize].opener {
            Some(opener) => self.add_policy(player, opener, acquired, rules),
            None => self.journal_policies(player),
        }
        Ok(())
    }

    /// Adopt a policy or a tenet; complete its branch when it was the last.
    pub(crate) fn adopt_policy(
        &mut self,
        player: PlayerId,
        policy: PolicyId,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let payment = policy_status(self.world, rules, player, policy)
            .map_err(|b| CommandError::RuleViolation(b.reason().into()))?;
        let acquired = self.pay(player, payment);
        self.add_policy(player, policy, acquired, rules);
        Ok(())
    }

    /// Choose an ideology: its branch opens for free, first and second
    /// adopters get free tenets (rules.md).
    pub(crate) fn choose_ideology(
        &mut self,
        player: PlayerId,
        ideology: PolicyBranchId,
        rules: &CompiledRuleset,
    ) -> Result<(), CommandError> {
        let free_tenets = ideology_status(self.world, rules, player, ideology)
            .map_err(|b| CommandError::RuleViolation(b.reason().into()))?;
        self.world.module.culture.count_adopter(ideology);
        if let Some(state) = self.world.module.culture.player_mut(player) {
            state.ideology = Some(ideology);
            state.branches.push(ideology);
            state.free_tenets = state.free_tenets.saturating_add(u16::from(free_tenets));
        }
        self.journal_policies(player);
        Ok(())
    }

    /// Culture of the turn into the pool (rules.md step 8): the sum of
    /// the cities' culture; the high-water mark of cities is updated.
    pub(crate) fn collect_culture(&mut self, player: PlayerId, cities_total: u32) {
        let cities = self.world.indexes.cities_of(player).len() as u16;
        let Some(state) = self.world.module.culture.player_mut(player) else {
            return;
        };
        state.culture = state.culture.saturating_add(cities_total);
        state.lifetime = state.lifetime.saturating_add(u64::from(cities_total));
        state.recent.push(cities_total);
        if state.recent.len() > RECENT_TURNS {
            state.recent.remove(0);
        }
        state.max_cities = state.max_cities.max(cities);
    }

    /// FreePolicies: the player may adopt `count` policies without culture.
    pub(crate) fn grant_free_policies(&mut self, player: PlayerId, count: u8) {
        if let Some(state) = self.world.module.culture.player_mut(player) {
            state.free_policies = state.free_policies.saturating_add(u16::from(count));
        }
    }

    /// FreeBuildingInCities: the building in the first `count` cities that
    /// lack it, in founding order; the rest is owed to the next cities.
    pub(crate) fn grant_free_buildings_in_cities(
        &mut self,
        player: PlayerId,
        building: BuildingKindId,
        count: u8,
    ) {
        let mut left = count;
        let cities = self.world.indexes.cities_of(player).to_vec();
        for city in cities {
            if left == 0 {
                break;
            }
            if self.give_free_building(city, building) {
                left -= 1;
            }
        }
        if left == 0 {
            return;
        }
        if let Some(state) = self.world.module.culture.player_mut(player) {
            state.free_buildings.push((building, left));
        }
    }

    /// A newly founded city receives the buildings still owed to the
    /// player's next cities.
    pub(crate) fn settle_free_buildings(&mut self, player: PlayerId, city: CityHandle) {
        let owed = match self.world.module.culture.player(player) {
            Some(state) if !state.free_buildings.is_empty() => state.free_buildings.clone(),
            _ => return,
        };
        let mut still_owed = Vec::with_capacity(owed.len());
        for (building, left) in owed {
            let given = self.give_free_building(city, building);
            let left = if given { left - 1 } else { left };
            if left > 0 {
                still_owed.push((building, left));
            }
        }
        if let Some(state) = self.world.module.culture.player_mut(player) {
            state.free_buildings = still_owed;
        }
    }

    /// The building appears in the city unless it is there; true if given.
    fn give_free_building(&mut self, city: CityHandle, building: BuildingKindId) -> bool {
        let present = match self.world.cities.get(city) {
            Some(core) => core.buildings.contains(&building),
            None => return false,
        };
        if present {
            return false;
        }
        if let CityWrite::Applied = self.world.cities.add_building(city, building) {
            self.journal
                .city_changes
                .push(CityChange::BuildingAdded { city, building });
            return true;
        }
        false
    }

    /// Take the payment validated by the status function.
    fn pay(&mut self, player: PlayerId, payment: Payment) -> Acquired {
        let Some(state) = self.world.module.culture.player_mut(player) else {
            return Acquired::Free;
        };
        match payment {
            Payment::Culture(cost) => {
                state.culture -= cost;
                Acquired::Paid
            }
            Payment::FreePolicy => {
                state.free_policies -= 1;
                Acquired::Free
            }
            Payment::FreeTenet => {
                state.free_tenets -= 1;
                Acquired::Free
            }
        }
    }

    /// Harness-only: the policy adopted for free, without its prerequisites
    /// (its OnAdopt effects fire as on adoption).
    #[cfg(any(test, feature = "harness"))]
    pub(crate) fn debug_adopt_policy(&mut self, player: PlayerId, policy: PolicyId, rules: &CompiledRuleset) {
        self.add_policy(player, policy, Acquired::Free, rules);
    }

    /// Record the policy, fire its OnAdopt effects, grant the finisher of a
    /// completed branch.
    fn add_policy(
        &mut self,
        player: PlayerId,
        policy: PolicyId,
        acquired: Acquired,
        rules: &CompiledRuleset,
    ) {
        let Some(state) = self.world.module.culture.player_mut(player) else {
            return;
        };
        if state.has_policy(policy) {
            return;
        }
        state.policies.push(policy);
        if acquired == Acquired::Free {
            state.free_used = state.free_used.saturating_add(1);
        }
        self.journal_policies(player);
        self.fire(rules, TriggerEvent::PolicyAdopted { player, policy });
        self.espionage_after_policy(player, policy, rules);
        self.grant_finisher(player, policy, rules);
    }

    /// The last policy of a normal branch completes it: its finisher is
    /// adopted for free (rules.md).
    fn grant_finisher(&mut self, player: PlayerId, policy: PolicyId, rules: &CompiledRuleset) {
        let def = &rules.module.policies[policy.0 as usize];
        if def.role != PolicyRole::Policy {
            return;
        }
        let Some(branch) = def.branch else {
            return;
        };
        let Some(finisher) = rules.module.policy_branches[branch.0 as usize].finisher else {
            return;
        };
        let complete = branch_complete(rules, self.world.module.culture.of(player), branch);
        if complete {
            self.add_policy(player, finisher, Acquired::Free, rules);
        }
    }

    fn journal_policies(&mut self, player: PlayerId) {
        self.journal
            .module_changes
            .push(ClassicChange::PolicyAdopted { player });
    }
}
