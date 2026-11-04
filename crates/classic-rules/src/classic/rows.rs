//! The classic fields on the kernel's rows (`kernel::rules::rows`): faith
//! prices, great works and theming, policy and religion requirements,
//! happiness, great person points, the unit and building abilities the
//! module's systems read. Each hashes at the place the field held in the
//! row before the split, so the ruleset fingerprint keeps its bytes.

use std::hash::{Hash, Hasher};

use crate::classic::Classic;
use crate::ids::{ImprovementId, PolicyId};
use crate::rules::compile::{improvements, RulesError};
use crate::rules::compiled::CompiledRuleset;
use crate::rules::content::{
    BuildingAbility, ContentTable, GreatPersonId, GreatWorkSlot, PolicyBranchId, ReligionId,
    ThemingBonus, UnitAbilities, UnitAbility,
};
use crate::rules::key_index::KeyIndex;
use crate::rules::rows::{
    BuildingExtension, BuildingPlace, NationPlace, EraPlace, HandicapExtension,
    HandicapPlace, ImprovementPlace, ModuleRows, NaturalWonderPlace, ResourcePlace, RowExtension,
    SpecialistExtension, SpecialistPlace, TechnologyPlace, UnitExtension, UnitPlace,
};
use crate::rules::source::{KeyCount, StableContentKey};

/// The classic extension of every kernel table.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClassicRows;

impl ModuleRows<Classic> for ClassicRows {
    type Unit = UnitFields;
    type Building = BuildingFields;
    type Technology = TechnologyFields;
    type Improvement = ImprovementFields;
    type Resource = ResourceFields;
    type Nation = NationFields;
    type Era = EraFields;
    type NaturalWonder = NaturalWonderFields;
    type Specialist = SpecialistFields;
    type Handicap = HandicapFields;
}

// ---- units ----

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnitFields {
    pub faith_cost: Option<u32>,
    pub abilities: Vec<UnitAbility>,
    /// Trained or bought only with this policy adopted (Landsknecht,
    /// Mercenary Army).
    pub required_policy: Option<PolicyId>,
    /// Improvements a combat unit builds (Legion: fort; Samurai: fishing
    /// boats) and whether it builds roads (Legion).
    pub builds: Vec<ImprovementId>,
    pub builds_roads: bool,
    /// The abilities the economy and society systems read (from `abilities`).
    pub economy: UnitAbilities,
}

#[derive(Debug, Clone, Default)]
pub struct SourceUnitFields {
    pub faith_cost: Option<u32>,
    pub abilities: Vec<UnitAbility>,
    pub required_policy: Option<StableContentKey>,
    pub builds: Vec<StableContentKey>,
    pub builds_roads: bool,
}

impl RowExtension<Classic, UnitPlace> for UnitFields {
    type Source = SourceUnitFields;

    fn compile(source: &SourceUnitFields, _: &str, ix: &KeyIndex, _: &CompiledRuleset) -> Result<Self, RulesError> {
        Ok(UnitFields {
            faith_cost: source.faith_cost,
            abilities: source.abilities.clone(),
            required_policy: ix.opt_id(crate::classic::tables::POLICIES, &source.required_policy)?.map(PolicyId),
            builds: improvements(ix, &source.builds)?,
            builds_roads: source.builds_roads,
            economy: UnitAbilities::of(&source.abilities),
        })
    }

    fn hash_at<H: Hasher>(&self, place: UnitPlace, h: &mut H) {
        match place {
            UnitPlace::AfterSight => self.faith_cost.hash(h),
            UnitPlace::AfterSpaceshipProject => {
                self.abilities.hash(h);
                self.economy.hash(h);
                // Only a unit with a policy or builds of its own writes
                // them: the rest keep the bytes they had.
                if self.required_policy.is_some() || !self.builds.is_empty() || self.builds_roads {
                    self.required_policy.hash(h);
                    self.builds.hash(h);
                    self.builds_roads.hash(h);
                }
            }
        }
    }
}

impl UnitExtension<Classic> for UnitFields {
    fn built_with_food(&self) -> bool {
        self.economy.food
    }

    fn purchase_only(&self) -> bool {
        self.abilities.contains(&UnitAbility::PurchaseOnly)
    }

    fn builds(&self) -> &[ImprovementId] {
        &self.builds
    }

    fn builds_roads(&self) -> bool {
        self.builds_roads
    }
}

// ---- buildings ----

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildingFields {
    pub faith_cost: Option<u32>,
    /// Great work slot type and count.
    pub great_work_slots: Option<(GreatWorkSlot, u16)>,
    /// Theming bonuses in checking order.
    pub theming: Vec<ThemingBonus>,
    /// The player must have unlocked this policy branch.
    pub requires_policy_branch: Option<PolicyBranchId>,
    /// Only a holy city may build it.
    pub requires_holy_city: bool,
    /// Its faith price is open only to a city whose majority religion has a
    /// belief unlocking it (`UnlockFaithBuilding`).
    pub faith_purchase_needs_belief: bool,
    pub abilities: Vec<BuildingAbility>,
}

#[derive(Debug, Clone, Default)]
pub struct SourceBuildingFields {
    pub faith_cost: Option<u32>,
    /// Great work slots: (`core:great-work-slot/<slug>`, count).
    pub great_work_slots: Option<KeyCount>,
    /// Theming bonuses in checking order.
    pub theming: Vec<ThemingBonus>,
    /// The player must have unlocked this policy branch (some wonders).
    pub requires_policy_branch: Option<StableContentKey>,
    /// Only a holy city may build it.
    pub requires_holy_city: bool,
    /// Its faith price is open only to a city whose majority religion has
    /// a belief unlocking it.
    pub faith_purchase_needs_belief: bool,
    pub abilities: Vec<BuildingAbility>,
}

impl RowExtension<Classic, BuildingPlace> for BuildingFields {
    type Source = SourceBuildingFields;

    fn compile(source: &SourceBuildingFields, key: &str, ix: &KeyIndex, _: &CompiledRuleset) -> Result<Self, RulesError> {
        let branch = ix.opt_id(crate::classic::tables::POLICY_BRANCHES, &source.requires_policy_branch)?;
        Ok(BuildingFields {
            faith_cost: source.faith_cost,
            great_work_slots: great_work_slots(key, ix, source.great_work_slots.as_ref())?,
            theming: source.theming.clone(),
            requires_policy_branch: branch.map(PolicyBranchId),
            requires_holy_city: source.requires_holy_city,
            faith_purchase_needs_belief: source.faith_purchase_needs_belief,
            abilities: source.abilities.clone(),
        })
    }

    fn hash_at<H: Hasher>(&self, place: BuildingPlace, h: &mut H) {
        match place {
            BuildingPlace::AfterClass => self.faith_cost.hash(h),
            BuildingPlace::AfterSpecialistSlots => {
                self.great_work_slots.hash(h);
                self.theming.hash(h);
                self.requires_policy_branch.hash(h);
                self.requires_holy_city.hash(h);
                self.faith_purchase_needs_belief.hash(h);
            }
            BuildingPlace::AfterReplaces => self.abilities.hash(h),
        }
    }
}

impl BuildingExtension<Classic> for BuildingFields {
    fn border_obstacle(&self) -> bool {
        self.abilities.contains(&BuildingAbility::BorderObstacle)
    }
}

/// The great work slots of a building: a known slot kind.
fn great_work_slots(key: &str, ix: &KeyIndex, slots: Option<&KeyCount>) -> Result<Option<(GreatWorkSlot, u16)>, RulesError> {
    let Some(slots) = slots else {
        return Ok(None);
    };
    ix.any(&slots.key.0)?;
    let Some(slot) = GreatWorkSlot::from_key(&slots.key.0) else {
        let reason = format!("{key}: unknown great work slot {}", slots.key.0);
        return Err(RulesError::Inconsistent(reason));
    };
    Ok(Some((slot, slots.count)))
}

// ---- technologies ----

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TechnologyFields {
    /// The first nation to know it founds the World Congress.
    pub allows_world_congress: bool,
    /// The first learning in the game makes the antiquity sites appear
    /// (Archaeology).
    pub triggers_archaeological_sites: bool,
}

impl RowExtension<Classic, TechnologyPlace> for TechnologyFields {
    type Source = TechnologyFields;

    fn compile(source: &TechnologyFields, _: &str, _: &KeyIndex, _: &CompiledRuleset) -> Result<Self, RulesError> {
        Ok(source.clone())
    }

    fn hash_at<H: Hasher>(&self, place: TechnologyPlace, h: &mut H) {
        match place {
            TechnologyPlace::AfterResearchAgreement => {
                self.allows_world_congress.hash(h);
                self.triggers_archaeological_sites.hash(h);
            }
        }
    }
}

// ---- improvements ----

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImprovementFields {
    pub created_by_great_person: bool,
    /// Copies of each luxury of the city-state whose land it stands on that
    /// the improvement's nation gets (Feitoria).
    pub city_state_luxury_copies: u8,
}

impl RowExtension<Classic, ImprovementPlace> for ImprovementFields {
    type Source = ImprovementFields;

    fn compile(source: &ImprovementFields, _: &str, _: &KeyIndex, _: &CompiledRuleset) -> Result<Self, RulesError> {
        Ok(source.clone())
    }

    fn hash_at<H: Hasher>(&self, place: ImprovementPlace, h: &mut H) {
        match place {
            ImprovementPlace::AfterNation => self.created_by_great_person.hash(h),
            ImprovementPlace::AfterAdjacentSameCulture => self.city_state_luxury_copies.hash(h),
        }
    }
}

// ---- resources ----

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResourceFields {
    pub happiness: i32,
    pub only_city_states: bool,
    /// The policy that lets a player see it (hidden antiquity sites,
    /// Exploration's finisher).
    pub revealed_by_policy: Option<PolicyId>,
}

#[derive(Debug, Clone, Default)]
pub struct SourceResourceFields {
    pub happiness: i32,
    pub only_city_states: bool,
    pub revealed_by_policy: Option<StableContentKey>,
}

impl RowExtension<Classic, ResourcePlace> for ResourceFields {
    type Source = SourceResourceFields;

    fn compile(source: &SourceResourceFields, _: &str, ix: &KeyIndex, _: &CompiledRuleset) -> Result<Self, RulesError> {
        let policy = ix.opt_id(crate::classic::tables::POLICIES, &source.revealed_by_policy)?;
        Ok(ResourceFields {
            happiness: source.happiness,
            only_city_states: source.only_city_states,
            revealed_by_policy: policy.map(PolicyId),
        })
    }

    fn hash_at<H: Hasher>(&self, place: ResourcePlace, h: &mut H) {
        match place {
            ResourcePlace::AfterYields => self.happiness.hash(h),
            ResourcePlace::AfterInStandardGame => self.only_city_states.hash(h),
            ResourcePlace::AfterImprovedYields => self.revealed_by_policy.hash(h),
        }
    }
}

// ---- nations ----

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NationFields {
    pub preferred_religions: Vec<ReligionId>,
    /// The names its spies take in turn (`Spy::name` counts through them); display only, so
    /// the ruleset fingerprint does not cover them.
    pub spy_names: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SourceNationFields {
    pub preferred_religions: Vec<StableContentKey>,
    pub spy_names: Vec<String>,
}

impl RowExtension<Classic, NationPlace> for NationFields {
    type Source = SourceNationFields;

    fn compile(source: &SourceNationFields, _: &str, ix: &KeyIndex, _: &CompiledRuleset) -> Result<Self, RulesError> {
        let religions = ix.ids(crate::classic::tables::RELIGIONS, &source.preferred_religions)?;
        Ok(NationFields {
            preferred_religions: religions.into_iter().map(ReligionId).collect(),
            spy_names: source.spy_names.clone(),
        })
    }

    fn hash_at<H: Hasher>(&self, place: NationPlace, h: &mut H) {
        match place {
            NationPlace::AfterFreeUnits => self.preferred_religions.hash(h),
        }
    }
}

// ---- eras ----

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EraFields {
    pub faith_cost_multiplier: u32,
}

impl RowExtension<Classic, EraPlace> for EraFields {
    type Source = EraFields;

    fn compile(source: &EraFields, _: &str, _: &KeyIndex, _: &CompiledRuleset) -> Result<Self, RulesError> {
        Ok(source.clone())
    }

    fn hash_at<H: Hasher>(&self, place: EraPlace, h: &mut H) {
        match place {
            EraPlace::Last => self.faith_cost_multiplier.hash(h),
        }
    }
}

// ---- natural wonders ----

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NaturalWonderFields {
    pub happiness: i32,
}

impl RowExtension<Classic, NaturalWonderPlace> for NaturalWonderFields {
    type Source = NaturalWonderFields;

    fn compile(source: &NaturalWonderFields, _: &str, _: &KeyIndex, _: &CompiledRuleset) -> Result<Self, RulesError> {
        Ok(source.clone())
    }

    fn hash_at<H: Hasher>(&self, place: NaturalWonderPlace, h: &mut H) {
        match place {
            NaturalWonderPlace::AfterDefense => self.happiness.hash(h),
        }
    }
}

// ---- specialists ----

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpecialistFields {
    pub great_person_points: u16,
    pub great_person: Option<GreatPersonId>,
}

#[derive(Debug, Clone, Default)]
pub struct SourceSpecialistFields {
    pub great_person_points: u16,
    /// The great person unit its points produce.
    pub great_person: Option<StableContentKey>,
}

impl RowExtension<Classic, SpecialistPlace> for SpecialistFields {
    type Source = SourceSpecialistFields;

    /// The great person is named by its unit (the great people are compiled).
    fn compile(source: &SourceSpecialistFields, key: &str, ix: &KeyIndex, rules: &CompiledRuleset) -> Result<Self, RulesError> {
        let great_person = match &source.great_person {
            Some(unit_key) => {
                let unit = crate::ids::UnitKindId(ix.id(ContentTable::Unit, &unit_key.0)?);
                let Some(index) = rules.module.great_people.iter().position(|g| g.unit == unit) else {
                    let reason = format!("{key}: {} is not a great person", unit_key.0);
                    return Err(RulesError::Inconsistent(reason));
                };
                Some(GreatPersonId(index as u16))
            }
            None => None,
        };
        Ok(SpecialistFields {
            great_person_points: source.great_person_points,
            great_person,
        })
    }

    fn hash_at<H: Hasher>(&self, place: SpecialistPlace, h: &mut H) {
        match place {
            SpecialistPlace::Last => {
                self.great_person_points.hash(h);
                self.great_person.hash(h);
            }
        }
    }
}

impl SpecialistExtension<Classic> for SpecialistFields {
    fn points(&self) -> u16 {
        self.great_person_points
    }
}

// ---- difficulty levels ----

/// The classic numbers of a difficulty level: happiness and policy cost.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HandicapFields {
    pub happiness_default: i32,
    pub extra_happiness_per_luxury: i32,
    pub num_cities_unhappiness_mod: i32,
    pub population_unhappiness_mod: i32,
    pub policy_percent: i32,
    pub ai_unhappiness_percent: i32,
}

impl HandicapExtension for HandicapFields {
    /// No base happiness and no unhappiness; policies at full price.
    fn neutral() -> Self {
        HandicapFields {
            policy_percent: 100,
            ai_unhappiness_percent: 100,
            ..HandicapFields::default()
        }
    }

    /// The AI row's numbers; the unhappiness percent of the chosen difficulty level.
    fn for_ai(level: &Self, ai: &Self) -> Self {
        HandicapFields {
            ai_unhappiness_percent: level.ai_unhappiness_percent,
            ..*ai
        }
    }

    fn hash_at<H: Hasher>(&self, place: HandicapPlace, h: &mut H) {
        match place {
            HandicapPlace::First => {
                self.happiness_default.hash(h);
                self.extra_happiness_per_luxury.hash(h);
                self.num_cities_unhappiness_mod.hash(h);
                self.population_unhappiness_mod.hash(h);
            }
            HandicapPlace::AfterResearchPercent => {
                self.policy_percent.hash(h);
                self.ai_unhappiness_percent.hash(h);
            }
        }
    }
}
