//! What the classic module adds to a game state (`GameState`, the kernel's
//! state instantiated with `Classic`): the AI's observations, the victory
//! outcome, and the harness setters scenarios and tests use to build a
//! world without playing to it. Every mutation goes through the kernel's
//! `commit_journal`, so derived state and knowledge follow it.

use crate::ids::{PlayerId, UnitHandle};
#[cfg(any(test, feature = "harness"))]
use crate::ids::{BuildingKindId, CityHandle, TileId};
use crate::knowledge::observation::{AIObservation, LocalObservation};
use crate::rules::CompiledRuleset;
#[cfg(any(test, feature = "harness"))]
use crate::transaction::journal::ChangeJournal;
#[cfg(any(test, feature = "harness"))]
use crate::transaction::transaction::WorldTransaction;
#[cfg(any(test, feature = "harness"))]
use crate::classic::ClassicChange;
use crate::world::state::GameState;
#[cfg(any(test, feature = "harness"))]
use crate::world::player::ClassicPlayers;

/// The classic side of a game state.
pub trait ClassicGame {

    fn ai_observation<'a>(&'a self, rules: &'a CompiledRuleset, player: PlayerId) -> AIObservation<'a>;

    fn ai_local_observation<'a>(
        &'a self,
        rules: &'a CompiledRuleset,
        player: PlayerId,
        unit: UnitHandle,
        radius: i32,
    ) -> Option<(LocalObservation<'a>, crate::metrics::WorkCounters)>;

    /// Would the rules accept `command` of `player` now (the interface's own
    /// check, `PlayerQueries::accepts`)? The AI driver's last look before it
    /// sends an order.
    fn accepts_order(&self, rules: &CompiledRuleset, player: PlayerId, command: &crate::command::Command) -> bool;

    fn ai_observation_counted<'a>(
        &'a self,
        rules: &'a CompiledRuleset,
        player: PlayerId,
    ) -> (AIObservation<'a>, crate::metrics::WorkCounters);


    #[cfg(any(test, feature = "harness"))]
    fn debug_force_war(&mut self, rules: &CompiledRuleset, player: PlayerId, enemy: PlayerId);

    #[cfg(any(test, feature = "harness"))]
    fn debug_adopt_policy(&mut self, rules: &CompiledRuleset, player: PlayerId, policy: crate::ids::PolicyId);

    #[cfg(any(test, feature = "harness"))]
    fn debug_complete_building(&mut self, rules: &CompiledRuleset, city: CityHandle, building: BuildingKindId);

    /// Harness-only: a golden age of `turns` for `player`.
    #[cfg(any(test, feature = "harness"))]
    fn debug_start_golden_age(&mut self, rules: &CompiledRuleset, player: PlayerId, turns: u16);

    #[cfg(any(test, feature = "harness"))]
    fn debug_discover(&mut self, rules: &CompiledRuleset, player: PlayerId, tech: crate::ids::TechId);

    #[cfg(any(test, feature = "harness"))]
    fn debug_start_league_project(&mut self, rules: &CompiledRuleset, project: crate::rules::content::ProjectId);

    #[cfg(any(test, feature = "harness"))]
    fn debug_contribute_to_league_project(
        &mut self,
        rules: &CompiledRuleset,
        player: PlayerId,
        project: crate::rules::content::ProjectId,
        production: u32,
    );

    #[cfg(any(test, feature = "harness"))]
    fn debug_set_ideology(&mut self, rules: &CompiledRuleset, p: PlayerId, ideology: crate::rules::content::PolicyBranchId);

    #[cfg(any(test, feature = "harness"))]
    fn debug_place_camp(&mut self, rules: &CompiledRuleset, tile: TileId);

    #[cfg(any(test, feature = "harness"))]
    fn debug_set_influence(&mut self, rules: &CompiledRuleset, city_state: PlayerId, major: PlayerId, influence: i32);

    #[cfg(any(test, feature = "harness"))]
    fn debug_set_victory_options(&mut self, options: crate::world::victory::VictoryOptions);

    #[cfg(any(test, feature = "harness"))]
    fn debug_add_project(&mut self, player: PlayerId, project: crate::rules::content::ProjectId);

    fn continue_after_victory(&mut self);

    #[cfg(any(test, feature = "harness"))]
    fn debug_add_quest(&mut self, city_state: PlayerId, major: PlayerId, kind: crate::world::city_states::QuestKind, end: Option<u32>);

    #[cfg(any(test, feature = "harness"))]
    fn debug_set_personality(&mut self, city_state: PlayerId, personality: crate::world::city_states::Personality);

    #[cfg(any(test, feature = "harness"))]
    fn debug_set_barbarian_mode(&mut self, mode: crate::world::barbarians::BarbarianMode);

    #[cfg(any(test, feature = "harness"))]
    fn debug_grant_faith(&mut self, p: PlayerId, amount: u32);

    #[cfg(any(test, feature = "harness"))]
    fn debug_grant_culture(&mut self, p: PlayerId, amount: u32);

    /// A landmark of the dig record of era `era` stands on `tile`.
    #[cfg(any(test, feature = "harness"))]
    fn debug_set_landmark(&mut self, tile: TileId, era: u8);

    /// Put a promotion on a unit as a free promotion would (no level, no
    /// experience spent).
    #[cfg(any(test, feature = "harness"))]
    fn debug_grant_promotion(&mut self, rules: &CompiledRuleset, unit: UnitHandle, promotion: crate::ids::PromotionId);

    /// Count one Great General as born for `p` (what a Carthaginian
    /// crossing of mountains waits for), as the kill that earns it would.
    #[cfg(any(test, feature = "harness"))]
    fn debug_earn_great_general(&mut self, p: PlayerId, rules: &CompiledRuleset);



    fn outcome(&self) -> Option<crate::world::victory::Outcome>;
}

impl ClassicGame for GameState {

    fn ai_observation<'a>(&'a self, rules: &'a CompiledRuleset, player: PlayerId) -> AIObservation<'a> {
        let _span = prof::span("classic/ai_observation (full)");
        AIObservation::for_player(self.view().auth, self.derived(), rules, player).0
    }


    fn ai_local_observation<'a>(
        &'a self,
        rules: &'a CompiledRuleset,
        player: PlayerId,
        unit: UnitHandle,
        radius: i32,
    ) -> Option<(LocalObservation<'a>, crate::metrics::WorkCounters)> {
        let _span = prof::span("classic/ai_local_observation");
        LocalObservation::for_unit(self.view().auth, self.derived(), rules, player, unit, radius)
    }


    fn ai_observation_counted<'a>(
        &'a self,
        rules: &'a CompiledRuleset,
        player: PlayerId,
    ) -> (AIObservation<'a>, crate::metrics::WorkCounters) {
        AIObservation::for_player(self.view().auth, self.derived(), rules, player)
    }

    fn accepts_order(&self, rules: &CompiledRuleset, player: PlayerId, command: &crate::command::Command) -> bool {
        use crate::knowledge::public::PlayerQueries;
        crate::knowledge::view::PlayerView::new(self.view().auth, self.derived(), rules, player).accepts(command)
    }




    #[cfg(any(test, feature = "harness"))]
    fn debug_force_war(&mut self, rules: &CompiledRuleset, player: PlayerId, enemy: PlayerId) {
        let mut tx = WorldTransaction::new(self.world_mut());
        let _ = tx.declare_war(player, enemy, rules);
        let journal = tx.commit().expect("debug_force_war commit");
        self.commit_journal(rules, &journal);
    }


    #[cfg(any(test, feature = "harness"))]
    fn debug_adopt_policy(&mut self, rules: &CompiledRuleset, player: PlayerId, policy: crate::ids::PolicyId) {
        let mut tx = WorldTransaction::new(self.world_mut());
        tx.debug_adopt_policy(player, policy, rules);
        let journal = tx.commit().expect("debug_adopt_policy commit");
        self.commit_journal(rules, &journal);
    }


    #[cfg(any(test, feature = "harness"))]
    fn debug_start_golden_age(&mut self, rules: &CompiledRuleset, player: PlayerId, turns: u16) {
        let mut tx = WorldTransaction::new(self.world_mut());
        tx.start_golden_age(player, turns);
        let journal = tx.commit().expect("debug_start_golden_age commit");
        self.commit_journal(rules, &journal);
    }

    #[cfg(any(test, feature = "harness"))]
    fn debug_complete_building(&mut self, rules: &CompiledRuleset, city: CityHandle, building: BuildingKindId) {
        let mut tx = WorldTransaction::new(self.world_mut());
        tx.debug_complete_building(city, building, rules);
        let journal = tx.commit().expect("debug_complete_building commit");
        self.commit_journal(rules, &journal);
    }


    #[cfg(any(test, feature = "harness"))]
    fn debug_discover(&mut self, rules: &CompiledRuleset, player: PlayerId, tech: crate::ids::TechId) {
        let mut tx = WorldTransaction::new(self.world_mut());
        tx.learn(player, tech, rules);
        let journal = tx.commit().expect("debug_discover commit");
        self.commit_journal(rules, &journal);
    }


    #[cfg(any(test, feature = "harness"))]
    fn debug_start_league_project(&mut self, rules: &CompiledRuleset, project: crate::rules::content::ProjectId) {
        if crate::systems::league_projects::project_problem(self.view().auth, rules, project).is_some() {
            return;
        }
        let mut tx = WorldTransaction::new(self.world_mut());
        tx.start_project(project);
        let journal = tx.commit().expect("debug_start_league_project commit");
        self.commit_journal(rules, &journal);
    }


    #[cfg(any(test, feature = "harness"))]
    fn debug_contribute_to_league_project(
        &mut self,
        rules: &CompiledRuleset,
        player: PlayerId,
        project: crate::rules::content::ProjectId,
        production: u32,
    ) {
        let mut tx = WorldTransaction::new(self.world_mut());
        tx.contribute_to_project(player, project, production);
        let journal = tx.commit().expect("debug_contribute_to_league_project commit");
        self.commit_journal(rules, &journal);
    }


    #[cfg(any(test, feature = "harness"))]
    fn debug_set_ideology(&mut self, rules: &CompiledRuleset, p: PlayerId, ideology: crate::rules::content::PolicyBranchId) {
        if let Some(state) = self.world_mut().module.culture.player_mut(p) {
            state.ideology = Some(ideology);
            state.branches.push(ideology);
        }
        let mut journal = ChangeJournal::default();
        journal
            .module_changes
            .push(ClassicChange::PolicyAdopted { player: p });
        self.commit_journal(rules, &journal);
    }


    #[cfg(any(test, feature = "harness"))]
    fn debug_place_camp(&mut self, rules: &CompiledRuleset, tile: TileId) {
        let mut tx = WorldTransaction::new(self.world_mut());
        tx.place_empty_camp(tile, rules);
        let journal = tx.commit().expect("debug_place_camp commit");
        self.commit_journal(rules, &journal);
    }


    #[cfg(any(test, feature = "harness"))]
    fn debug_set_influence(&mut self, rules: &CompiledRuleset, city_state: PlayerId, major: PlayerId, influence: i32) {
        let mut tx = WorldTransaction::new(self.world_mut());
        tx.set_influence(city_state, major, influence * 100, rules);
        let journal = tx.commit().expect("debug_set_influence commit");
        self.commit_journal(rules, &journal);
    }


    #[cfg(any(test, feature = "harness"))]
    fn debug_set_victory_options(&mut self, options: crate::world::victory::VictoryOptions) {
        self.world_mut().module.victory.options = options;
    }


    #[cfg(any(test, feature = "harness"))]
    fn debug_add_project(&mut self, player: PlayerId, project: crate::rules::content::ProjectId) {
        self.world_mut().module.victory.add_project(player, project);
    }


    fn continue_after_victory(&mut self) {
        if self.module_mut().victory.outcome.is_some() {
            self.module_mut().victory.continued = true;
        }
    }


    #[cfg(any(test, feature = "harness"))]
    fn debug_add_quest(&mut self, city_state: PlayerId, major: PlayerId, kind: crate::world::city_states::QuestKind, end: Option<u32>) {
        let start = self.world_mut().turn.round;
        self.world_mut().module.city_states.quests.push(crate::world::city_states::Quest {
            city_state,
            major,
            kind,
            start,
            end,
        });
    }


    #[cfg(any(test, feature = "harness"))]
    fn debug_set_personality(&mut self, city_state: PlayerId, personality: crate::world::city_states::Personality) {
        if let Some(minor) = self.world_mut().module.city_states.minor_mut(city_state) {
            minor.personality = personality;
        }
    }


    #[cfg(any(test, feature = "harness"))]
    fn debug_set_barbarian_mode(&mut self, mode: crate::world::barbarians::BarbarianMode) {
        self.world_mut().module.barbarians.mode = mode;
        if let Some(barbarian) = self.world_mut().players.barbarian() {
            self.world_mut().players.set_alive(barbarian, mode != crate::world::barbarians::BarbarianMode::None);
        }
    }


    #[cfg(any(test, feature = "harness"))]
    fn debug_grant_faith(&mut self, p: PlayerId, amount: u32) {
        if let Some(state) = self.world_mut().module.religion.player_mut(p) {
            state.faith = state.faith.saturating_add(amount);
        }
    }


    #[cfg(any(test, feature = "harness"))]
    fn debug_set_landmark(&mut self, tile: TileId, era: u8) {
        self.world_mut().module.tourism.archaeology_mut().landmarks.push((tile, era));
    }

    #[cfg(any(test, feature = "harness"))]
    fn debug_grant_promotion(&mut self, rules: &CompiledRuleset, unit: UnitHandle, promotion: crate::ids::PromotionId) {
        if let Some(core) = self.world_mut().units.get_mut_core(unit) {
            core.promotions.insert(promotion);
        }
        let mut journal = ChangeJournal::default();
        journal.unit_changes.push(crate::transaction::journal::UnitChange::Promoted { unit, promotion });
        self.commit_journal(rules, &journal);
    }

    #[cfg(any(test, feature = "harness"))]
    fn debug_earn_great_general(&mut self, p: PlayerId, rules: &CompiledRuleset) {
        if let Some(m) = self.world_mut().module.military.get_mut(p) {
            m.generals_born += 1;
        }
        let mut journal = ChangeJournal::default();
        journal.module_changes.push(ClassicChange::GeneralBorn { player: p });
        self.commit_journal(rules, &journal);
    }

    #[cfg(any(test, feature = "harness"))]
    fn debug_grant_culture(&mut self, p: PlayerId, amount: u32) {
        if let Some(state) = self.world_mut().module.culture.player_mut(p) {
            state.culture = state.culture.saturating_add(amount);
            state.lifetime = state.lifetime.saturating_add(u64::from(amount));
        }
    }






    fn outcome(&self) -> Option<crate::world::victory::Outcome> {
        self.view().auth.module.victory.outcome()
    }
}
