//! World Congress projects (classic/docs/classic/rules.md): the World's Fair, the
//! International Games and the International Space Station.
//!
//! ```text
//! start         the InternationalProject resolution passes (choice: the project)
//! contribution  a city with an empty queue and the project's process adds its
//!               production ×100 in its Production step
//! cost          cost_per_player × 100 × nations of the game
//! completion    checked in the league's turn (once a round): Σ ≥ cost
//! tiers         1: ≥ 50 % of cost_per_player; 2: ≥ 100 %; 3: the best
//!               contribution of a living member (and ≥ 100 %), only the
//!               first such member in player order — the others get tier 2
//! rewards       a tier's rewards and those of every tier below
//! ```
//!
//! Rewards for good or for a while live in `CongressState::rewards`
//! (happiness, culture and tourism boost turns; read by `derived::happiness`,
//! the Culture step and `systems::tourism`); the rest change the world once.
//! Journal: the free policies, units and buildings journal themselves; the
//! happiness reward is read on demand (no cache).

use crate::ids::{CityHandle, PlayerId};
use crate::classic::tables::{LeagueProject, LeagueReward, LEAGUE_TIER_PERCENTS};
use crate::rules::content::ProjectId;
use crate::rules::CompiledRuleset;
use crate::transaction::transaction::WorldTransaction;
use crate::world::city_economy::Process;
use crate::world::congress::{LeagueRewards, ProjectProgress};
use crate::world::state::AuthWorld;
use crate::world::player::ClassicPlayers;

/// Percent boost to culture and to tourism while a league project reward
/// runs. Own design (no Unciv equivalent: Unciv has no league projects).
pub const BOOST_PERCENT: u32 = 75;

/// The league data of a project (None: not a World Congress project).
pub fn league_of(rules: &CompiledRuleset, project: ProjectId) -> Option<&LeagueProject> {
    rules.module.projects.get(project.0 as usize)?.league.as_ref()
}

/// Production ×100 of one nation's share, × the game speed's production
/// percent (a production cost, scaled as every other one).
pub fn share_times100(auth: &AuthWorld, league: &LeagueProject) -> u64 {
    let share = kernel::rules::speeds::SpeedValues::scale(league.cost_per_player, auth.settings().speed_values().production_percent);
    u64::from(share) * 100
}

/// Production ×100 the whole project costs: a share per nation of
/// the game (`countMajorCivsEverAlive`: every nation starts alive).
pub fn cost_times100(auth: &AuthWorld, league: &LeagueProject) -> u64 {
    let nations = (0..auth.players().count())
        .filter(|p| auth.players().is_major(PlayerId(*p as u8)))
        .count() as u64;
    share_times100(auth, league) * nations
}

/// Why `project` may not be proposed now (None: it may): the congress is
/// founded, a World Congress project, not under way, not completed, its
/// technology known by a member.
pub fn project_problem(auth: &AuthWorld, rules: &CompiledRuleset, project: ProjectId) -> Option<&'static str> {
    let Some(league) = league_of(rules, project) else {
        return Some("not a World Congress project");
    };
    let congress = auth.module().congress();
    if !congress.founded() {
        return Some("no World Congress");
    }
    if congress.project(project).is_some() || congress.completed().contains(&project) {
        return Some("project under way or completed");
    }
    match league.tech_any_member {
        None => None,
        Some(tech) => {
            let known = crate::systems::city_states::majors(auth)
                .into_iter()
                .any(|p| auth.players().techs_of(p).contains(&tech));
            (!known).then_some("no member knows the technology")
        }
    }
}

/// The reward tier of every member at completion (index = player id; 0:
/// none), §6.8.
pub fn tiers(auth: &AuthWorld, league: &LeagueProject, progress: &ProjectProgress) -> Vec<u8> {
    let share = share_times100(auth, league);
    let members = crate::systems::city_states::majors(auth);
    let best = members.iter().map(|p| progress.of(*p)).max().unwrap_or(0);
    let mut tiers = vec![0u8; auth.players().count()];
    let mut top_given = false;
    for member in members {
        let amount = progress.of(member);
        let tier = if amount >= best.max(share) && !top_given {
            top_given = true;
            3
        } else if amount >= share * u64::from(LEAGUE_TIER_PERCENTS[1]) / 100 {
            2
        } else if amount >= share * u64::from(LEAGUE_TIER_PERCENTS[0]) / 100 {
            1
        } else {
            0
        };
        tiers[member.0 as usize] = tier;
    }
    tiers
}

/// Culture or tourism percent of a running boost (0 without one).
pub fn boost_percent(turns_left: u16) -> u32 {
    if turns_left > 0 {
        BOOST_PERCENT
    } else {
        0
    }
}

impl<'a> WorldTransaction<'a> {
    /// The InternationalProject resolution passed: the project starts.
    pub(crate) fn start_project(&mut self, project: ProjectId) {
        let players = self.world.players.count();
        self.world.module.congress.projects.push(ProjectProgress {
            project,
            contributions: vec![0; players],
        });
    }

    /// A city's production goes to the project (its process).
    pub(crate) fn contribute_to_project(&mut self, player: PlayerId, project: ProjectId, production: u32) {
        let Some(progress) = self.world.module.congress.projects.iter_mut().find(|p| p.project == project) else {
            return;
        };
        if let Some(slot) = progress.contributions.get_mut(player.0 as usize) {
            *slot = slot.saturating_add(u64::from(production) * 100);
        }
    }

    /// The league's turn: every project whose contributions reach its cost
    /// is completed and rewarded.
    pub(crate) fn check_projects(&mut self, rules: &CompiledRuleset) {
        let mut index = 0;
        while index < self.world.module.congress.projects.len() {
            let progress = &self.world.module.congress.projects[index];
            let Some(league) = league_of(rules, progress.project) else {
                index += 1;
                continue;
            };
            if progress.total() < cost_times100(self.world, league) {
                index += 1;
                continue;
            }
            let progress = self.world.module.congress.projects.remove(index);
            self.complete_league_project(progress, rules);
        }
    }

    fn complete_league_project(&mut self, progress: ProjectProgress, rules: &CompiledRuleset) {
        let Some(league) = league_of(rules, progress.project) else {
            return;
        };
        let tiers = tiers(self.world, league, &progress);
        self.world.module.congress.completed.push(progress.project);
        self.stop_project_processes(progress.project);
        for (player, tier) in tiers.iter().enumerate() {
            let player = PlayerId(player as u8);
            for rewards in league.tiers.iter().take(usize::from(*tier)) {
                for reward in rewards {
                    self.give_league_reward(player, *reward, rules);
                }
            }
        }
    }

    /// Cities working on a completed project stop (their queue decides
    /// again).
    fn stop_project_processes(&mut self, project: ProjectId) {
        let cities: Vec<CityHandle> = self.world.cities.iter().map(|(h, _)| h).collect();
        for city in cities {
            let Some(core) = self.world.cities.get_mut(city) else {
                continue;
            };
            if core.economy.process == Some(Process::Contribute(project)) {
                core.economy.process = None;
            }
        }
    }

    fn give_league_reward(&mut self, player: PlayerId, reward: LeagueReward, rules: &CompiledRuleset) {
        match reward {
            LeagueReward::Happiness(n) => self.change_league_rewards(player, |r| r.happiness = r.happiness.saturating_add(n)),
            LeagueReward::CultureBoostTurns(n) => {
                self.change_league_rewards(player, |r| r.culture_boost_turns = r.culture_boost_turns.saturating_add(n))
            }
            LeagueReward::TourismBoostTurns(n) => {
                self.change_league_rewards(player, |r| r.tourism_boost_turns = r.tourism_boost_turns.saturating_add(n))
            }
            LeagueReward::FreePolicies(n) => self.grant_free_policies(player, n),
            LeagueReward::GoldenAgePoints(n) => {
                if let Some(economy) = self.world.players.economy_mut(player) {
                    economy.golden_age_meter = economy.golden_age_meter.saturating_add(n);
                }
            }
            LeagueReward::CityStateInfluence(n) => self.boost_city_state_influence(player, n, rules),
            LeagueReward::ResearchTurns(turns) => {
                let science = crate::derived::great_people::bulb_science(self.world, player, turns);
                self.world.players.earn_science(player, science);
            }
            LeagueReward::FreeUnit(kind) => {
                if let Some(capital) = crate::world::city::capital_of(self.world, rules, player) {
                    if let Some(tile) = self.world.cities.get(capital).map(|c| c.tile) {
                        self.spawn_at_city(player, tile, kind, rules);
                    }
                }
            }
            LeagueReward::Building(building) => {
                if let Some(capital) = crate::world::city::capital_of(self.world, rules, player) {
                    self.grant_building(capital, building);
                }
            }
        }
    }

    fn change_league_rewards(&mut self, player: PlayerId, change: impl FnOnce(&mut LeagueRewards)) {
        if let Some(rewards) = self.world.module.congress.rewards.get_mut(player.0 as usize) {
            change(rewards);
        }
    }

    /// Influence with every city-state the player met, once.
    fn boost_city_state_influence(&mut self, player: PlayerId, amount: u16, rules: &CompiledRuleset) {
        for minor in 0..self.world.players.count() {
            let minor = PlayerId(minor as u8);
            let met = crate::systems::diplomacy::has_met(self.world, player, minor);
            if self.world.players.is_city_state(minor) && self.world.players.is_alive(minor) && met {
                self.change_influence(minor, player, i32::from(amount) * 100, rules);
            }
        }
    }

    /// The player's culture and tourism boosts run out a turn.
    pub(crate) fn league_rewards_turn(&mut self, player: PlayerId) {
        self.change_league_rewards(player, |r| {
            r.culture_boost_turns = r.culture_boost_turns.saturating_sub(1);
            r.tourism_boost_turns = r.tourism_boost_turns.saturating_sub(1);
        });
    }
}
