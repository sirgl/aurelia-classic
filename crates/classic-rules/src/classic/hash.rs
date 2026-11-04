//! The classic module's part of the authoritative hash
//! (`world::module::HashPoint`). Each section mixes at the point where it
//! mixed before the platform split, so the pinned game hashes hold; a
//! pristine section mixes nothing, so games without the mechanic keep their
//! hashes. The saved form is the canonical one: what survives a save is
//! what is hashed.

use crate::classic::ClassicWorld;
use crate::ids::PlayerId;
use crate::world::module::HashPoint;
use crate::world::state::AuthWorld;

pub(crate) fn hash(
    world: &ClassicWorld,
    point: HashPoint,
    auth: &AuthWorld,
    mix: &mut dyn FnMut(u64),
) {
    match point {
        HashPoint::Player(player) => military(world, player, mix),
        HashPoint::Diplomacy => relations(world, auth, mix),
        HashPoint::Economy => trade(auth, mix),
        HashPoint::AfterEconomy => society(auth, mix),
        HashPoint::AfterRng => {
            barbarians(world, mix);
            espionage(world, auth, mix);
            congress(world, mix);
            victory(world, mix);
            city_states(world, mix);
        }
    }
}

fn military(world: &ClassicWorld, p: PlayerId, mix: &mut dyn FnMut(u64)) {
    // Great general and admiral points: mixed only when not the default
    // (a major without military history hashes as before M4).
    let m = world.military().of(p);
    if !m.is_default() {
        mix(0x6d69_6c69);
        mix((m.general_points as u64) << 32 | m.admiral_points as u64);
        mix((m.generals_born as u64) << 16 | m.admirals_born as u64);
    }
}

fn relations(world: &ClassicWorld, auth: &AuthWorld, mix: &mut dyn FnMut(u64)) {
    // Relations beyond war and borders: mixed only when not pristine (a
    // game without treaties hashes as before M4).
    let relations = world.relations();
    if !relations.is_pristine() {
        let city_ordinals = crate::persistence::encode::city_ordinals(auth);
        let saved = crate::persistence::relations::encode_relations(relations, &city_ordinals);
        mix(0x7265_6c61);
        let matrices = [
            &saved.peace_until,
            &saved.war_since,
            &saved.pact_until,
            &saved.research_until,
            &saved.research_points,
            &saved.friends_until,
            &saved.denounced_until,
            &saved.open_until,
            &saved.warmonger,
        ];
        for m in matrices {
            for v in m {
                mix(u64::from(*v));
            }
        }
        for e in &saved.embassy {
            mix(u64::from(*e));
        }
        for (from, to, (tag, value, extra), until) in &saved.trades {
            mix(u64::from(*from) << 8 | u64::from(*to));
            mix(u64::from(*tag) << 48 | u64::from(*value) << 16 | u64::from(*extra));
            mix(u64::from(*until));
        }
        for (id, from, to, expires, items) in &saved.offers {
            mix(u64::from(*id) << 16 | u64::from(*from) << 8 | u64::from(*to));
            mix(u64::from(*expires));
            for (giver, (tag, value, extra)) in items {
                mix(u64::from(*giver) << 56
                    | u64::from(*tag) << 48
                    | u64::from(*value) << 16
                    | u64::from(*extra));
            }
        }
        mix(u64::from(saved.next_offer));
    }
}

/// Inside the economy fold, after the difficulty.
fn trade(auth: &AuthWorld, mix: &mut dyn FnMut(u64)) {
    // Mixed only with routes: games without trade keep their hashes.
    let routes = crate::persistence::trade::encode_routes(auth);
    for w in crate::persistence::trade::hash_words(&routes) {
        mix(w);
    }
}

fn society(auth: &AuthWorld, mix: &mut dyn FnMut(u64)) {
    // Culture pools and policies (players with a state only).
    crate::persistence::culture::hash_culture(auth, mix);
    // Faith and religion (an untouched state mixes nothing).
    crate::persistence::religion::hash_faith(auth, mix);
    crate::persistence::tourism::hash_tourism(auth, mix);
}

fn barbarians(world: &ClassicWorld, mix: &mut dyn FnMut(u64)) {
    // Barbarians: mixed only in games with barbarians (with their stream).
    let barbarians = world.barbarians();
    if !barbarians.is_pristine() {
        mix(0x6261_7262 ^ crate::classic::save::encode_barbarian_mode(barbarians.mode) as u64);
        mix(u64::from(barbarians.released()));
        for c in barbarians.camps() {
            mix((c.tile.0 as u64) << 8 | c.spawn_in as u64);
        }
        for c in barbarians.cleared() {
            mix((c.tile.0 as u64) << 40 ^ (c.player.0 as u64) << 32 ^ c.round as u64);
        }
        mix(barbarians.rng().state());
    }
}

fn espionage(world: &ClassicWorld, auth: &AuthWorld, mix: &mut dyn FnMut(u64)) {
    // Espionage: mixed once a spy exists.
    if !world.espionage().is_pristine() {
        let ordinals = crate::persistence::encode::city_ordinals(auth);
        let e = crate::persistence::espionage::encode_espionage(world.espionage(), &ordinals);
        mix(0x7370_7973 ^ u64::from(e.renaissance_granted));
        for ((name, rank, state, city), (amount, rate, goal), (diplomat, dead)) in
            e.spies.iter().flatten()
        {
            mix(u64::from(*name) << 56
                ^ u64::from(*rank) << 48
                ^ u64::from(*state) << 40
                ^ u64::from(*city));
            mix(*amount ^ rate.rotate_left(21) ^ goal.rotate_left(42));
            mix(u64::from(*diplomat) << 8 ^ u64::from(*dead));
        }
        for r in e.steal_rights.iter().chain(&e.era_granted) {
            mix(u64::from(*r));
        }
    }
}

fn congress(world: &ClassicWorld, mix: &mut dyn FnMut(u64)) {
    // World Congress: mixed once the league's turn ran.
    if !world.congress().is_pristine() {
        let c = crate::persistence::congress::encode_congress(world.congress());
        mix(0x636f_6e67 ^ u64::from(c.founded) << 8 ^ u64::from(c.special) << 16);
        mix(c.host.map(|p| u64::from(p) + 1).unwrap_or(0) ^ u64::from(c.turns_until_session) << 8);
        mix(c.session_round.map(|r| u64::from(r) + 1).unwrap_or(0)
            ^ u64::from(c.leader_vote_due) << 40);
        mix(c.last_round.map(|r| u64::from(r) + 1).unwrap_or(0) ^ u64::from(c.united_nations) << 40);
        for p in &c.proposers {
            mix(u64::from(*p));
        }
        for (k, choice, proposer, repeal) in &c.proposals {
            mix(u64::from(*k) << 48
                ^ u64::from(*choice)
                ^ proposer.map(|p| u64::from(p) + 1).unwrap_or(0) << 32
                ^ u64::from(*repeal) << 56);
        }
        for d in c.delegates.iter().chain(&c.extra) {
            mix(u64::from(*d));
        }
        for (voter, proposal, choice, delegates) in &c.votes {
            mix(u64::from(*voter) << 56
                ^ u64::from(*proposal) << 48
                ^ u64::from(*choice) << 16
                ^ u64::from(*delegates));
        }
        for (k, choice) in &c.active {
            mix(u64::from(*k) << 32 ^ u64::from(*choice));
        }
        // Projects and their rewards: mixed only once they exist.
        for (project, contributions) in &c.projects {
            mix(0x7072_6f6a ^ u64::from(*project));
            for amount in contributions {
                mix(*amount);
            }
        }
        for project in &c.completed {
            mix(0x646f_6e65 ^ u64::from(*project));
        }
        for (player, (happiness, culture, tourism)) in c.rewards.iter().enumerate() {
            if (*happiness, *culture, *tourism) != (0, 0, 0) {
                mix((player as u64) << 48
                    ^ u64::from(*happiness) << 32
                    ^ u64::from(*culture) << 16
                    ^ u64::from(*tourism));
            }
        }
    }
}

fn victory(world: &ClassicWorld, mix: &mut dyn FnMut(u64)) {
    // Victory: mixed only once a project, an option or an outcome exists.
    if !world.victory().is_pristine() {
        let saved = crate::persistence::victory::encode_victory(world.victory());
        mix(0x7669_6374 ^ u64::from(saved.disabled) << 32 ^ u64::from(saved.max_rounds));
        for c in &saved.project_counts {
            mix(u64::from(*c));
        }
        mix(saved.world_leader.map(|p| u64::from(p) + 1).unwrap_or(0));
        if let Some((kind, winner, round)) = saved.outcome {
            mix(u64::from(kind) << 40 ^ u64::from(winner) << 32 ^ u64::from(round));
        }
        mix(u64::from(saved.continued));
    }
}

fn city_states(world: &ClassicWorld, mix: &mut dyn FnMut(u64)) {
    // City-states: mixed only once something happened with them.
    let city_states = world.city_states();
    if !city_states.is_pristine() {
        let saved = crate::persistence::city_states::encode_city_states(city_states);
        mix(0x6373_7461);
        for (a, b, c, d, e) in &saved.standings {
            mix(u64::from(*a) << 32 ^ u64::from(*b) << 24 ^ u64::from(*c));
            mix(u64::from(*d) << 32 ^ u64::from(*e));
        }
        for (a, b) in &saved.counters {
            mix(u64::from(*a) << 32 ^ u64::from(*b));
        }
        for (a, b, c, d) in &saved.minors {
            mix(u64::from(*a) << 56
                ^ u64::from(*b) << 16
                ^ u64::from(*c) << 8
                ^ u64::from(*d) << 32);
        }
        for (a, b, c) in &saved.majors {
            mix(u64::from(*a) << 32 ^ u64::from(*b) ^ u64::from(*c) << 48);
        }
        for (cs, major, (tag, v), start, end) in &saved.quests {
            mix(u64::from(*cs) << 56
                ^ u64::from(*major) << 48
                ^ u64::from(*tag) << 40
                ^ u64::from(*v));
            mix(u64::from(*start) << 32 ^ u64::from(*end));
        }
    }
}
