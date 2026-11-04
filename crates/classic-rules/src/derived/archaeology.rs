//! Archaeology rules (read-only): classic/docs/classic/rules.md "Culture, trade,
//! diplomacy and victories" 2.5–2.6. Which content plays the antiquity site,
//! the dig and the Landmark; where sites go (tile weights); what an
//! archaeologist may dig and what its player may take from a finished dig.

use crate::derived::tourism::free_slot;
use crate::ids::{ImprovementId, PlayerId, PolicyId, ResourceId, TechId, TileId, UnitHandle};
use crate::rules::content::{GreatWorkSlot, ResourceClass, UnitAbility};
use crate::rules::roles::{Climate, FeatureRole, Ground};
use crate::rules::CompiledRuleset;
use crate::world::state::AuthWorld;
use crate::world::tourism::{ArchRecord, ArtifactKind, DigSite, PendingDig};

// ---- tuning: archaeology (own design; Unciv has no archaeology) ----

/// Dig sites per major nation: (fewest, most). Own design: a game places about five per nation.
pub const SITES_PER_NATION: (u32, u32) = (4, 6);
/// Percent of the sites that start hidden until a policy reveals them. Own design.
pub const PERCENT_HIDDEN: u64 = 25;
/// Percent of the hidden sites that hold a writing. Own design.
pub const PERCENT_HIDDEN_WRITING: u32 = 40;
/// Most sites of the game that hold a writing, whatever the number of nations. Own design.
pub const ARCHAEOLOGY_WRITINGS: u32 = 10;
/// Turns of culture a writing site's culture boost gives: the whole recent-culture window
/// ([`crate::world::culture::RECENT_TURNS`], Unciv's 8 turns).
pub const BOOST_TURNS: usize = crate::world::culture::RECENT_TURNS;
/// Weight of a tile without any history in the choice of the sites. Own design.
pub const SITE_BASE_WEIGHT: u32 = 6;
/// Weight a history record adds per rank of its kind (battle < camp < ruin < razed city < writing). Own design.
pub const SITE_WEIGHT_PER_RANK: u32 = 5;
/// Weight a history record adds per era it lies back from the seventh: old history draws sites. Own design.
pub const SITE_WEIGHT_PER_OLD_ERA: u32 = 2;
/// Percent of the weight of the terrains where ruins survive best (desert and jungle), 150 %. Own design.
pub const SITE_TERRAIN_PERCENT: u32 = 150;
/// Percent of the weight of land inside someone's borders: sites lie a little more often in the wild. Own design.
pub const SITE_OWNED_PERCENT: u32 = 85;
/// Percent of the weight of an improved or road tile (building would have found the ruin). Own design.
pub const SITE_IMPROVED_PERCENT: u32 = 60;
/// Percent of the weight of a city tile. Own design.
pub const SITE_CITY_PERCENT: u32 = 10;
/// Radius around a chosen site where other sites are made less likely; the penalty is `radius + 1 - distance`
/// (4 next to the site, 1 at the edge). Own design.
pub const SITE_CROWD_RADIUS: i32 = 4;

/// What the player takes from a finished dig.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArchaeologyChoice {
    Landmark,
    /// An artifact by the record's first player.
    ArtifactFirst,
    /// An artifact by the record's second player.
    ArtifactSecond,
    /// The site's writing as a great work of literature.
    Writing,
    /// The culture of the last turns (writing sites).
    CultureBoost,
}

impl ArchaeologyChoice {
    pub const ALL: [ArchaeologyChoice; 5] = [
        ArchaeologyChoice::Landmark,
        ArchaeologyChoice::ArtifactFirst,
        ArchaeologyChoice::ArtifactSecond,
        ArchaeologyChoice::Writing,
        ArchaeologyChoice::CultureBoost,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DigBlock {
    UnknownUnit,
    NotAnArchaeologist,
    NoSite,
    /// A hidden site needs the policy that reveals it.
    HiddenSite,
    AlreadyDigging,
    /// The dig improvement does not ignore ownership and the land is
    /// somebody else's.
    NotYourLand,
    NoPendingDig,
    ChoiceUnavailable,
    NoFreeSlot,
}

impl DigBlock {
    pub const ALL: [DigBlock; 9] = [
        DigBlock::UnknownUnit,
        DigBlock::NotAnArchaeologist,
        DigBlock::NoSite,
        DigBlock::HiddenSite,
        DigBlock::AlreadyDigging,
        DigBlock::NotYourLand,
        DigBlock::NoPendingDig,
        DigBlock::ChoiceUnavailable,
        DigBlock::NoFreeSlot,
    ];

    pub fn reason(self) -> &'static str {
        match self {
            DigBlock::UnknownUnit => "unknown unit",
            DigBlock::NotAnArchaeologist => "only an archaeologist digs",
            DigBlock::NoSite => "no antiquity site here",
            DigBlock::HiddenSite => "the site is hidden from you",
            DigBlock::AlreadyDigging => "someone digs here already",
            DigBlock::NotYourLand => "not your land",
            DigBlock::NoPendingDig => "no finished dig here",
            DigBlock::ChoiceUnavailable => "this site does not offer that",
            DigBlock::NoFreeSlot => "no free great work slot of this kind",
        }
    }
}

// ---- content ----

/// The visible and the hidden antiquity site resources (class Artifact;
/// the visible one has a reveal technology).
pub fn site_resources(rules: &CompiledRuleset) -> (Option<ResourceId>, Option<ResourceId>) {
    let artifact = |want_tech: bool| {
        rules
            .resources
            .iter()
            .position(|r| {
                r.class == Some(ResourceClass::Artifact) && r.required_tech.is_some() == want_tech
            })
            .map(|i| ResourceId(i as u16))
    };
    (artifact(true), artifact(false))
}

/// The technology whose first discovery makes the sites appear
/// (the technology flagged `triggers_archaeological_sites`: Archaeology).
pub fn trigger_tech(rules: &CompiledRuleset) -> Option<TechId> {
    let i = rules
        .technologies
        .iter()
        .position(|t| t.module.triggers_archaeological_sites)?;
    Some(TechId(i as u16))
}

/// The improvement a dig leaves as a Landmark (culture per era).
pub fn landmark(rules: &CompiledRuleset) -> Option<ImprovementId> {
    let i = rules
        .improvements
        .iter()
        .position(|d| d.culture_per_era > 0)?;
    Some(ImprovementId(i as u16))
}

/// The improvement an archaeologist builds on a site (the visible site's
/// connecting improvement: the archaeological dig).
pub fn dig_improvement(rules: &CompiledRuleset) -> Option<ImprovementId> {
    let (visible, _) = site_resources(rules);
    rules.resources[visible?.0 as usize].improvements.first().copied()
}

/// Turns of a dig: the dig improvement's work time at the archaeologist's
/// work rate 100 (600 → 6).
pub fn dig_turns(rules: &CompiledRuleset) -> u16 {
    let time = dig_improvement(rules)
        .map(|d| rules.improvements[d.0 as usize].work_time)
        .unwrap_or(600);
    (time / 100).max(1) as u16
}

/// The Landmark may stand on `tile` now: over the finished dig when it
/// requires an improvement (`placement.requires_improvement`).
fn landmark_fits(auth: &AuthWorld, rules: &CompiledRuleset, tile: TileId) -> bool {
    let Some(landmark) = landmark(rules) else {
        return false;
    };
    let needs_improvement = rules.improvements[landmark.0 as usize].placement.requires_improvement;
    !needs_improvement || auth.map().improvement(tile).flatten().is_some()
}

/// The policy that reveals hidden sites (the hidden site resource's
/// `revealed_by_policy`: Exploration's finisher).
pub fn hidden_reveal_policy(rules: &CompiledRuleset) -> Option<PolicyId> {
    let (_, hidden) = site_resources(rules);
    rules.resources[hidden?.0 as usize].module.revealed_by_policy
}

/// The player sees hidden sites.
pub fn sees_hidden(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> bool {
    hidden_reveal_policy(rules).is_some_and(|p| auth.module().culture().of(player).has_policy(p))
}

// ---- digging ----

/// The archaeologist may start digging the site it stands on.
pub fn excavate_status(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    unit: UnitHandle,
) -> Result<DigSite, DigBlock> {
    let core = auth
        .units()
        .get(unit)
        .map(|v| *v.core)
        .ok_or(DigBlock::UnknownUnit)?;
    if !rules.units[core.kind.0 as usize]
        .module.abilities
        .contains(&UnitAbility::Excavates)
    {
        return Err(DigBlock::NotAnArchaeologist);
    }
    let archaeology = auth.module().tourism().archaeology();
    let site = *archaeology.site_at(core.tile).ok_or(DigBlock::NoSite)?;
    if site.hidden && !sees_hidden(auth, rules, core.owner) {
        return Err(DigBlock::HiddenSite);
    }
    if archaeology.digs.iter().any(|d| d.tile == core.tile) {
        return Err(DigBlock::AlreadyDigging);
    }
    let ignores_ownership = dig_improvement(rules)
        .is_some_and(|d| rules.improvements[d.0 as usize].placement.ignore_ownership);
    let owner = auth.map().owner(core.tile).flatten();
    if !ignores_ownership && owner.is_some_and(|o| o != core.owner) {
        return Err(DigBlock::NotYourLand);
    }
    Ok(site)
}

/// The finished dig of `player` on `tile`.
pub fn pending_dig(auth: &AuthWorld, player: PlayerId, tile: TileId) -> Option<PendingDig> {
    let archaeology = auth.module().tourism().archaeology();
    archaeology
        .pending
        .iter()
        .find(|p| p.player == player && p.site.tile == tile)
        .copied()
}

/// The choice is open for the finished dig (rules.md table).
pub fn choice_status(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
    tile: TileId,
    choice: ArchaeologyChoice,
) -> Result<PendingDig, DigBlock> {
    let pending = pending_dig(auth, player, tile).ok_or(DigBlock::NoPendingDig)?;
    let record = pending.site.record;
    let writing = record.kind == ArtifactKind::Writing;
    let slot = |s: GreatWorkSlot| free_slot(auth, rules, player, s, tile).is_some();
    let open = match choice {
        ArchaeologyChoice::Landmark => !writing && landmark_fits(auth, rules, tile),
        ArchaeologyChoice::ArtifactFirst => !writing,
        ArchaeologyChoice::ArtifactSecond => !writing && record.second.is_some(),
        ArchaeologyChoice::Writing | ArchaeologyChoice::CultureBoost => writing,
    };
    if !open {
        return Err(DigBlock::ChoiceUnavailable);
    }
    let needs = match choice {
        ArchaeologyChoice::ArtifactFirst | ArchaeologyChoice::ArtifactSecond => {
            Some(GreatWorkSlot::ArtArtifact)
        }
        ArchaeologyChoice::Writing => Some(GreatWorkSlot::Literature),
        _ => None,
    };
    if needs.is_some_and(|s| !slot(s)) {
        return Err(DigBlock::NoFreeSlot);
    }
    Ok(pending)
}

// ---- where sites go ----

/// Weight of a tile before randomness and crowding: [`SITE_BASE_WEIGHT`] plus what its history record
/// adds; None when no site may go there.
pub fn site_base_weight(auth: &AuthWorld, rules: &CompiledRuleset, tile: TileId) -> Option<u32> {
    let map = auth.map();
    let terrain = map.terrain(tile)?;
    let def = rules
        .terrains
        .iter()
        .find(|t| t.map_code == Some(terrain.0))?;
    let blocked = def.water
        || def.impassable
        || map.resource(tile).flatten().is_some()
        || map.natural_wonder(tile).is_some()
        || feature_role(auth, rules, tile) == Some(FeatureRole::Ice)
        || great_person_improvement(auth, rules, tile);
    if blocked {
        return None;
    }
    Some(match auth.module().tourism().archaeology().record_at(tile) {
        Some(r) => {
            SITE_BASE_WEIGHT
                + SITE_WEIGHT_PER_RANK * u32::from(r.kind.value())
                + SITE_WEIGHT_PER_OLD_ERA * 7u32.saturating_sub(u32::from(r.era))
        }
        None => SITE_BASE_WEIGHT,
    })
}

/// The full weight of a tile with the random part already drawn (`noise` in 0..10), before
/// crowding: `base + noise`, 150 % on desert and jungle, 85 % inside borders, 60 % when improved or
/// on a road, 10 % under a city. Own design.
pub fn site_weight(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    tile: TileId,
    base: u32,
    noise: u32,
) -> u32 {
    let map = auth.map();
    let mut w = base + noise;
    let dry_or_wet = ground(auth, rules, tile) == Some(Ground::Flat(Climate::Arid))
        || feature_role(auth, rules, tile) == Some(FeatureRole::Jungle);
    if dry_or_wet {
        w = w * SITE_TERRAIN_PERCENT / 100;
    }
    if map.owner(tile).flatten().is_some() {
        w = w * SITE_OWNED_PERCENT / 100;
    }
    if map.improvement(tile).flatten().is_some() || map.road(tile) {
        w = w * SITE_IMPROVED_PERCENT / 100;
    }
    if auth.indexes().city_on_tile(tile).is_some() {
        w = w * SITE_CITY_PERCENT / 100;
    }
    w
}

/// The ground the tile's terrain row plays (None: no row for its code).
fn ground(auth: &AuthWorld, rules: &CompiledRuleset, tile: TileId) -> Option<Ground> {
    let kind = rules.terrain_kind_of(auth.map().terrain(tile)?)?;
    rules.roles.ground_of(kind)
}

fn feature_role(auth: &AuthWorld, rules: &CompiledRuleset, tile: TileId) -> Option<FeatureRole> {
    rules.roles.feature_role(auth.map().feature(tile).flatten()?)
}

fn great_person_improvement(auth: &AuthWorld, rules: &CompiledRuleset, tile: TileId) -> bool {
    auth.map()
        .improvement(tile)
        .flatten()
        .is_some_and(|i| rules.improvements[i.0 as usize].module.created_by_great_person)
}

/// How many sites the game places for `majors` nations, with two
/// random draws in `0..spread`.
pub fn ideal_sites(majors: u32, draws: (u32, u32)) -> u32 {
    SITES_PER_NATION.0 * majors + draws.0 + draws.1
}

/// The range of each of the two draws of `ideal_sites`: (max − min) / 2.
pub fn site_draw_range(majors: u32) -> u32 {
    (SITES_PER_NATION.1 - SITES_PER_NATION.0) * majors / 2
}

/// Hidden sites that hold a writing: min(10, ideal × 25 % × 40 %).
pub fn writing_sites(ideal: u32) -> u32 {
    ARCHAEOLOGY_WRITINGS.min(ideal * PERCENT_HIDDEN as u32 / 100 * PERCENT_HIDDEN_WRITING / 100)
}

/// A history record invented for a site without one: era by weight (`highest - i`, the oldest eras most likely),
/// kind drawn from this table (ruins and barbarian camps twice in seven, the others once). Own design.
pub const INVENTED_KINDS: [ArtifactKind; 7] = [
    ArtifactKind::AncientRuin,
    ArtifactKind::AncientRuin,
    ArtifactKind::RazedCity,
    ArtifactKind::BarbarianCamp,
    ArtifactKind::BarbarianCamp,
    ArtifactKind::BattleMelee,
    ArtifactKind::BattleRanged,
];

/// The record keeps a battle, a razed city or a ruin (records of a lower
/// rank are replaced by a higher one).
pub fn replaces(old: Option<&ArchRecord>, kind: ArtifactKind) -> bool {
    old.is_none_or(|o| kind > o.kind)
}

/// Sites the player sees (hidden ones with the reveal policy) that nobody
/// digs yet.
pub fn open_sites(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> Vec<TileId> {
    let archaeology = auth.module().tourism().archaeology();
    let hidden_ok = sees_hidden(auth, rules, player);
    archaeology
        .sites
        .iter()
        .filter(|s| !s.hidden || hidden_ok)
        .filter(|s| !archaeology.digs.iter().any(|d| d.tile == s.tile))
        .map(|s| s.tile)
        .collect()
}

/// The player's finished digs with the choices open now.
pub fn pending_choices(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    player: PlayerId,
) -> Vec<(TileId, Vec<ArchaeologyChoice>)> {
    auth.module().tourism()
        .archaeology()
        .pending
        .iter()
        .filter(|p| p.player == player)
        .map(|p| {
            let tile = p.site.tile;
            let open = ArchaeologyChoice::ALL
                .into_iter()
                .filter(|c| choice_status(auth, rules, player, tile, *c).is_ok())
                .collect();
            (tile, open)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_game_places_about_five_sites_per_nation() {
        for majors in [2u32, 4, 6, 8, 12] {
            let range = site_draw_range(majors);
            let fewest = ideal_sites(majors, (0, 0));
            let most = ideal_sites(majors, (range - 1, range - 1));
            assert_eq!(fewest, 4 * majors);
            assert!(most <= 6 * majors, "{majors} nations: at most {most}");
            // The mean of two uniform draws is the middle: 5 per nation.
            let mean = (fewest + most) / 2;
            assert!(mean.abs_diff(5 * majors) <= majors / 2 + 1, "{majors} nations: mean {mean}");
        }
    }

    #[test]
    fn writings_are_a_tenth_of_the_sites_up_to_ten() {
        assert_eq!(writing_sites(20), 2);
        assert_eq!(writing_sites(40), 4);
        assert_eq!(writing_sites(100), 10, "capped at ten");
        assert_eq!(writing_sites(5), 0);
    }

    #[test]
    fn the_site_weights_are_ordered_by_their_history() {
        // A record's weight: 6 + 5 per rank + 2 per era back from the seventh.
        let weight = |rank: u32, era: u32| SITE_BASE_WEIGHT + SITE_WEIGHT_PER_RANK * rank + SITE_WEIGHT_PER_OLD_ERA * 7u32.saturating_sub(era);
        assert!(weight(5, 0) > weight(5, 6), "old history draws sites");
        assert!(weight(5, 3) > weight(1, 3), "a razed city beats a skirmish");
        assert!(weight(1, 7) > SITE_BASE_WEIGHT, "any record beats none");
        assert_eq!(weight(6, 0), 6 + 30 + 14);
        // The percents shrink the weight monotonically.
        assert!(SITE_CITY_PERCENT < SITE_IMPROVED_PERCENT && SITE_IMPROVED_PERCENT < SITE_OWNED_PERCENT);
        assert!(SITE_OWNED_PERCENT < 100 && SITE_TERRAIN_PERCENT > 100);
    }

    #[test]
    fn records_of_a_higher_rank_replace_lower_ones() {
        let record = |kind| ArchRecord { kind, era: 1, first: PlayerId(0), second: None };
        assert!(replaces(None, ArtifactKind::BattleRanged));
        assert!(replaces(Some(&record(ArtifactKind::BattleRanged)), ArtifactKind::RazedCity));
        assert!(!replaces(Some(&record(ArtifactKind::RazedCity)), ArtifactKind::BattleMelee));
    }
}
