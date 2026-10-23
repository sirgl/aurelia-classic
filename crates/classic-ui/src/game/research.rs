//! Research screen data from the real technology table and the player's progress.
use classic_rules::ids::TechId;
use classic_rules::{Command, CompiledRuleset};
use i18n::{t, tf};

use super::names;
use super::session::Session;
use crate::ui::model::{Availability, ResearchSummary, ResearchView, TechCard, TechStatus, TechUnlock};

impl Session {
    pub fn research_view(&mut self) -> ResearchView {
        let layout = tree_layout(self.rules());
        let free = classic_rules::derived::grants::next_technology_options(self.game().view().auth, self.rules(), self.me());
        let free_choice = !classic_rules::derived::grants::owed(self.game().view().auth, self.me()).technologies.is_empty();
        let techs = (0..self.rules().technologies.len())
            .map(|i| self.tech_card(TechId(i as u16), layout[i], free_choice, &free))
            .collect();
        let economy = self.economy();
        ResearchView {
            techs,
            free_choice,
            stored: economy.science,
            per_turn: economy.science_per_turn,
        }
    }

    fn tech_card(&mut self, id: TechId, (column, row): (usize, usize), free_choice: bool, free: &[TechId]) -> TechCard {
        let def = self.rules().technologies[id.0 as usize].clone();
        let command = if free_choice {
            Command::ChooseFreeTechnology { player: self.me(), tech: id }
        } else { Command::PlanResearch { player: self.me(), target: id } };
        let availability = if free_choice && !free.contains(&id) {
            Availability::Blocked(super::reasons::free_tech_first().into())
        } else { self.availability(&command) };
        let plan = if free_choice { Vec::new() } else {
            classic_rules::plan_research(self.rules(), self.game().view().auth.players().techs_of(self.me()), id)
                .unwrap_or_default().iter().map(|id| id.0 as usize).collect()
        };
        TechCard {
            id,
            name: names::named("tech", &def.name, &def.key),
            art: names::art(&def.key),
            era: def.era_name.as_deref().map(|era| names::era_named(self.rules(), era)).unwrap_or_default(),
            cost: classic_rules::technology_cost(self.game().view().auth, self.rules(), self.me(), id),
            column,
            row,
            requires: def.requires.iter().map(|t| t.0 as usize).collect(),
            status: self.tech_status(id),
            unlocks: unlocks(self.rules(), id),
            availability,
            plan,
        }
    }

    fn tech_status(&self, id: TechId) -> TechStatus {
        let players = self.game().view().auth.players();
        if players.techs_of(self.me()).contains(&id) {
            return TechStatus::Known;
        }
        let queue = players.research_queue_of(self.me());
        match queue.iter().position(|t| *t == id) {
            Some(0) => TechStatus::Researching,
            Some(n) => TechStatus::Queued(n),
            None => self.locked_or_available(id),
        }
    }

    fn locked_or_available(&self, id: TechId) -> TechStatus {
        let known = self.game().view().auth.players().techs_of(self.me());
        let missing: Vec<String> = self.rules().technologies[id.0 as usize]
            .requires
            .iter()
            .filter(|t| !known.contains(t))
            .map(|t| names::tech(self.rules(), *t))
            .collect();
        if missing.is_empty() {
            TechStatus::Available
        } else {
            TechStatus::Locked(format!("Requires {}", missing.join(", ")))
        }
    }

    pub fn research_summary(&self) -> Option<ResearchSummary> {
        let players = self.game().view().auth.players();
        let head = *players.research_queue_of(self.me()).first()?;
        let def = self.rules().technologies.get(head.0 as usize)?;
        let stored = players.science_of(self.me());
        let per_turn = self.economy().science_per_turn;
        let cost = classic_rules::technology_cost(self.game().view().auth, self.rules(), self.me(), head);
        let missing = cost.saturating_sub(stored);
        Some(ResearchSummary {
            name: names::named("tech", &def.name, &def.key),
            art: names::art(&def.key),
            stored,
            cost,
            turns: (per_turn > 0).then(|| missing.div_ceil(per_turn).max(1)),
        })
    }

    /// A technology the player is owed for free (Great Library, Oxford,
    /// Rationalism, Assyria) is taken for free; any other is queued.
    pub(super) fn choose_research(&mut self, technology: TechId) {
        let player = self.me();
        let free = classic_rules::derived::grants::next_technology_options(self.game().view().auth, self.rules(), player);
        if free.contains(&technology) {
            self.submit(Command::ChooseFreeTechnology { player, tech: technology });
            return;
        }
        if !classic_rules::derived::grants::owed(self.game().view().auth, player).technologies.is_empty() {
            self.message = Some(t!("classic.game.research.free-tech-first").into());
            return;
        }
        self.submit(Command::PlanResearch { player, target: technology });
    }
}

/// Column = longest prerequisite chain; rows fill each column in content order.
fn tree_layout(rules: &CompiledRuleset) -> Vec<(usize, usize)> {
    let count = rules.technologies.len();
    let mut depth = vec![0usize; count];
    // Prerequisites form a DAG; `count` relaxation passes always converge.
    for _ in 0..count {
        let mut changed = false;
        for (i, def) in rules.technologies.iter().enumerate() {
            let want = def
                .requires
                .iter()
                .map(|t| depth[t.0 as usize] + 1)
                .max()
                .unwrap_or(0);
            if want > depth[i] {
                depth[i] = want;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // An era may span several dependency columns. Keep each band contiguous,
    // while ordering rows by their already placed prerequisites.
    let mut eras: Vec<(String, usize, usize)> = Vec::new();
    for (i, tech) in rules.technologies.iter().enumerate() {
        let label = tech.era_name.clone().unwrap_or_default();
        if let Some(era) = eras.iter_mut().find(|e| e.0 == label) {
            era.1 = era.1.min(depth[i]); era.2 = era.2.max(depth[i]);
        } else { eras.push((label, depth[i], depth[i])); }
    }
    eras.sort_by_key(|era| era.1);
    let mut columns = 0;
    let mut starts = Vec::new();
    for era in &eras { starts.push(columns); columns += era.2 - era.1 + 1; }
    let mut groups = vec![Vec::new(); columns];
    for (i, tech) in rules.technologies.iter().enumerate() {
        let era = eras.iter().position(|e| Some(&e.0) == tech.era_name.as_ref()).unwrap_or(0);
        groups[starts[era] + depth[i] - eras[era].1].push(i);
    }
    let rows = groups.iter().map(Vec::len).max().unwrap_or(1);
    let mut layout = vec![(0, 0); count];
    for (column, group) in groups.iter_mut().enumerate() {
        let center = |i: usize| {
            let requires = &rules.technologies[i].requires;
            if requires.is_empty() { 0.0 } else {
                requires.iter().map(|p| layout[p.0 as usize].1 as f32).sum::<f32>() / requires.len() as f32
            }
        };
        group.sort_by(|a,b| center(*a).total_cmp(&center(*b)).then_with(|| rules.technologies[*a].key.cmp(&rules.technologies[*b].key)));
        let offset = (rows - group.len()) / 2;
        for (row, &i) in group.iter().enumerate() { layout[i] = (column, row + offset); }
    }
    layout
}

/// What a technology unlocks: «Unit: Archer», or with «(Egypt only)» for a unique one.
fn unlock_name(rules: &CompiledRuleset, what: Unlock, kind: &str, name: &str, key: &str, nation: Option<u8>) -> String {
    let shown = names::named(kind, name, key);
    match (what, nation) {
        (Unlock::Unit, None) => tf!("classic.game.research.unlock-unit", name = shown),
        (Unlock::Unit, Some(id)) => tf!("classic.game.research.unlock-unit-only", name = shown, nation = names::nation(rules, id)),
        (Unlock::Wonder, None) => tf!("classic.game.research.unlock-wonder", name = shown),
        (Unlock::Wonder, Some(id)) => tf!("classic.game.research.unlock-wonder-only", name = shown, nation = names::nation(rules, id)),
        (Unlock::Building, None) => tf!("classic.game.research.unlock-building", name = shown),
        (Unlock::Building, Some(id)) => tf!("classic.game.research.unlock-building-only", name = shown, nation = names::nation(rules, id)),
        (Unlock::Improvement, None) => tf!("classic.game.research.unlock-improvement", name = shown),
        (Unlock::Improvement, Some(id)) => tf!("classic.game.research.unlock-improvement-only", name = shown, nation = names::nation(rules, id)),
    }
}

#[derive(Clone, Copy)]
enum Unlock {
    Unit,
    Wonder,
    Building,
    Improvement,
}

fn unlocks(rules: &CompiledRuleset, id: TechId) -> Vec<TechUnlock> {
    let mut items = Vec::new();
    for def in &rules.units {
        if def.required_tech == Some(id) {
            items.push(TechUnlock { name: unlock_name(rules, Unlock::Unit, "unit", &def.name, &def.key, def.civilization), art: crate::ui::pedia::art::existing_card_for_key(&def.key) });
        }
    }
    for def in &rules.buildings {
        if def.required_tech == Some(id) {
            let what = if def.wonder { Unlock::Wonder } else { Unlock::Building };
            items.push(TechUnlock { name: unlock_name(rules, what, "building", &def.name, &def.key, def.civilization), art: crate::ui::pedia::art::existing_card_for_key(&def.key) });
        }
    }
    for def in &rules.improvements {
        if def.required_tech == Some(id) {
            items.push(TechUnlock { name: unlock_name(rules, Unlock::Improvement, "improvement", &def.name, &def.key, def.civilization), art: crate::ui::pedia::art::existing_card_for_key(&def.key) });
        }
    }
    for def in &rules.module.projects {
        if def.required_tech == Some(id) {
            items.push(TechUnlock { name: tf!("classic.game.research.unlock-project", name = names::named("project", &def.name, &def.key)), art: crate::ui::pedia::art::project(&def.key) });
        }
    }
    for def in &rules.resources {
        if def.required_tech == Some(id) {
            items.push(TechUnlock { name: tf!("classic.game.research.unlock-resource", name = names::named("resource", &def.name, &def.key)), art: crate::ui::pedia::art::existing_card_for_key(&def.key) });
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_layout_places_every_tech_once_after_its_prerequisites() {
        let rules = crate::ui::screens::game_rules();
        let layout = tree_layout(&rules);
        assert_eq!(layout.len(), rules.technologies.len());
        let mut cells = layout.clone();
        cells.sort();
        cells.dedup();
        assert_eq!(cells.len(), layout.len(), "no two cards share a cell");
        for (i, def) in rules.technologies.iter().enumerate() {
            for required in &def.requires {
                assert!(layout[required.0 as usize].0 < layout[i].0, "{}", def.key);
            }
        }
    }
    #[test]
    fn free_choice_cannot_be_used_to_unlock_a_distant_target() {
        let rules=crate::ui::screens::game_rules();
        let game=classic_rules::scenario::generate_player_game(&rules,&classic_rules::scenario::ScenarioConfig::quick_game(),0);
        let mut save=classic_rules::persistence::encode::encode(&game,&rules);
        save.world.economy.players[0].free_technologies.push(classic_rules::persistence::economy::FREE_TECH_ANY);
        let game=classic_rules::persistence::decode::decode(&save,&rules).unwrap();
        let mut session=Session::new(game,rules);
        let view=session.research_view(); assert!(view.free_choice);
        let far=view.techs.iter().find(|t|t.name=="Future Tech").unwrap();
        assert!(!far.availability.is_ready()); assert!(far.plan.is_empty());
        let before=session.world_hash(); session.choose_research(far.id); assert_eq!(before,session.world_hash());
        let eligible=view.techs.iter().find(|t|t.availability.is_ready()).unwrap();
        let stock=session.game().view().auth.players().science_of(session.me());
        session.choose_research(eligible.id);
        assert!(session.game().view().auth.players().techs_of(session.me()).contains(&eligible.id));
        assert_eq!(stock,session.game().view().auth.players().science_of(session.me()));
        assert!(!session.research_view().free_choice);
    }

}
