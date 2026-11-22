//! The guard of `systems::elimination`: what in the world state still names a player. After
//! a nation is eliminated nothing may (the tests of the elimination ask for an empty list).
//!
//! Two kinds of reference are searched:
//!
//! - **Typed**: every `PlayerId` the module's state holds, found in its `Debug` text, so a new
//!   field that holds a `PlayerId` is searched without being listed here. Only history is left
//!   out, by name below: what a dead nation did stays true after its death.
//! - **Positional**: tables indexed by player (pairs `players × players`, one entry per
//!   player) cannot be found by their text. They are listed one by one in `positional`. A new
//!   table of ties between players must join that list.
//!
//! Not references (a nation's own record or its measure, read only for the living): the
//! per-player parts of culture, religion, tourism (influence included), military, victory
//! projects, congress rewards and project contributions, espionage eras.

use crate::ids::PlayerId;
use crate::world::congress::ResolutionKind;
use crate::world::state::AuthWorld;

/// Every place of the world state that still names `player`, by name (empty: none).
pub fn references_to(auth: &AuthWorld, player: PlayerId) -> Vec<String> {
    let mut found = typed(auth, player);
    found.extend(positional(auth, player));
    found
}

/// The parts whose `Debug` text holds `PlayerId(player)`. History is left out: who cleared a
/// camp, who won, who created a great work, who fought on a tile (archaeology), who founded
/// a religion (the religion lives on without its founder, as in Civ V).
fn typed(auth: &AuthWorld, player: PlayerId) -> Vec<String> {
    let world = auth.module();
    let mut barbarians = world.barbarians.clone();
    barbarians.cleared.clear();
    let mut victory = world.victory.clone();
    victory.outcome = None;
    let parts = [
        ("trade", format!("{:?}", world.trade)),
        ("military", format!("{:?}", world.military)),
        ("barbarians", format!("{barbarians:?}")),
        ("city_states", format!("{:?}", world.city_states)),
        ("victory", format!("{victory:?}")),
        ("congress", format!("{:?}", world.congress)),
        ("espionage", format!("{:?}", world.espionage)),
        ("culture", format!("{:?}", world.culture)),
        ("religion", format!("{:?}", world.religion.without_founders())),
        ("tourism", format!("{:?}", world.tourism.without_history())),
        ("relations", format!("{:?}", world.relations)),
    ];
    let name = format!("PlayerId({})", player.0);
    let mut found = Vec::new();
    for (part, text) in parts {
        if text.contains(&name) {
            found.push(format!("{part}: {}", excerpt(&text, &name)));
        }
    }
    found
}

/// A few words around the first mention, to say which field it is.
fn excerpt(text: &str, name: &str) -> String {
    let at = text.find(name).unwrap_or(0);
    let start = text[..at].char_indices().rev().nth(80).map_or(0, |(i, _)| i);
    let end = text[at..].char_indices().nth(40).map_or(text.len(), |(i, _)| at + i);
    text[start..end].to_string()
}

/// The tables indexed by player that hold a tie with `player`. Every table of ties between
/// players is here; a new one must join.
fn positional(auth: &AuthWorld, player: PlayerId) -> Vec<String> {
    let mut found = Vec::new();
    let mut check = |part: &str, tied: bool| {
        if tied {
            found.push(part.to_string());
        }
    };
    let world = auth.module();
    let players = auth.players().count();
    let others: Vec<PlayerId> = (0..players).map(|i| PlayerId(i as u8)).filter(|p| *p != player).collect();
    let relations = &world.relations;
    let pair = |table: &[u32]| {
        others.iter().any(|o| {
            let ab = relations.slot(player, *o).map_or(0, |i| table[i]);
            let ba = relations.slot(*o, player).map_or(0, |i| table[i]);
            ab != 0 || ba != 0
        })
    };
    check("relations.peace_until", pair(&relations.peace_until));
    check("relations.war_since", pair(&relations.war_since));
    check("relations.pact_until", pair(&relations.pact_until));
    check("relations.research_until", pair(&relations.research_until));
    check("relations.research_points", pair(&relations.research_points));
    check("relations.friends_until", pair(&relations.friends_until));
    check("relations.denounced_until", pair(&relations.denounced_until));
    check("relations.open_until", pair(&relations.open_until));
    check("relations.embassy", others.iter().any(|o| relations.has_embassy(player, *o) || relations.has_embassy(*o, player)));
    check("relations.warmonger", others.iter().any(|o| relations.warmonger(player, *o) != 0 || relations.warmonger(*o, player) != 0));
    let diplomacy = auth.diplomacy();
    check("kernel diplomacy: war", others.iter().any(|o| diplomacy.at_war(player, *o)));
    check(
        "kernel diplomacy: open borders",
        others.iter().any(|o| diplomacy.grants_entry(player, *o) || diplomacy.grants_entry(*o, player)),
    );
    let city_states = &world.city_states;
    let fresh = crate::world::city_states::Standing::default();
    check(
        "city_states.standings",
        others.iter().any(|o| city_states.standing(*o, player) != fresh || city_states.standing(player, *o) != fresh),
    );
    check("city_states.majors", city_states.major(player) != Default::default());
    let congress = &world.congress;
    let named = u32::from(player.0);
    check("congress.delegates", congress.delegates_of(player) != 0);
    check("congress.extra", congress.extra.get(usize::from(player.0)).is_some_and(|n| *n != 0));
    let elects = |index: u8| congress.proposals.get(usize::from(index)).is_some_and(|p| p.kind.elects());
    check("congress.votes (candidate)", congress.votes.iter().any(|v| elects(v.proposal) && v.choice == named));
    let embargo = |kind: ResolutionKind, choice: u32| kind == ResolutionKind::PlayerEmbargo && choice == named;
    check("congress.proposals (embargo)", congress.proposals.iter().any(|p| embargo(p.kind, p.choice)));
    check("congress.active (embargo)", congress.active.iter().any(|a| embargo(a.kind, a.choice)));
    let espionage = &world.espionage;
    check("espionage.spies", !espionage.spies_of(player).is_empty());
    check(
        "espionage.steal_rights",
        others.iter().any(|o| espionage.steal_rights(player, *o) != 0 || espionage.steal_rights(*o, player) != 0),
    );
    found
}
