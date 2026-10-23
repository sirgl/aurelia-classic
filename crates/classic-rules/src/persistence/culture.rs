//! Save schema of culture and policies (`world::culture`): plain values,
//! content ids as u16. Decoding checks every id against the rules and the
//! invariants the policies system keeps (no duplicate policy or branch, an
//! ideology is an unlocked ideology branch).

use crate::ids::{BuildingKindId, PlayerId, PolicyId};
use crate::persistence::DecodeError;
use crate::rules::content::{PolicyBranchId, PolicyBranchKind};
use crate::rules::CompiledRuleset;
use crate::world::culture::{CultureState, PlayerCulture, RECENT_TURNS};
use crate::world::state::AuthWorld;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedPlayerCulture {
    pub culture: u32,
    pub lifetime: u64,
    pub recent: Vec<u32>,
    pub policies: Vec<u16>,
    pub branches: Vec<u16>,
    pub ideology: Option<u16>,
    pub free_policies: u16,
    pub free_tenets: u16,
    pub free_used: u16,
    pub max_cities: u16,
    /// (building, cities still owed).
    pub free_buildings: Vec<(u16, u8)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedCulture {
    /// One per player, in player order.
    pub players: Vec<SavedPlayerCulture>,
    /// (ideology branch, adopters), first-adoption order.
    pub ideology_adopters: Vec<(u16, u8)>,
}

pub(crate) fn encode_culture(auth: &AuthWorld) -> SavedCulture {
    let players = (0..auth.players().count())
        .map(|i| encode_player(auth.module().culture().of(PlayerId(i as u8))))
        .collect();
    let ideology_adopters = auth
        .module()
        .culture()
        .ideology_adopters()
        .iter()
        .map(|(b, n)| (b.0, *n))
        .collect();
    SavedCulture {
        players,
        ideology_adopters,
    }
}

fn encode_player(p: &PlayerCulture) -> SavedPlayerCulture {
    SavedPlayerCulture {
        culture: p.culture,
        lifetime: p.lifetime,
        recent: p.recent.clone(),
        policies: p.policies.iter().map(|x| x.0).collect(),
        branches: p.branches.iter().map(|x| x.0).collect(),
        ideology: p.ideology.map(|x| x.0),
        free_policies: p.free_policies,
        free_tenets: p.free_tenets,
        free_used: p.free_used,
        max_cities: p.max_cities,
        free_buildings: p.free_buildings.iter().map(|(b, n)| (b.0, *n)).collect(),
    }
}

/// Untrusted data: every id resolves, lists are duplicate-free.
pub(crate) fn decode_culture(
    saved: &SavedCulture,
    rules: &CompiledRuleset,
    players: usize,
) -> Result<CultureState, DecodeError> {
    if saved.players.len() != players {
        return Err(corrupt("culture players"));
    }
    let mut out = Vec::with_capacity(players);
    for (i, p) in saved.players.iter().enumerate() {
        out.push(decode_player(p, rules).map_err(|what| corrupt(&format!("culture[{i}] {what}")))?);
    }
    let mut adopters = Vec::with_capacity(saved.ideology_adopters.len());
    for (branch, count) in &saved.ideology_adopters {
        let id = branch_id(*branch, rules).ok_or_else(|| corrupt("ideology adopters"))?;
        if adopters.iter().any(|(b, _)| *b == id) {
            return Err(corrupt("ideology adopters duplicate"));
        }
        adopters.push((id, *count));
    }
    Ok(CultureState::from_parts(out, adopters))
}

fn decode_player(
    p: &SavedPlayerCulture,
    rules: &CompiledRuleset,
) -> Result<PlayerCulture, &'static str> {
    if p.recent.len() > RECENT_TURNS {
        return Err("recent");
    }
    let mut policies = Vec::with_capacity(p.policies.len());
    for id in &p.policies {
        if (*id as usize) >= rules.module.policies.len() || policies.contains(&PolicyId(*id)) {
            return Err("policy");
        }
        policies.push(PolicyId(*id));
    }
    let mut branches = Vec::with_capacity(p.branches.len());
    for id in &p.branches {
        let branch = branch_id(*id, rules).ok_or("branch")?;
        if branches.contains(&branch) {
            return Err("branch duplicate");
        }
        branches.push(branch);
    }
    let ideology = match p.ideology {
        Some(id) => {
            let branch = branch_id(id, rules).ok_or("ideology")?;
            let is_ideology =
                rules.module.policy_branches[branch.0 as usize].kind == PolicyBranchKind::Ideology;
            if !is_ideology || !branches.contains(&branch) {
                return Err("ideology");
            }
            Some(branch)
        }
        None => None,
    };
    if usize::from(p.free_used) > policies.len() {
        return Err("free used");
    }
    let mut free_buildings = Vec::with_capacity(p.free_buildings.len());
    for (b, n) in &p.free_buildings {
        if (*b as usize) >= rules.buildings.len() || *n == 0 {
            return Err("free building");
        }
        free_buildings.push((BuildingKindId(*b), *n));
    }
    Ok(PlayerCulture {
        culture: p.culture,
        lifetime: p.lifetime,
        recent: p.recent.clone(),
        policies,
        branches,
        ideology,
        free_policies: p.free_policies,
        free_tenets: p.free_tenets,
        free_used: p.free_used,
        max_cities: p.max_cities,
        free_buildings,
    })
}

fn branch_id(id: u16, rules: &CompiledRuleset) -> Option<PolicyBranchId> {
    if (id as usize) < rules.module.policy_branches.len() {
        Some(PolicyBranchId(id))
    } else {
        None
    }
}

fn corrupt(what: &str) -> DecodeError {
    DecodeError::Corrupt(what.to_string())
}

/// Mix the culture state into the world hash (player order, then the
/// adopters). A player with an empty state mixes nothing (a world without
/// culture hashes as before the state existed).
pub(crate) fn hash_culture(auth: &AuthWorld, mix: &mut dyn FnMut(u64)) {
    let empty = PlayerCulture::default();
    for (i, p) in auth.module().culture().players().iter().enumerate() {
        if *p == empty {
            continue;
        }
        mix(0x6375_6c74_0000 | i as u64);
        mix(u64::from(p.culture));
        mix(p.lifetime);
        for c in &p.recent {
            mix(u64::from(*c));
        }
        for policy in &p.policies {
            mix(0x1_0000 | u64::from(policy.0));
        }
        for branch in &p.branches {
            mix(0x2_0000 | u64::from(branch.0));
        }
        mix(p.ideology.map(|b| u64::from(b.0) + 1).unwrap_or(0));
        mix(u64::from(p.free_policies) << 32
            | u64::from(p.free_tenets) << 16
            | u64::from(p.free_used));
        mix(u64::from(p.max_cities));
        for (b, n) in &p.free_buildings {
            mix(0x3_0000 | u64::from(b.0) << 8 | u64::from(*n));
        }
    }
    for (b, n) in auth.module().culture().ideology_adopters() {
        mix(0x4_0000 | u64::from(b.0) << 8 | u64::from(*n));
    }
}
