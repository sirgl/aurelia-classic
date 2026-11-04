//! The events a player learns between two of its turns, for the end-to-end
//! net (classic/docs/ai-net-data.md, Format 7): what the game's
//! notifications tell a human — a war declared on it or by it, a city of its
//! lost or a city it took, a rival's world wonder (and its own production of
//! that wonder lost), a foreign city founded or first seen, a city-state's
//! quest, a religion founded, a unit lost, a nation entering an era.
//!
//! An event is found as the difference of two observations of the same
//! player: the start of its previous turn and the start of this one
//! ([`Snapshot`]). Both are the player's own view (`AIObservation`: its
//! empire, the relations it has, the cities it sees or remembers, the
//! city-states it met) and the announcements every player gets
//! (`PlayerQueries::wonder_news`, `met_eras`): nothing behind the fog.
//!
//! A [`Tracker`] holds the last snapshot and the events of the last
//! [`KEEP_ROUNDS`] rounds (at most [`MAX_PER_KIND`] of a kind and
//! [`MAX_EVENTS`] in all, the newest kept). The net player keeps it in the
//! player's saved AI memory (`mind.rs`); the exporter and the E2 rebuild run
//! one per nation over the replay and observe at the same turn starts, so
//! the three read the same events. A tracker's first observation gives no
//! event: a game the net joins mid-way (a mid-game start, a loaded game
//! without its AI state) starts with an empty log.
//!
//! The kinds are mechanics of the rules, never content: which wonder,
//! religion, quest, unit or era an event is about is its parameter (the
//! content's id, embedded by the net) and its table data (era, cost).

use crate::ids::{BuildingKindId, PlayerId, ProductionItem, TileId};
use crate::knowledge::observation::AIObservation;
use crate::knowledge::public::PlayerQueries;
use crate::rules::content::BuildingKind;
use crate::rules::CompiledRuleset;
use crate::world::city_states::QuestKind;
use crate::world::player::{ClassicKind, PlayerKind};

/// Events older than this many rounds are forgotten (age 0 … 9).
pub const KEEP_ROUNDS: u32 = 10;
/// At most this many events in a log (the newest).
pub const MAX_EVENTS: usize = 64;
/// At most this many events of one kind in a log (the newest).
pub const MAX_PER_KIND: usize = 8;

/// The kinds of event (`event_meta` column 0; the row's
/// `event_kind_id_plus_1` is the kind + 1).
pub mod kind {
    /// We are at war with a nation or city-state we were at peace with.
    pub const WAR_BEGAN: u8 = 0;
    /// A war of ours ended.
    pub const PEACE_MADE: u8 = 1;
    /// We met a nation or city-state.
    pub const MET: u8 = 2;
    /// A nation or city-state we knew is gone.
    pub const ELIMINATED: u8 = 3;
    /// A friendship began (value 1) or ended (−1).
    pub const FRIENDSHIP: u8 = 4;
    /// A defensive pact began (1) or ended (−1).
    pub const DEFENSIVE_PACT: u8 = 5;
    /// A nation denounced us.
    pub const DENOUNCED_US: u8 = 6;
    /// A city of ours is no longer ours (taken, or razed).
    pub const CITY_LOST: u8 = 7;
    /// We took a city (it was another player's).
    pub const CITY_TAKEN: u8 = 8;
    /// A city of ours was damaged.
    pub const CITY_ATTACKED: u8 = 9;
    /// A foreign city we did not know: founded, or first seen.
    pub const FOREIGN_CITY: u8 = 10;
    /// A foreign city we know changed hands between other players.
    pub const CITY_CHANGED_HANDS: u8 = 11;
    /// A unit of ours is gone (not by founding a city or by an upgrade).
    pub const UNIT_LOST: u8 = 12;
    /// Units of ours were damaged (one event per turn).
    pub const UNITS_DAMAGED: u8 = 13;
    /// A world wonder was completed by another player.
    pub const WONDER_ELSEWHERE: u8 = 14;
    /// Another player founded a religion.
    pub const RELIGION_FOUNDED: u8 = 15;
    /// A city-state gave us a quest.
    pub const QUEST_GIVEN: u8 = 16;
    /// A quest of a city-state to us ended (done, or expired).
    pub const QUEST_ENDED: u8 = 17;
    /// A city-state's ally changed.
    pub const ALLY_CHANGED: u8 = 18;
    /// A nation we know entered a later era.
    pub const ERA_REACHED: u8 = 19;
    pub const NAMES: [&str; 20] = [
        "war_began",
        "peace_made",
        "met",
        "eliminated",
        "friendship",
        "defensive_pact",
        "denounced_us",
        "city_lost",
        "city_taken",
        "city_attacked",
        "foreign_city",
        "city_changed_hands",
        "unit_lost",
        "units_damaged",
        "wonder_elsewhere",
        "religion_founded",
        "quest_given",
        "quest_ended",
        "ally_changed",
        "era_reached",
    ];
}

/// One event, as the player learned it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    /// The round of the turn start that found it.
    pub round: u32,
    pub kind: u8,
    /// The other party: the nation or city-state it is about (None: none,
    /// or not known to the player).
    pub actor: Option<PlayerId>,
    pub tile: Option<TileId>,
    /// The content it is about (−1: none): a building (wonder), a religion,
    /// a unit kind, a quest kind (`quest_code`), an era index.
    pub param: i32,
    /// An amount of the kind (`ai-net-data.md`, Format 7).
    pub value: i32,
    /// How many occurrences it stands for (units damaged).
    pub count: u16,
    /// The event touched the player's own plans: its production of the
    /// wonder completed elsewhere.
    pub ours: bool,
    /// The item's era index + 1 (0: none) and cost (0: none).
    pub item_era: i16,
    pub item_cost: u32,
}

impl Event {
    fn new(round: u32, kind: u8) -> Self {
        Event { round, kind, actor: None, tile: None, param: -1, value: 0, count: 1, ours: false, item_era: 0, item_cost: 0 }
    }

    fn actor(self, p: Option<PlayerId>) -> Self {
        Event { actor: p, ..self }
    }

    fn tile(self, t: Option<TileId>) -> Self {
        Event { tile: t, ..self }
    }

    fn param(self, param: i32) -> Self {
        Event { param, ..self }
    }

    fn value(self, value: i32) -> Self {
        Event { value, ..self }
    }
}

/// The code of a quest kind (its variant, in the declaration order of
/// `QuestKind`): the event's parameter, without the quest's target.
pub fn quest_code(quest: QuestKind) -> u8 {
    match quest {
        QuestKind::KillCamp(_) => 0,
        QuestKind::ConnectResource(_) => 1,
        QuestKind::ConstructWonder(_) => 2,
        QuestKind::FindPlayer(_) => 3,
        QuestKind::FindNaturalWonder { .. } => 4,
        QuestKind::GiveGold { .. } => 5,
        QuestKind::PledgeToProtect => 6,
        QuestKind::BullyCityState(_) => 7,
        QuestKind::DenounceMajor(_) => 8,
        QuestKind::ContestTechs { .. } => 9,
        QuestKind::Invest => 10,
    }
}

/// Relation bits of a snapshot.
const WAR: u8 = 1;
const FRIENDS: u8 = 2;
const PACT: u8 = 4;
const DENOUNCED: u8 = 8;

/// What a player knew at the start of one of its turns, as far as events
/// are found from it. Every list is sorted by its first field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub round: u32,
    /// Own cities: entity id, tile, population, damage.
    cities: Vec<(u64, u32, u32, u16)>,
    /// Own units: entity id, kind, hp, tile.
    units: Vec<(u64, u16, u16, u32)>,
    /// Nations and city-states met (barbarians aside): player, relation bits.
    relations: Vec<(u8, u8)>,
    /// Foreign cities seen now or remembered: tile, owner, population.
    foreign: Vec<(u32, u8, u32)>,
    /// World wonders built (anywhere).
    wonders: Vec<u16>,
    /// World wonders in own queues: building, the production stock of a city
    /// where it heads the queue (0 when it waits behind another item).
    queued: Vec<(u16, u32)>,
    /// Religions founded: religion, founder.
    religions: Vec<(u16, u8)>,
    /// City-states met: player, ally (−1 none), its quests to us (codes, sorted).
    city_states: Vec<(u8, i16, Vec<u8>)>,
    /// Eras of the nations met: player, era.
    eras: Vec<(u8, u8)>,
}

fn is_world_wonder(rules: &CompiledRuleset, b: BuildingKindId) -> bool {
    rules.buildings.get(usize::from(b.0)).is_some_and(|d| d.wonder && d.kind != BuildingKind::NationalWonder)
}

impl Snapshot {
    /// The snapshot of `obs`'s player now.
    pub fn of(obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Self {
        let mut cities: Vec<_> = obs.own_cities.iter().map(|c| (c.id.0, c.tile.0, c.population, c.damage)).collect();
        cities.sort_unstable();
        let mut units: Vec<_> = obs.own_units.iter().map(|u| (u.id.0, u.kind.0, u.hp, u.tile.0)).collect();
        units.sort_unstable();
        let mut relations: Vec<(u8, u8)> = obs
            .relations
            .iter()
            .filter(|r| !matches!(r.kind, PlayerKind::Module(ClassicKind::Barbarian)))
            .map(|r| (r.other.0, (u8::from(r.at_war) * WAR) | (u8::from(r.friends) * FRIENDS) | (u8::from(r.defensive_pact) * PACT) | (u8::from(r.they_denounced) * DENOUNCED)))
            .collect();
        relations.sort_unstable();
        let mut foreign: Vec<(u32, u8, u32)> = obs.visible_foreign_cities.iter().filter(|c| c.owner != obs.player).map(|c| (c.tile.0, c.owner.0, c.population)).collect();
        foreign.extend(obs.view.remembered_cities().filter(|(_, s)| s.owner != obs.player).map(|(t, s)| (t.0, s.owner.0, s.seen.as_ref().map_or(0, |seen| seen.population))));
        foreign.sort_unstable();
        foreign.dedup_by_key(|f| f.0);
        let mut wonders: Vec<u16> = obs.built_wonders.iter().filter(|b| is_world_wonder(rules, **b)).map(|b| b.0).collect();
        wonders.sort_unstable();
        wonders.dedup();
        let mut queued: Vec<(u16, u32)> = Vec::new();
        for city in &obs.own_cities {
            for (i, item) in city.queue.iter().enumerate() {
                if let ProductionItem::Building(b) = *item {
                    if is_world_wonder(rules, b) {
                        let stock = if i == 0 { city.production } else { 0 };
                        match queued.iter_mut().find(|q| q.0 == b.0) {
                            Some(q) => q.1 = q.1.max(stock),
                            None => queued.push((b.0, stock)),
                        }
                    }
                }
            }
        }
        queued.sort_unstable();
        let mut religions: Vec<(u16, u8)> = obs.faith.overview.religions.iter().map(|r| (r.religion.0, r.founder.0)).collect();
        religions.sort_unstable();
        let mut city_states: Vec<(u8, i16, Vec<u8>)> = obs
            .city_states
            .iter()
            .map(|c| {
                let mut quests: Vec<u8> = c.quests.iter().map(|q| quest_code(*q)).collect();
                quests.sort_unstable();
                (c.player.0, c.ally.map_or(-1, |a| i16::from(a.0)), quests)
            })
            .collect();
        city_states.sort_unstable();
        let mut eras: Vec<(u8, u8)> = obs.view.met_eras().into_iter().map(|(p, e)| (p.0, e)).collect();
        eras.sort_unstable();
        Snapshot { round: obs.round, cities, units, relations, foreign, wonders, queued, religions, city_states, eras }
    }
}

/// The table data of a building or a unit kind: era index + 1, cost.
fn building_item(rules: &CompiledRuleset, b: u16) -> (i16, u32) {
    rules.buildings.get(usize::from(b)).map_or((0, 0), |d| (era_plus_1(rules, d.era), d.cost))
}

fn unit_item(rules: &CompiledRuleset, k: u16) -> (i16, u32) {
    rules.units.get(usize::from(k)).map_or((0, 0), |d| (era_plus_1(rules, d.era), d.cost))
}

fn era_plus_1(rules: &CompiledRuleset, era: Option<crate::rules::content::EraId>) -> i16 {
    era.and_then(|e| rules.eras.get(usize::from(e.0))).map_or(0, |e| i16::from(e.index) + 1)
}

/// The events between two snapshots of one player (`now` the later), in a
/// fixed order: relations, own cities, foreign cities, own units, wonders,
/// religions, city-states, eras.
fn diff(before: &Snapshot, now: &Snapshot, obs: &AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Event> {
    let round = now.round;
    let me = obs.player;
    let mut out = Vec::new();
    let ev = |k: u8| Event::new(round, k);
    let p = |id: u8| Some(PlayerId(id));
    // Relations.
    for &(other, bits) in &now.relations {
        match before.relations.binary_search_by_key(&other, |r| r.0) {
            Err(_) => {
                out.push(ev(kind::MET).actor(p(other)));
                if bits & WAR != 0 {
                    out.push(ev(kind::WAR_BEGAN).actor(p(other)));
                }
            }
            Ok(i) => {
                let old = before.relations[i].1;
                let rose = |b: u8| old & b == 0 && bits & b != 0;
                let fell = |b: u8| old & b != 0 && bits & b == 0;
                if rose(WAR) {
                    out.push(ev(kind::WAR_BEGAN).actor(p(other)));
                }
                if fell(WAR) {
                    out.push(ev(kind::PEACE_MADE).actor(p(other)));
                }
                if rose(FRIENDS) || fell(FRIENDS) {
                    out.push(ev(kind::FRIENDSHIP).actor(p(other)).value(if rose(FRIENDS) { 1 } else { -1 }));
                }
                if rose(PACT) || fell(PACT) {
                    out.push(ev(kind::DEFENSIVE_PACT).actor(p(other)).value(if rose(PACT) { 1 } else { -1 }));
                }
                if rose(DENOUNCED) {
                    out.push(ev(kind::DENOUNCED_US).actor(p(other)));
                }
            }
        }
    }
    for &(other, _) in &before.relations {
        if now.relations.binary_search_by_key(&other, |r| r.0).is_err() {
            out.push(ev(kind::ELIMINATED).actor(p(other)));
        }
    }
    // Own cities.
    let foreign_now = |tile: u32| now.foreign.binary_search_by_key(&tile, |f| f.0).ok().map(|i| now.foreign[i]);
    let foreign_before = |tile: u32| before.foreign.binary_search_by_key(&tile, |f| f.0).ok().map(|i| before.foreign[i]);
    for &(id, tile, population, damage) in &before.cities {
        match now.cities.binary_search_by_key(&id, |c| c.0) {
            Err(_) => out.push(ev(kind::CITY_LOST).actor(foreign_now(tile).map(|f| PlayerId(f.1))).tile(Some(TileId(tile))).value(population as i32)),
            Ok(i) => {
                let hit = i32::from(now.cities[i].3) - i32::from(damage);
                if hit > 0 {
                    out.push(ev(kind::CITY_ATTACKED).tile(Some(TileId(tile))).value(hit));
                }
            }
        }
    }
    let was_own_city = |tile: u32| before.cities.iter().any(|c| c.1 == tile);
    for &(id, tile, population, _) in &now.cities {
        if before.cities.binary_search_by_key(&id, |c| c.0).is_err() {
            if let Some(f) = foreign_before(tile) {
                out.push(ev(kind::CITY_TAKEN).actor(p(f.1)).tile(Some(TileId(tile))).value(population as i32));
            }
        }
    }
    // Foreign cities.
    for &(tile, owner, population) in &now.foreign {
        match foreign_before(tile) {
            None if !was_own_city(tile) => out.push(ev(kind::FOREIGN_CITY).actor(p(owner)).tile(Some(TileId(tile))).value(population as i32)),
            Some(f) if f.1 != owner && !was_own_city(tile) => out.push(ev(kind::CITY_CHANGED_HANDS).actor(p(owner)).tile(Some(TileId(tile))).value(population as i32)),
            _ => {}
        }
    }
    // Own units: gone (not into a city founded there, not replaced on its
    // tile by a new unit of ours: an upgrade), or damaged.
    let new_city_at = |tile: u32| now.cities.iter().any(|c| c.1 == tile && before.cities.binary_search_by_key(&c.0, |b| b.0).is_err());
    let new_unit_at = |tile: u32| now.units.iter().any(|u| u.3 == tile && before.units.binary_search_by_key(&u.0, |b| b.0).is_err());
    let (mut damaged, mut lost_hp, mut worst) = (0u16, 0i32, (0i32, 0u32));
    for &(id, kind_id, hp, tile) in &before.units {
        match now.units.binary_search_by_key(&id, |u| u.0) {
            Err(_) if !new_city_at(tile) && !new_unit_at(tile) => {
                let (era, cost) = unit_item(rules, kind_id);
                out.push(Event { item_era: era, item_cost: cost, ..ev(kind::UNIT_LOST).tile(Some(TileId(tile))).param(i32::from(kind_id)).value(i32::from(hp)) });
            }
            Err(_) => {}
            Ok(i) => {
                let hit = i32::from(hp) - i32::from(now.units[i].2);
                if hit > 0 {
                    damaged += 1;
                    lost_hp += hit;
                    if hit > worst.0 {
                        worst = (hit, now.units[i].3);
                    }
                }
            }
        }
    }
    if damaged > 0 {
        out.push(Event { count: damaged, ..ev(kind::UNITS_DAMAGED).tile(Some(TileId(worst.1))).value(lost_hp) });
    }
    // World wonders completed by others.
    let own_buildings = |b: u16| obs.own_cities.iter().any(|c| c.buildings.iter().any(|x| x.0 == b));
    let mut news: Option<Vec<crate::knowledge::public::WonderNews>> = None;
    for &b in &now.wonders {
        if before.wonders.binary_search(&b).is_ok() || own_buildings(b) {
            continue;
        }
        let news = news.get_or_insert_with(|| obs.view.wonder_news());
        let told = news.iter().find(|w| w.building.0 == b);
        let lost = before.queued.iter().find(|q| q.0 == b);
        let (era, cost) = building_item(rules, b);
        out.push(Event {
            ours: lost.is_some(),
            item_era: era,
            item_cost: cost,
            ..ev(kind::WONDER_ELSEWHERE).actor(told.and_then(|w| w.builder)).tile(told.and_then(|w| w.tile)).param(i32::from(b)).value(lost.map_or(0, |q| (q.1 / 10) as i32))
        });
    }
    // Religions founded by others.
    for &(religion, founder) in &now.religions {
        if founder == me.0 || before.religions.iter().any(|r| r.0 == religion) {
            continue;
        }
        let holy = obs.faith.overview.religions.iter().find(|r| r.religion.0 == religion).and_then(|r| obs.faith.cities.iter().find(|c| c.handle == r.holy_city)).map(|c| c.tile);
        let known = obs.relation(PlayerId(founder)).is_some();
        out.push(ev(kind::RELIGION_FOUNDED).actor(known.then_some(PlayerId(founder))).tile(holy).param(i32::from(religion)));
    }
    // City-states: quests to us given and ended, the ally changed.
    for (cs, ally, quests) in &now.city_states {
        let old = before.city_states.iter().find(|c| c.0 == *cs);
        let empty = Vec::new();
        let old_quests = old.map_or(&empty, |c| &c.2);
        for code in multiset_minus(quests, old_quests) {
            let mut e = ev(kind::QUEST_GIVEN).actor(p(*cs)).param(i32::from(code));
            if code == 2 {
                if let Some(QuestKind::ConstructWonder(b)) = obs.city_states.iter().find(|c| c.player.0 == *cs).and_then(|c| c.quests.iter().copied().find(|q| matches!(q, QuestKind::ConstructWonder(_)))) {
                    (e.item_era, e.item_cost) = building_item(rules, b.0);
                }
            }
            out.push(e);
        }
        for code in multiset_minus(old_quests, quests) {
            out.push(ev(kind::QUEST_ENDED).actor(p(*cs)).param(i32::from(code)));
        }
        if let Some(old) = old {
            if old.1 != *ally {
                let mine = |a: i16| i32::from(a == i16::from(me.0));
                out.push(ev(kind::ALLY_CHANGED).actor(p(*cs)).value(mine(*ally) - mine(old.1)));
            }
        }
    }
    // Eras of the nations met.
    for &(other, era) in &now.eras {
        if let Some(old) = before.eras.iter().find(|e| e.0 == other) {
            if era > old.1 {
                out.push(Event { item_era: i16::from(era) + 1, ..ev(kind::ERA_REACHED).actor(p(other)).param(i32::from(era)).value(i32::from(era)) });
            }
        }
    }
    out
}

/// Forgets the events of `log` (oldest first) older than [`KEEP_ROUNDS`] at
/// `round`, then keeps the newest: at most [`MAX_PER_KIND`] of a kind and
/// [`MAX_EVENTS`] in all.
fn prune(log: &mut Vec<Event>, round: u32) {
    log.retain(|e| round.saturating_sub(e.round) < KEEP_ROUNDS);
    let mut per_kind = [0usize; 256];
    let mut kept = 0usize;
    let mut keep = vec![false; log.len()];
    for i in (0..log.len()).rev() {
        let k = usize::from(log[i].kind);
        if per_kind[k] < MAX_PER_KIND && kept < MAX_EVENTS {
            per_kind[k] += 1;
            kept += 1;
            keep[i] = true;
        }
    }
    let mut i = 0;
    log.retain(|_| {
        i += 1;
        keep[i - 1]
    });
}

/// The items of `a` not matched in `b` (both sorted), with repeats.
fn multiset_minus(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut j = 0;
    for &x in a {
        while j < b.len() && b[j] < x {
            j += 1;
        }
        if j < b.len() && b[j] == x {
            j += 1;
        } else {
            out.push(x);
        }
    }
    out
}

/// A player's last snapshot and the events of its last rounds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tracker {
    snapshot: Option<Snapshot>,
    log: Vec<Event>,
}

/// The format of a tracker's bytes.
const TRACKER_FORMAT: u32 = 1;

impl Tracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// The events kept, oldest first (in the order found within a round).
    pub fn events(&self) -> &[Event] {
        &self.log
    }

    /// Observes the player at the start of its turn: the events since its
    /// last observation join the log (none at the first), the old ones and
    /// those over the caps leave it, and `obs` becomes the last snapshot.
    pub fn observe(&mut self, obs: &AIObservation<'_>, rules: &CompiledRuleset) {
        let now = Snapshot::of(obs, rules);
        if let Some(before) = self.snapshot.as_ref() {
            let found = diff(before, &now, obs, rules);
            self.log.extend(found);
        }
        prune(&mut self.log, now.round);
        self.snapshot = Some(now);
    }

    /// The tracker as bytes (little-endian): u32 format (1), u8 whether a
    /// snapshot follows, the snapshot, u32 count and the events.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = W(Vec::new());
        w.u32(TRACKER_FORMAT);
        match &self.snapshot {
            None => w.u8(0),
            Some(s) => {
                w.u8(1);
                w.u32(s.round);
                w.len(s.cities.len());
                for c in &s.cities {
                    w.u64(c.0);
                    w.u32(c.1);
                    w.u32(c.2);
                    w.u16(c.3);
                }
                w.len(s.units.len());
                for u in &s.units {
                    w.u64(u.0);
                    w.u16(u.1);
                    w.u16(u.2);
                    w.u32(u.3);
                }
                w.len(s.relations.len());
                for r in &s.relations {
                    w.u8(r.0);
                    w.u8(r.1);
                }
                w.len(s.foreign.len());
                for f in &s.foreign {
                    w.u32(f.0);
                    w.u8(f.1);
                    w.u32(f.2);
                }
                w.len(s.wonders.len());
                s.wonders.iter().for_each(|b| w.u16(*b));
                w.len(s.queued.len());
                for q in &s.queued {
                    w.u16(q.0);
                    w.u32(q.1);
                }
                w.len(s.religions.len());
                for r in &s.religions {
                    w.u16(r.0);
                    w.u8(r.1);
                }
                w.len(s.city_states.len());
                for c in &s.city_states {
                    w.u8(c.0);
                    w.u16(c.1 as u16);
                    w.len(c.2.len());
                    c.2.iter().for_each(|q| w.u8(*q));
                }
                w.len(s.eras.len());
                for e in &s.eras {
                    w.u8(e.0);
                    w.u8(e.1);
                }
            }
        }
        w.len(self.log.len());
        for e in &self.log {
            w.u32(e.round);
            w.u8(e.kind);
            w.u16(e.actor.map_or(u16::MAX, |p| u16::from(p.0)));
            w.u32(e.tile.map_or(u32::MAX, |t| t.0));
            w.u32(e.param as u32);
            w.u32(e.value as u32);
            w.u16(e.count);
            w.u8(u8::from(e.ours));
            w.u16(e.item_era as u16);
            w.u32(e.item_cost);
        }
        w.0
    }

    /// The tracker of `bytes` (`to_bytes`); None when they are not one.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let mut r = R(bytes);
        if r.u32()? != TRACKER_FORMAT {
            return None;
        }
        let snapshot = match r.u8()? {
            0 => None,
            1 => {
                let round = r.u32()?;
                let cities = r.list(|r| Some((r.u64()?, r.u32()?, r.u32()?, r.u16()?)))?;
                let units = r.list(|r| Some((r.u64()?, r.u16()?, r.u16()?, r.u32()?)))?;
                let relations = r.list(|r| Some((r.u8()?, r.u8()?)))?;
                let foreign = r.list(|r| Some((r.u32()?, r.u8()?, r.u32()?)))?;
                let wonders = r.list(|r| r.u16())?;
                let queued = r.list(|r| Some((r.u16()?, r.u32()?)))?;
                let religions = r.list(|r| Some((r.u16()?, r.u8()?)))?;
                let city_states = r.list(|r| Some((r.u8()?, r.u16()? as i16, r.list(|r| r.u8())?)))?;
                let eras = r.list(|r| Some((r.u8()?, r.u8()?)))?;
                Some(Snapshot { round, cities, units, relations, foreign, wonders, queued, religions, city_states, eras })
            }
            _ => return None,
        };
        let log = r.list(|r| {
            let round = r.u32()?;
            let kind = r.u8()?;
            let actor = r.u16()?;
            let tile = r.u32()?;
            Some(Event {
                round,
                kind,
                actor: (actor != u16::MAX).then_some(PlayerId(actor as u8)),
                tile: (tile != u32::MAX).then_some(TileId(tile)),
                param: r.u32()? as i32,
                value: r.u32()? as i32,
                count: r.u16()?,
                ours: r.u8()? != 0,
                item_era: r.u16()? as i16,
                item_cost: r.u32()?,
            })
        })?;
        r.0.is_empty().then_some(Tracker { snapshot, log })
    }
}

struct W(Vec<u8>);

impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn len(&mut self, n: usize) {
        self.u32(n as u32);
    }
}

struct R<'a>(&'a [u8]);

impl R<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let head = self.0.get(..N)?.try_into().ok()?;
        self.0 = &self.0[N..];
        Some(head)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take::<1>()?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take()?))
    }
    fn list<T>(&mut self, mut item: impl FnMut(&mut Self) -> Option<T>) -> Option<Vec<T>> {
        let n = self.u32()? as usize;
        if n > self.0.len() {
            return None;
        }
        (0..n).map(|_| item(self)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiset_minus_keeps_repeats() {
        assert_eq!(multiset_minus(&[1, 2, 2, 5], &[2, 3]), vec![1, 2, 5]);
        assert_eq!(multiset_minus(&[], &[1]), Vec::<u8>::new());
    }

    #[test]
    fn a_tracker_round_trips_its_bytes() {
        let mut t = Tracker::new();
        t.snapshot = Some(Snapshot {
            round: 7,
            cities: vec![(3, 40, 5, 12)],
            units: vec![(9, 2, 100, 41)],
            relations: vec![(1, WAR | PACT)],
            foreign: vec![(77, 1, 3)],
            wonders: vec![4],
            queued: vec![(6, 120)],
            religions: vec![(0, 1)],
            city_states: vec![(5, -1, vec![2, 6])],
            eras: vec![(1, 2)],
        });
        t.log.push(Event { item_era: 3, item_cost: 250, ours: true, count: 2, ..Event::new(6, kind::WONDER_ELSEWHERE).actor(Some(PlayerId(1))).param(4).value(-3) });
        t.log.push(Event::new(7, kind::MET));
        let bytes = t.to_bytes();
        assert_eq!(Tracker::from_bytes(&bytes), Some(t.clone()));
        assert_eq!(Tracker::from_bytes(&bytes[..bytes.len() - 1]), None);
        assert_eq!(Tracker::from_bytes(&Tracker::new().to_bytes()), Some(Tracker::new()));
    }

    #[test]
    fn the_caps_keep_the_newest() {
        // 3 lost units a round for rounds 0 … 11, one war a round: at round
        // 11 the rounds 2 … 11 remain, of them the newest 8 of each kind.
        let mut log = Vec::new();
        for r in 0..12u32 {
            for _ in 0..3 {
                log.push(Event::new(r, kind::UNIT_LOST));
            }
            log.push(Event::new(r, kind::WAR_BEGAN));
        }
        prune(&mut log, 11);
        let units: Vec<u32> = log.iter().filter(|e| e.kind == kind::UNIT_LOST).map(|e| e.round).collect();
        assert_eq!(units, vec![9, 9, 10, 10, 10, 11, 11, 11]);
        let wars: Vec<u32> = log.iter().filter(|e| e.kind == kind::WAR_BEGAN).map(|e| e.round).collect();
        assert_eq!(wars, (4..12).collect::<Vec<u32>>());
        // MAX_EVENTS over many kinds.
        let mut many: Vec<Event> = (0..200u32).map(|i| Event::new(5, (i % 20) as u8)).collect();
        prune(&mut many, 5);
        assert_eq!(many.len(), MAX_EVENTS.min(20 * MAX_PER_KIND));
    }
}
