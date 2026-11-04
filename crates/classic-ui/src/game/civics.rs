//! Civics screen data from the policy tables and the player's culture state, and the
//! three culture requests (unlock a branch, adopt a policy, choose an ideology).
use classic_rules::derived::culture::{
    branch_statuses, culture_overview, culture_per_turn, era_index, ideology_status,
    policy_statuses, BranchBlock, PolicyBlock,
};
use classic_rules::ids::PolicyId;
use classic_rules::rules::content::{PolicyBranchId, PolicyBranchKind, PolicyRole};
use classic_rules::{Command, CompiledRuleset};

use i18n::tf;

use super::names;
use super::reasons::{branch, ideology, policy};
use super::session::Session;
use crate::ui::model::{
    Availability, BranchCard, BranchState, CivicsView, GameRequest, NoticeKind, PolicyCard,
};

impl Session {
    pub fn civics_view(&mut self) -> CivicsView {
        let me = self.me();
        let auth = self.game().view().auth;
        let rules = self.rules();
        let per_turn = culture_per_turn(auth, rules, me);
        let overview = culture_overview(auth, rules, me, per_turn);
        let policies = policy_statuses(auth, rules, me);
        let branches = branch_statuses(auth, rules, me);
        let missing = overview.next_policy_cost.saturating_sub(overview.culture);
        let turns = crate::ui::model::turns_to(missing, per_turn);
        let mut cards = Vec::new();
        for kind in [PolicyBranchKind::Normal, PolicyBranchKind::Ideology] {
            for i in branch_order(rules) {
                let def = &rules.module.policy_branches[i];
                if def.kind != kind {
                    continue;
                }
                let id = PolicyBranchId(i as u16);
                let state = match kind {
                    PolicyBranchKind::Normal => normal_state(&overview.branches, id, &branches[i]),
                    PolicyBranchKind::Ideology => {
                        ideology_state(overview.ideology, id, auth, rules, me)
                    }
                };
                let free_tenets = ideology_status(auth, rules, me, id).unwrap_or(0);
                cards.push(branch_card(
                    rules,
                    id,
                    state,
                    free_tenets,
                    &overview.policies,
                    &policies,
                ));
            }
        }
        CivicsView {
            culture: overview.culture,
            per_turn,
            next_cost: overview.next_policy_cost,
            turns,
            free_policies: overview.free_policies,
            free_tenets: overview.free_tenets,
            branches: cards,
        }
    }

    pub(super) fn civics_request(&mut self, request: GameRequest) {
        let player = self.me();
        let (command, done) = match request {
            GameRequest::UnlockBranch(branch) => (
                Command::UnlockPolicyBranch { player, branch },
                tf!("classic.game.civics.unlocked", name = branch_name(self.rules(), branch)),
            ),
            GameRequest::AdoptPolicy(policy) => (
                Command::AdoptPolicy { player, policy },
                tf!("classic.game.civics.adopted", name = policy_name(self.rules(), policy)),
            ),
            GameRequest::ChooseIdeology(ideology) => (
                Command::ChooseIdeology { player, ideology },
                tf!("classic.game.civics.ideology-chosen", name = branch_name(self.rules(), ideology)),
            ),
            _ => return,
        };
        if self.submit(command) {
            self.notify(NoticeKind::Culture, done);
        }
    }
}

fn normal_state(
    unlocked: &[PolicyBranchId],
    id: PolicyBranchId,
    status: &Result<classic_rules::derived::culture::Payment, BranchBlock>,
) -> BranchState {
    if unlocked.contains(&id) {
        return BranchState::Open;
    }
    match status {
        Ok(_) => BranchState::Available,
        Err(block) => BranchState::Locked(branch(*block).into()),
    }
}

fn ideology_state(
    chosen: Option<PolicyBranchId>,
    id: PolicyBranchId,
    auth: &classic_rules::world::state::AuthWorld,
    rules: &CompiledRuleset,
    me: classic_rules::ids::PlayerId,
) -> BranchState {
    if chosen == Some(id) {
        return BranchState::Open;
    }
    match ideology_status(auth, rules, me, id) {
        Ok(_) => BranchState::Available,
        Err(block) => BranchState::Locked(ideology(block).into()),
    }
}

fn branch_card(
    rules: &CompiledRuleset,
    id: PolicyBranchId,
    state: BranchState,
    free_tenets: u8,
    adopted: &[PolicyId],
    statuses: &[Result<classic_rules::derived::culture::Payment, PolicyBlock>],
) -> BranchCard {
    let def = &rules.module.policy_branches[id.0 as usize];
    let slug = slug(&def.key);
    let card = |p: PolicyId, members: &[PolicyId]| {
        policy_card(rules, p, &slug, adopted, statuses, members)
    };
    let members: Vec<PolicyId> = (0..rules.module.policies.len())
        .map(|i| PolicyId(i as u16))
        .filter(|p| {
            let d = &rules.module.policies[p.0 as usize];
            d.branch == Some(id) && matches!(d.role, PolicyRole::Policy | PolicyRole::Tenet(_))
        })
        .collect();
    BranchCard {
        id,
        name: names::named("policy", &def.name, &def.key),
        art: slug.clone(),
        ideology: def.kind == PolicyBranchKind::Ideology,
        era: def
            .era_prerequisite
            .filter(|e| era_index(rules, *e) > 0)
            .map(|e| names::named("era", &rules.eras[e.0 as usize].name, &rules.eras[e.0 as usize].key)),
        state,
        opener: def.opener.map(|p| card(p, &members)),
        finisher: def.finisher.map(|p| card(p, &members)),
        policies: members.iter().map(|p| card(*p, &members)).collect(),
        free_tenets,
    }
}

fn policy_card(
    rules: &CompiledRuleset,
    id: PolicyId,
    branch_slug: &str,
    adopted: &[PolicyId],
    statuses: &[Result<classic_rules::derived::culture::Payment, PolicyBlock>],
    members: &[PolicyId],
) -> PolicyCard {
    let def = &rules.module.policies[id.0 as usize];
    let (art, tier) = match def.role {
        PolicyRole::Opener | PolicyRole::Finisher => (branch_slug.to_string(), 0),
        PolicyRole::Tenet(tier) => (slug_with_branch(&def.key), tier),
        PolicyRole::Policy => (slug_with_branch(&def.key), 0),
    };
    let availability = match &statuses[id.0 as usize] {
        Ok(_) => Availability::Ready,
        Err(block) => Availability::Blocked(policy(*block).into()),
    };
    PolicyCard {
        id,
        name: names::named("policy", &def.name, &def.key),
        art,
        text: if def.text.is_empty() {
            crate::ui::pedia::describe::effect_summary(rules, &def.effects, &def.inert)
        } else {
            help_text(
                &[&def.name, &names::named("policy", &def.name, &def.key)],
                &i18n::content("classic", "policy", &def.key, "desc", &def.text),
            )
        },
        grid: def.grid,
        tier,
        requires: def
            .prerequisites
            .iter()
            .filter_map(|p| members.iter().position(|m| m == p))
            .collect(),
        adopted: adopted.contains(&id),
        availability,
    }
}

/// The catalog text repeats the name as a title ("Legalism Provides ..."): drop the title,
/// keep a sentence that starts with the name ("Tradition is best for ...").
fn help_text(names: &[&str], text: &str) -> String {
    let Some(rest) = names.iter().find_map(|name| text.strip_prefix(*name)) else {
        return text.to_string();
    };
    let rest = rest.trim_start();
    if rest.chars().next().is_some_and(char::is_uppercase) {
        rest.to_string()
    } else {
        text.to_string()
    }
}

/// Branches in the classic screen order: by era, then the game's order of the classic
/// branches (the tables are sorted by key).
fn branch_order(rules: &CompiledRuleset) -> Vec<usize> {
    const ORDER: [&str; 12] = [
        "tradition",
        "liberty",
        "honor",
        "piety",
        "patronage",
        "aesthetics",
        "commerce",
        "exploration",
        "rationalism",
        "freedom",
        "order",
        "autocracy",
    ];
    let mut order: Vec<usize> = (0..rules.module.policy_branches.len()).collect();
    order.sort_by_key(|i| {
        let def = &rules.module.policy_branches[*i];
        let era = def
            .era_prerequisite
            .map(|e| era_index(rules, e))
            .unwrap_or(0);
        let place = ORDER
            .iter()
            .position(|k| *k == slug(&def.key))
            .unwrap_or(ORDER.len());
        (era, place, *i)
    });
    order
}

/// "core:policy/tradition" -> "tradition".
fn slug(key: &str) -> String {
    key.strip_prefix("core:policy/").unwrap_or(key).to_string()
}

/// "core:policy/tradition/legalism" -> "tradition/legalism" (the art catalog id).
fn slug_with_branch(key: &str) -> String {
    slug(key)
}

fn branch_name(rules: &CompiledRuleset, id: PolicyBranchId) -> String {
    rules
        .module.policy_branches
        .get(id.0 as usize)
        .map(|b| names::named("policy", &b.name, &b.key))
        .unwrap_or_default()
}

fn policy_name(rules: &CompiledRuleset, id: PolicyId) -> String {
    rules
        .module.policies
        .get(id.0 as usize)
        .map(|p| names::named("policy", &p.name, &p.key))
        .unwrap_or_default()
}
