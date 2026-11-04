// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Derived from Unciv (https://github.com/yairm210/Unciv), tag 4.22.10, MPL-2.0: the leaders of
// android/assets/jsons/Civ V - Gods & Kings/Personalities.json (values and policy priorities, the
// BNW leaders it carries included) and each nation's preferredVictoryType from Nations.json;
// core/src/com/unciv/models/ruleset/nation/Personality.kt (`scaledFocus`, `modifierFocus`, ...).

//! Unciv's leader personalities: fifteen values from 0 to 10 (5 neutral) and
//! the policy branch priorities of each leader, as Unciv's data has them,
//! found by the leader's name of the classic rules' nation. A leader Unciv
//! does not have (Maria I of Portugal) plays Unciv's template: every value 5,
//! no priorities.

/// A path to victory (Unciv's `VictoryTypes.json` names).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Victory {
    Neutral,
    Cultural,
    Diplomatic,
    Domination,
    Scientific,
}

/// Unciv's `PersonalityValue`, in the order of `UncivPersonality::values`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    Production,
    Food,
    Gold,
    Science,
    Culture,
    Happiness,
    Faith,
    Military,
    Aggressive,
    DeclareWar,
    Commerce,
    Diplomacy,
    Loyal,
    Expansion,
    DenounceWillingness,
}

/// The policy branches Unciv's priorities name, in the order of
/// `UncivPersonality::priorities`.
pub const BRANCHES: [&str; 10] = ["Tradition", "Liberty", "Honor", "Commerce", "Patronage", "Piety", "Rationalism", "Freedom", "Autocracy", "Order"];

/// One leader of Unciv's data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UncivPersonality {
    pub leader: &'static str,
    /// The nation's `preferredVictoryType`.
    pub victory: Victory,
    pub values: [u8; 15],
    pub priorities: [u8; 10],
}

/// Unciv's template: every value neutral.
pub const TEMPLATE: UncivPersonality = UncivPersonality { leader: "Template", victory: Victory::Neutral, values: [5; 15], priorities: [0; 10] };

impl UncivPersonality {
    /// The personality of the classic nation `nation` (by its leader's name).
    pub fn of(rules: &crate::rules::CompiledRuleset, nation: u8) -> UncivPersonality {
        let Some(def) = rules.nations.get(usize::from(nation)) else {
            return TEMPLATE;
        };
        let leader = def.leader.as_str();
        let last = leader.split_whitespace().last().unwrap_or(leader);
        LEADERS
            .iter()
            .find(|p| p.leader == leader)
            .or_else(|| LEADERS.iter().find(|p| p.leader.split_whitespace().any(|w| w == last) && !matches!(leader, "Maria I")))
            .copied()
            .unwrap_or(TEMPLATE)
    }

    pub fn get(&self, value: Value) -> f32 {
        f32::from(self.values[value as usize])
    }
    /// `scaledFocus`: 0 → 0, 5 → 1, 10 → 2.
    pub fn scaled_focus(&self, value: Value) -> f32 {
        self.get(value) / 5.0
    }
    /// `inverseScaledFocus`: 0 → 2, 5 → 1, 10 → 0.
    pub fn inverse_scaled_focus(&self, value: Value) -> f32 {
        (10.0 - self.get(value)) / 5.0
    }
    /// `modifierFocus`.
    pub fn modifier_focus(&self, value: Value, weight: f32) -> f32 {
        1.0 + (self.scaled_focus(value) - 1.0) * weight
    }
    /// `inverseModifierFocus`.
    pub fn inverse_modifier_focus(&self, value: Value, weight: f32) -> f32 {
        1.0 + (self.inverse_scaled_focus(value) - 1.0) * weight
    }
    /// The leader's priority of the policy branch Unciv names `branch` (0 for another).
    pub fn priority(&self, branch: &str) -> u8 {
        BRANCHES.iter().position(|b| *b == branch).map_or(0, |i| self.priorities[i])
    }
}

/// Unciv's leaders (Personalities.json), generated.
pub const LEADERS: &[UncivPersonality] = &[
    UncivPersonality { leader: "Ahmad al-Mansur", victory: Victory::Neutral, values: [6, 6, 5, 5, 7, 5, 6, 5, 3, 5, 6, 6, 7, 4, 5], priorities: [7, 5, 4, 5, 5, 6, 5, 5, 4, 7] },
    UncivPersonality { leader: "Alexander", victory: Victory::Diplomatic, values: [5, 4, 3, 6, 7, 5, 5, 5, 8, 6, 6, 3, 4, 8, 7], priorities: [7, 8, 8, 4, 3, 6, 7, 5, 8, 5] },
    UncivPersonality { leader: "Ashurbanipal", victory: Victory::Neutral, values: [6, 5, 5, 7, 5, 5, 4, 8, 7, 8, 3, 5, 4, 6, 6], priorities: [5, 6, 7, 4, 3, 5, 8, 5, 7, 3] },
    UncivPersonality { leader: "Askia", victory: Victory::Domination, values: [5, 6, 8, 4, 6, 4, 7, 6, 8, 7, 6, 6, 6, 5, 5], priorities: [5, 5, 8, 8, 4, 8, 5, 4, 7, 4] },
    UncivPersonality { leader: "Attila the Hun", victory: Victory::Domination, values: [6, 4, 7, 3, 5, 5, 5, 6, 8, 8, 6, 3, 4, 7, 6], priorities: [5, 7, 9, 7, 3, 5, 3, 5, 8, 4] },
    UncivPersonality { leader: "Augustus Caesar", victory: Victory::Scientific, values: [6, 5, 6, 5, 5, 8, 4, 7, 4, 5, 4, 7, 4, 8, 7], priorities: [5, 9, 6, 6, 7, 4, 6, 6, 5, 8] },
    UncivPersonality { leader: "Otto von Bismarck", victory: Victory::Scientific, values: [8, 5, 5, 7, 5, 5, 3, 8, 4, 6, 5, 9, 6, 7, 8], priorities: [5, 7, 4, 5, 7, 4, 8, 5, 5, 7] },
    UncivPersonality { leader: "Boudicca", victory: Victory::Domination, values: [5, 4, 6, 7, 7, 8, 8, 6, 7, 6, 5, 4, 6, 5, 6], priorities: [7, 5, 6, 6, 5, 8, 7, 8, 6, 6] },
    UncivPersonality { leader: "Casimir III", victory: Victory::Neutral, values: [8, 4, 7, 4, 6, 6, 7, 8, 7, 6, 6, 7, 7, 7, 4], priorities: [6, 7, 6, 7, 6, 7, 4, 6, 7, 5] },
    UncivPersonality { leader: "Catherine", victory: Victory::Scientific, values: [6, 3, 5, 8, 6, 3, 6, 5, 4, 4, 7, 7, 6, 8, 6], priorities: [6, 8, 3, 6, 7, 8, 10, 2, 3, 6] },
    UncivPersonality { leader: "Darius I", victory: Victory::Scientific, values: [6, 6, 7, 5, 5, 8, 6, 7, 3, 4, 5, 6, 8, 5, 4], priorities: [5, 6, 3, 7, 4, 7, 6, 8, 3, 7] },
    UncivPersonality { leader: "Dido", victory: Victory::Domination, values: [5, 5, 7, 5, 4, 5, 5, 6, 7, 7, 8, 5, 6, 7, 5], priorities: [3, 7, 7, 8, 5, 5, 5, 5, 6, 5] },
    UncivPersonality { leader: "Elizabeth", victory: Victory::Diplomatic, values: [6, 4, 8, 6, 6, 5, 5, 5, 4, 4, 6, 7, 5, 6, 6], priorities: [6, 6, 4, 8, 8, 5, 6, 5, 4, 6] },
    UncivPersonality { leader: "Enrico Dandolo", victory: Victory::Neutral, values: [5, 6, 8, 5, 5, 5, 7, 5, 5, 3, 8, 6, 3, 3, 6], priorities: [5, 3, 2, 9, 8, 7, 5, 5, 3, 5] },
    UncivPersonality { leader: "Gajah Mada", victory: Victory::Neutral, values: [5, 5, 6, 4, 6, 6, 7, 5, 4, 5, 4, 5, 4, 8, 6], priorities: [6, 8, 6, 6, 5, 7, 4, 6, 6, 6] },
    UncivPersonality { leader: "Gandhi", victory: Victory::Cultural, values: [5, 10, 4, 6, 8, 8, 8, 3, 5, 2, 6, 7, 7, 3, 6], priorities: [8, 3, 1, 4, 4, 9, 7, 10, 1, 5] },
    UncivPersonality { leader: "Genghis Khan", victory: Victory::Domination, values: [6, 5, 6, 4, 5, 5, 3, 6, 8, 7, 5, 4, 6, 8, 5], priorities: [4, 8, 9, 6, 2, 3, 4, 5, 8, 3] },
    UncivPersonality { leader: "Gustavus Adolphus", victory: Victory::Diplomatic, values: [6, 5, 6, 7, 5, 5, 4, 8, 4, 7, 6, 7, 6, 4, 5], priorities: [5, 4, 6, 6, 6, 4, 7, 5, 6, 6] },
    UncivPersonality { leader: "Haile Selassie", victory: Victory::Cultural, values: [6, 7, 6, 4, 7, 7, 7, 4, 2, 3, 4, 7, 7, 4, 5], priorities: [7, 4, 3, 6, 4, 7, 4, 8, 4, 8] },
    UncivPersonality { leader: "Harald Bluetooth", victory: Victory::Cultural, values: [5, 5, 7, 5, 5, 5, 5, 6, 7, 6, 5, 4, 7, 6, 5], priorities: [4, 6, 7, 7, 5, 5, 5, 5, 7, 4] },
    UncivPersonality { leader: "Harun al-Rashid", victory: Victory::Scientific, values: [5, 5, 8, 7, 6, 7, 8, 5, 4, 3, 6, 5, 6, 5, 3], priorities: [6, 5, 2, 8, 4, 9, 8, 7, 3, 6] },
    UncivPersonality { leader: "Hiawatha", victory: Victory::Scientific, values: [5, 3, 6, 4, 5, 7, 7, 4, 3, 4, 3, 5, 8, 9, 4], priorities: [5, 9, 2, 5, 4, 8, 5, 6, 2, 7] },
    UncivPersonality { leader: "Isabella", victory: Victory::Domination, values: [5, 5, 7, 3, 5, 5, 8, 5, 6, 6, 6, 3, 5, 7, 6], priorities: [5, 7, 5, 7, 3, 8, 3, 5, 5, 4] },
    UncivPersonality { leader: "Kamehameha I", victory: Victory::Cultural, values: [5, 3, 5, 4, 6, 8, 5, 5, 3, 4, 6, 5, 8, 8, 7], priorities: [5, 8, 5, 5, 6, 5, 4, 7, 5, 7] },
    UncivPersonality { leader: "Maria Theresa", victory: Victory::Diplomatic, values: [5, 4, 7, 5, 5, 6, 5, 4, 5, 5, 8, 5, 5, 7, 7], priorities: [5, 7, 4, 8, 7, 5, 5, 6, 4, 5] },
    UncivPersonality { leader: "Montezuma I", victory: Victory::Domination, values: [5, 5, 5, 4, 6, 6, 7, 4, 9, 8, 4, 5, 3, 8, 6], priorities: [6, 8, 10, 5, 4, 8, 5, 6, 9, 3] },
    UncivPersonality { leader: "Napoleon", victory: Victory::Cultural, values: [5, 5, 4, 7, 8, 4, 3, 6, 6, 8, 6, 3, 2, 8, 3], priorities: [8, 8, 10, 4, 7, 3, 7, 4, 9, 3] },
    UncivPersonality { leader: "Nebuchadnezzar II", victory: Victory::Scientific, values: [5, 6, 5, 8, 5, 5, 5, 4, 4, 6, 6, 6, 4, 4, 6], priorities: [6, 5, 5, 5, 5, 5, 8, 5, 5, 6] },
    UncivPersonality { leader: "Oda Nobunaga", victory: Victory::Domination, values: [5, 5, 6, 4, 6, 5, 4, 8, 8, 7, 3, 7, 3, 8, 7], priorities: [6, 8, 9, 5, 6, 5, 5, 5, 9, 5] },
    UncivPersonality { leader: "Pacal", victory: Victory::Scientific, values: [5, 5, 5, 8, 6, 6, 8, 5, 3, 4, 3, 6, 7, 4, 4], priorities: [7, 4, 3, 4, 4, 8, 8, 6, 3, 7] },
    UncivPersonality { leader: "Pachacuti", victory: Victory::Cultural, values: [5, 5, 8, 5, 6, 4, 6, 6, 6, 5, 8, 4, 5, 6, 5], priorities: [7, 6, 4, 9, 3, 6, 5, 4, 4, 5] },
    UncivPersonality { leader: "Pedro II", victory: Victory::Neutral, values: [6, 7, 6, 5, 7, 8, 5, 3, 4, 4, 5, 6, 7, 5, 6], priorities: [7, 6, 4, 6, 5, 5, 5, 9, 3, 6] },
    UncivPersonality { leader: "Pocatello", victory: Victory::Neutral, values: [5, 8, 5, 5, 5, 5, 5, 5, 2, 5, 5, 5, 6, 8, 5], priorities: [5, 8, 5, 5, 5, 5, 5, 6, 5, 8] },
    UncivPersonality { leader: "Ramesses II", victory: Victory::Cultural, values: [7, 6, 6, 5, 7, 6, 7, 3, 4, 3, 4, 5, 4, 5, 4], priorities: [9, 6, 4, 6, 5, 8, 6, 6, 4, 7] },
    UncivPersonality { leader: "Ramkhamhaeng", victory: Victory::Diplomatic, values: [5, 6, 5, 5, 8, 6, 6, 5, 2, 5, 6, 8, 7, 6, 5], priorities: [8, 6, 5, 5, 5, 7, 6, 6, 5, 8] },
    UncivPersonality { leader: "Sejong", victory: Victory::Scientific, values: [5, 6, 5, 8, 5, 5, 5, 4, 3, 4, 4, 6, 8, 5, 4], priorities: [5, 5, 4, 5, 4, 6, 9, 5, 4, 7] },
    UncivPersonality { leader: "Shaka", victory: Victory::Neutral, values: [8, 3, 6, 3, 3, 7, 3, 7, 8, 8, 5, 3, 8, 11, 8], priorities: [2, 11, 9, 6, 5, 2, 2, 6, 9, 5] },
    UncivPersonality { leader: "Suleiman I", victory: Victory::Domination, values: [5, 4, 5, 6, 6, 4, 5, 6, 7, 5, 6, 7, 5, 8, 7], priorities: [6, 8, 8, 5, 6, 6, 7, 4, 8, 4] },
    UncivPersonality { leader: "Theodora", victory: Victory::Cultural, values: [5, 7, 5, 6, 7, 5, 7, 5, 2, 4, 8, 7, 5, 4, 5], priorities: [7, 4, 4, 6, 6, 7, 6, 6, 4, 8] },
    UncivPersonality { leader: "George Washington", victory: Victory::Scientific, values: [5, 3, 6, 6, 6, 7, 4, 5, 2, 4, 5, 7, 7, 7, 4], priorities: [6, 7, 7, 6, 6, 5, 7, 6, 7, 8] },
    UncivPersonality { leader: "William of Orange", victory: Victory::Cultural, values: [5, 3, 7, 5, 6, 8, 5, 4, 5, 4, 5, 6, 7, 6, 5], priorities: [6, 7, 4, 7, 7, 5, 5, 7, 4, 5] },
    UncivPersonality { leader: "Wu Zetian", victory: Victory::Scientific, values: [5, 8, 5, 8, 6, 6, 5, 4, 3, 4, 6, 7, 4, 6, 7], priorities: [6, 6, 7, 5, 7, 6, 9, 7, 7, 7] },
];
