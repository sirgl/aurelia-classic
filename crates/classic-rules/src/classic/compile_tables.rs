//! Compilation of the classic tables (`rules::tables::ModuleTables`):
//! policies and their branches, beliefs and religions, projects,
//! city-states and great people, their cross-table rules and the effects
//! of policies and beliefs.

use crate::classic::effects::{ClassicSource, ClassicTable};
use crate::classic::tables::{
    BeliefDefinition, CityStateDefinition, CityStateTypeDefinition, ClassicTables,
    GreatPersonDefinition, LeagueProject, LeagueReward, PolicyBranchDefinition, PolicyDefinition,
    ProjectDefinition, ReligionDefinition,
};
use crate::ids::{BeliefId, PolicyId, UnitKindId};
use crate::rules::compile::{effect_sets, era, tech, RulesError, SourceKeys};
use crate::rules::compile_effects::{compile_inert, SourceKind};
use crate::rules::compile_tables::check_acyclic;
use crate::rules::compiled::CompiledRuleset;
use crate::rules::content::{
    CityStateTypeId, ContentTable, Personality, PolicyBranchId, PolicyBranchKind, PolicyRole, ProjectId,
    ProjectKind, SpecialistId,
};
use crate::classic::tables::{BELIEFS, CITY_STATES, CITY_STATE_TYPES, GREAT_PEOPLE, POLICIES, POLICY_BRANCHES, RELIGIONS};
use crate::rules::effects::EffectSet;
use crate::rules::key_index::KeyIndex;
use crate::rules::source::{SourceLeagueProject, SourceLeagueReward, SourceRules};
use crate::classic::content::GREAT_PERSON_ROLE;

pub(crate) fn compile(
    s: &SourceRules,
    k: &SourceKeys,
    ix: &KeyIndex,
    rules: &CompiledRuleset,
) -> Result<ClassicTables, RulesError> {
    Ok(ClassicTables {
        policies: compile_policies(s, k, ix)?,
        beliefs: compile_beliefs(s, k, ix)?,
        projects: compile_projects(s, k, ix)?,
        policy_branches: compile_policy_branches(s, k, ix)?,
        religions: compile_religions(s, k),
        city_state_types: compile_city_state_types(s, k, ix)?,
        city_states: compile_city_states(s, k, ix)?,
        great_people: compile_great_people(s, k, ix, rules)?,
        personalities: compile_personalities(s, ix, rules.civs.len())?,
    })
}

/// Personalities by civilization index: one row per civilization at most,
/// every value on the scale; a civilization without a row is neutral.
fn compile_personalities(s: &SourceRules, ix: &KeyIndex, civs: usize) -> Result<Vec<Personality>, RulesError> {
    let mut out = vec![Personality::NEUTRAL; civs];
    let mut named = vec![false; civs];
    for row in &s.module.personalities {
        let civ = usize::from(ix.id(ContentTable::Civilization, &row.civ.0)?);
        let key = &row.civ.0;
        if std::mem::replace(&mut named[civ], true) {
            return Err(RulesError::Inconsistent(format!("{key}: two personalities")));
        }
        if row.personality.values().iter().any(|v| *v > Personality::MAX) {
            return Err(RulesError::Inconsistent(format!("{key}: a personality value above {}", Personality::MAX)));
        }
        out[civ] = row.personality;
    }
    Ok(out)
}

/// Policy branches own their opener and finisher, prerequisites of
/// policies are acyclic, World Congress projects reward unbuildable
/// buildings, nuclear weapons need their project.
pub(crate) fn check(rules: &CompiledRuleset) -> Result<(), RulesError> {
    check_branches(rules)?;
    check_projects(rules)?;
    let tables = &rules.module;
    let policies: Vec<Vec<u16>> = tables
        .policies
        .iter()
        .map(|p| p.prerequisites.iter().map(|x| x.0).collect())
        .collect();
    check_acyclic(&policies, |i| &tables.policies[i].key)
}

/// The effects of policies and beliefs, compiled against the finished tables.
pub(crate) fn attach_effects(
    s: &SourceRules,
    k: &SourceKeys,
    ix: &KeyIndex,
    rules: &mut CompiledRuleset,
) -> Result<(), RulesError> {
    let policy = SourceKind::Module(ClassicTable::Policy);
    let sets = effect_sets(rules, ix, policy, k.module_table(POLICIES), |i| &s.module.policies[i].effects)?;
    for (def, set) in rules.module.policies.iter_mut().zip(sets) {
        def.effects = set;
    }
    let belief = SourceKind::Module(ClassicTable::Belief);
    let sets = effect_sets(rules, ix, belief, k.module_table(BELIEFS), |i| &s.module.beliefs[i].effects)?;
    for (def, set) in rules.module.beliefs.iter_mut().zip(sets) {
        def.effects = set;
    }
    Ok(())
}

/// Policies and beliefs, in the module's order.
pub(crate) fn effect_sets_of(tables: &ClassicTables) -> Vec<(ClassicSource, &EffectSet)> {
    let policies = tables.policies.iter().enumerate();
    let policies = policies.map(|(i, p)| (ClassicSource::Policy(PolicyId(i as u16)), &p.effects));
    let beliefs = tables.beliefs.iter().enumerate();
    let beliefs = beliefs.map(|(i, b)| (ClassicSource::Belief(BeliefId(i as u16)), &b.effects));
    policies.chain(beliefs).collect()
}

fn compile_policies(s: &SourceRules, k: &SourceKeys, ix: &KeyIndex) -> Result<Vec<PolicyDefinition>, RulesError> {
    let mut out = Vec::with_capacity(s.module.policies.len());
    let keys = k.module_table(POLICIES);
    for (id, row) in keys.order.iter().enumerate() {
        let p = &s.module.policies[*row];
        let key = keys.key_of(id);
        let branch = ix.opt_id(POLICY_BRANCHES, &p.branch)?;
        let prerequisites = ix.ids(POLICIES, &p.prerequisites)?;
        out.push(PolicyDefinition {
            key: key.to_string(),
            effects: EffectSet::default(),
            name: p.name.clone(),
            branch: branch.map(PolicyBranchId),
            role: p.role,
            prerequisites: prerequisites.into_iter().map(PolicyId).collect(),
            grid: p.grid,
            text: p.text.clone(),
            inert: compile_inert(ix, key, &p.inert)?,
        });
    }
    Ok(out)
}

fn compile_beliefs(s: &SourceRules, k: &SourceKeys, ix: &KeyIndex) -> Result<Vec<BeliefDefinition>, RulesError> {
    let mut out = Vec::with_capacity(s.module.beliefs.len());
    let keys = k.module_table(BELIEFS);
    for (id, row) in keys.order.iter().enumerate() {
        let b = &s.module.beliefs[*row];
        let key = keys.key_of(id);
        out.push(BeliefDefinition {
            key: key.to_string(),
            effects: EffectSet::default(),
            name: b.name.clone(),
            category: b.category,
            text: b.text.clone(),
            inert: compile_inert(ix, key, &b.inert)?,
        });
    }
    Ok(out)
}

/// Key and name of every religion, in id order.
fn compile_religions(s: &SourceRules, k: &SourceKeys) -> Vec<ReligionDefinition> {
    let keys = k.module_table(RELIGIONS);
    let rows = keys.order.iter().enumerate();
    rows.map(|(id, row)| ReligionDefinition {
        key: keys.key_of(id).to_string(),
        name: s.module.religions[*row].name.clone(),
    })
    .collect()
}

/// Projects: World Congress projects have no production cost, the others
/// must name one.
fn compile_projects(
    s: &SourceRules,
    k: &SourceKeys,
    ix: &KeyIndex,
) -> Result<Vec<ProjectDefinition>, RulesError> {
    let mut out = Vec::with_capacity(s.module.projects.len());
    let keys = k.module_table(ContentTable::Project);
    for (id, row) in keys.order.iter().enumerate() {
        let p = &s.module.projects[*row];
        let key = keys.key_of(id);
        let kind = p.kind.unwrap_or(ProjectKind::Project);
        let buildable = kind == ProjectKind::Project && !p.unbuildable;
        if buildable && p.cost == 0 {
            return Err(RulesError::ZeroCost(key.to_string()));
        }
        if !buildable && p.cost != 0 && kind == ProjectKind::Project {
            let reason = format!("{key}: unbuildable with a cost");
            return Err(RulesError::Inconsistent(reason));
        }
        let mut prerequisites = Vec::with_capacity(p.prerequisite_projects.len());
        for prereq in &p.prerequisite_projects {
            let id = ix.id(ContentTable::Project, &prereq.key.0)?;
            prerequisites.push((ProjectId(id), prereq.count));
        }
        out.push(ProjectDefinition {
            key: key.to_string(),
            name: p.name.clone(),
            kind,
            cost: p.cost,
            unbuildable: p.unbuildable,
            required_tech: tech(ix, &p.required_tech)?,
            prerequisite_projects: prerequisites,
            max_global_instances: p.max_global_instances,
            max_team_instances: p.max_team_instances,
            victory: p.victory.clone(),
            allows_nukes: p.allows_nukes,
            league: league_project(ix, key, kind, p.league.as_ref())?,
            inert: compile_inert(ix, key, &p.inert)?,
        });
    }
    Ok(out)
}

/// The World Congress data of a project: present exactly on World Congress
/// projects, three tiers, a cost per civilization, rewards that exist (a
/// rewarded building is never built with production).
fn league_project(
    ix: &KeyIndex,
    key: &str,
    kind: ProjectKind,
    source: Option<&SourceLeagueProject>,
) -> Result<Option<LeagueProject>, RulesError> {
    let inconsistent = |reason: &str| Err(RulesError::Inconsistent(format!("{key}: {reason}")));
    let source = match (kind, source) {
        (ProjectKind::Project, None) => return Ok(None),
        (ProjectKind::Project, Some(_)) => return inconsistent("World Congress data on a project"),
        (ProjectKind::LeagueProject, None) => return inconsistent("a World Congress project without its data"),
        (ProjectKind::LeagueProject, Some(source)) => source,
    };
    if source.cost_per_player == 0 || source.tiers.len() != 3 {
        return inconsistent("a World Congress project needs a cost per player and three tiers");
    }
    let mut tiers: [Vec<LeagueReward>; 3] = Default::default();
    for (tier, rewards) in tiers.iter_mut().zip(&source.tiers) {
        for reward in rewards {
            tier.push(league_reward(ix, reward)?);
        }
    }
    Ok(Some(LeagueProject {
        cost_per_player: source.cost_per_player,
        tech_any_member: tech(ix, &source.tech_any_member)?,
        tiers,
    }))
}

fn league_reward(ix: &KeyIndex, reward: &SourceLeagueReward) -> Result<LeagueReward, RulesError> {
    Ok(match reward {
        SourceLeagueReward::Happiness(n) => LeagueReward::Happiness(*n),
        SourceLeagueReward::FreePolicies(n) => LeagueReward::FreePolicies(*n),
        SourceLeagueReward::GoldenAgePoints(n) => LeagueReward::GoldenAgePoints(*n),
        SourceLeagueReward::CultureBoostTurns(n) => LeagueReward::CultureBoostTurns(*n),
        SourceLeagueReward::TourismBoostTurns(n) => LeagueReward::TourismBoostTurns(*n),
        SourceLeagueReward::CityStateInfluence(n) => LeagueReward::CityStateInfluence(*n),
        SourceLeagueReward::ResearchTurns(n) => LeagueReward::ResearchTurns(*n),
        SourceLeagueReward::FreeUnit(k) => LeagueReward::FreeUnit(UnitKindId(ix.id(ContentTable::Unit, &k.0)?)),
        SourceLeagueReward::Building(k) => {
            LeagueReward::Building(crate::ids::BuildingKindId(ix.id(ContentTable::Building, &k.0)?))
        }
    })
}

fn compile_policy_branches(
    s: &SourceRules,
    k: &SourceKeys,
    ix: &KeyIndex,
) -> Result<Vec<PolicyBranchDefinition>, RulesError> {
    let mut out = Vec::with_capacity(s.module.policy_branches.len());
    let keys = k.module_table(POLICY_BRANCHES);
    for (id, row) in keys.order.iter().enumerate() {
        let b = &s.module.policy_branches[*row];
        let key = keys.key_of(id);
        let exclusive = ix.ids(POLICY_BRANCHES, &b.mutually_exclusive_with)?;
        out.push(PolicyBranchDefinition {
            key: key.to_string(),
            name: b.name.clone(),
            kind: b.kind.unwrap_or(PolicyBranchKind::Normal),
            era_prerequisite: era(ix, &b.era_prerequisite)?,
            opener: ix.opt_id(POLICIES, &b.opener)?.map(PolicyId),
            finisher: ix.opt_id(POLICIES, &b.finisher)?.map(PolicyId),
            mutually_exclusive_with: exclusive.into_iter().map(PolicyBranchId).collect(),
            locked_without_religion: b.locked_without_religion,
            first_adopter_free_policies: b.first_adopter_free_policies,
            second_adopter_free_policies: b.second_adopter_free_policies,
        });
    }
    Ok(out)
}

fn compile_city_state_types(
    s: &SourceRules,
    k: &SourceKeys,
    ix: &KeyIndex,
) -> Result<Vec<CityStateTypeDefinition>, RulesError> {
    let mut out = Vec::with_capacity(s.module.city_state_types.len());
    let keys = k.module_table(CITY_STATE_TYPES);
    for (id, row) in keys.order.iter().enumerate() {
        let t = &s.module.city_state_types[*row];
        let key = keys.key_of(id);
        out.push(CityStateTypeDefinition {
            key: key.to_string(),
            name: t.name.clone(),
            friends_threshold: t.friends_threshold,
            allies_threshold: t.allies_threshold,
            minor_trait: t.minor_trait,
            bonuses: compile_city_state_bonuses(ix, &t.bonuses)?,
            inert: compile_inert(ix, key, &t.inert)?,
        });
    }
    Ok(out)
}

fn compile_city_state_bonuses(
    ix: &KeyIndex,
    bonuses: &[crate::rules::source::SourceCityStateBonus],
) -> Result<Vec<crate::classic::tables::CityStateBonus>, RulesError> {
    let mut out = Vec::with_capacity(bonuses.len());
    for b in bonuses {
        let era = match &b.era {
            Some(key) => Some(crate::rules::content::EraId(ix.id(ContentTable::Era, &key.0)?)),
            None => None,
        };
        out.push(crate::classic::tables::CityStateBonus {
            level: b.level,
            kind: b.kind,
            era,
            amount: b.amount,
        });
    }
    Ok(out)
}

fn compile_city_states(
    s: &SourceRules,
    k: &SourceKeys,
    ix: &KeyIndex,
) -> Result<Vec<CityStateDefinition>, RulesError> {
    let mut out = Vec::with_capacity(s.module.city_states.len());
    let keys = k.module_table(CITY_STATES);
    for (id, row) in keys.order.iter().enumerate() {
        let c = &s.module.city_states[*row];
        let key = keys.key_of(id);
        let Some(kind) = &c.city_state_type else {
            let reason = format!("{key}: city-state without a type");
            return Err(RulesError::Inconsistent(reason));
        };
        out.push(CityStateDefinition {
            key: key.to_string(),
            name: c.name.clone(),
            city_state_type: CityStateTypeId(ix.id(CITY_STATE_TYPES, &kind.0)?),
        });
    }
    Ok(out)
}

/// Great people: the unit must exist and have the great person role.
fn compile_great_people(
    s: &SourceRules,
    k: &SourceKeys,
    ix: &KeyIndex,
    rules: &CompiledRuleset,
) -> Result<Vec<GreatPersonDefinition>, RulesError> {
    let mut out = Vec::with_capacity(s.module.great_people.len());
    let keys = k.module_table(GREAT_PEOPLE);
    for (id, row) in keys.order.iter().enumerate() {
        let g = &s.module.great_people[*row];
        let key = keys.key_of(id);
        let (Some(kind), Some(unit)) = (g.kind, &g.unit) else {
            let reason = format!("{key}: great person needs a kind and a unit");
            return Err(RulesError::Inconsistent(reason));
        };
        let unit = UnitKindId(ix.id(ContentTable::Unit, &unit.0)?);
        if rules.units[unit.0 as usize].role != Some(GREAT_PERSON_ROLE) {
            let reason = format!("{key}: its unit is not a great person");
            return Err(RulesError::Inconsistent(reason));
        }
        let specialist = ix.opt_id(ContentTable::Specialist, &g.specialist)?;
        out.push(GreatPersonDefinition {
            key: key.to_string(),
            kind,
            unit,
            specialist: specialist.map(SpecialistId),
            improvements: crate::rules::compile::improvements(ix, &g.improvements)?,
            points_source: g.points_source,
            inert: compile_inert(ix, key, &g.inert)?,
        });
    }
    Ok(out)
}

/// Cross-table rules: unlock lists mirror `required_tech`, unique items
/// mirror their civilization, prerequisite graphs of policies and
/// promotions are acyclic, policy branches own their opener and finisher.
/// A building a World Congress project grants is not built with production
/// (the International Space Station); a
/// nuclear weapon needs a project that allows nuclear weapons.
fn check_projects(rules: &CompiledRuleset) -> Result<(), RulesError> {
    for project in &rules.module.projects {
        let Some(league) = &project.league else {
            continue;
        };
        for reward in league.tiers.iter().flatten() {
            let LeagueReward::Building(b) = reward else {
                continue;
            };
            let building = &rules.buildings[b.0 as usize];
            if !building.unbuildable {
                let reason = format!("{}: {} is rewarded by a World Congress project but buildable", project.key, building.key);
                return Err(RulesError::Inconsistent(reason));
            }
        }
    }
    for unit in rules.units.iter().filter(|u| u.combat.nuke_level > 0) {
        let allowed = unit.required_project.is_some_and(|p| rules.module.projects[p.0 as usize].allows_nukes);
        if !allowed {
            let reason = format!("{}: a nuclear weapon needs a project that allows nuclear weapons", unit.key);
            return Err(RulesError::Inconsistent(reason));
        }
    }
    Ok(())
}

/// Openers and finishers belong to their branch; tenets to an ideology.
fn check_branches(rules: &CompiledRuleset) -> Result<(), RulesError> {
    for (b, branch) in rules.module.policy_branches.iter().enumerate() {
        let own = Some(PolicyBranchId(b as u16));
        for (policy, role) in [
            (branch.opener, PolicyRole::Opener),
            (branch.finisher, PolicyRole::Finisher),
        ] {
            let Some(policy) = policy else { continue };
            let def = &rules.module.policies[policy.0 as usize];
            if def.branch != own || def.role != role {
                let reason = format!("{}: {} is not its {role:?}", branch.key, def.key);
                return Err(RulesError::Inconsistent(reason));
            }
        }
    }
    for policy in &rules.module.policies {
        let Some(branch) = policy.branch else {
            continue;
        };
        let ideology = rules.module.policy_branches[branch.0 as usize].kind == PolicyBranchKind::Ideology;
        if matches!(policy.role, PolicyRole::Tenet(_)) != ideology {
            let reason = format!("{}: tenets belong to ideologies only", policy.key);
            return Err(RulesError::Inconsistent(reason));
        }
    }
    Ok(())
}
