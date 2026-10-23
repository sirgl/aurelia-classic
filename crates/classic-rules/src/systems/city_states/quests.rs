//! City-state quests (§4.9): personal quests (at most two per civilization
//! and city-state) and global ones (one at a time), started after
//! countdowns, completed for influence, expired or revoked.
//!
//! Implemented kinds: kill a barbarian camp, connect a resource, build a
//! wonder, find a civilization, find a natural wonder, give gold, pledge to
//! protect, bully another city-state, denounce a civilization, the
//! technology contest, invest. Not yet: route, great person, spread
//! religion, trade route, culture and faith contests (they need the
//! systems of tracks A and B).

use super::{majors, type_of};
use crate::ids::{BuildingKindId, PlayerId, ResourceId, TileId};
use crate::rules::content::{BuildingKind, CityStateTrait, ResourceClass};
use crate::rules::CompiledRuleset;
use crate::transaction::transaction::WorldTransaction;
use crate::world::city_states::{Personality, Quest, QuestKind};
use crate::world::state::AuthWorld;

/// First round a quest may start, and the random extra rounds added to it.
/// Unciv: logic/civilization/managers/quests/QuestManager.kt (30, 20);
/// equal.
pub const FIRST_QUEST_ROUND: u32 = 30;
/// Unciv: QuestManager.kt, equal.
pub const FIRST_QUEST_RAND: u64 = 20;
/// Least rounds between two personal quests, between two global quests, and
/// the random extra rounds. Unciv: QuestManager.kt (20, 40, 25); equal.
pub const PERSONAL_MIN_BETWEEN: i32 = 20;
/// Unciv: QuestManager.kt, equal.
pub const GLOBAL_MIN_BETWEEN: i32 = 40;
/// Unciv: QuestManager.kt, equal.
pub const RAND_BETWEEN: u64 = 25;
/// Active personal quests per civilization and city-state. Unciv:
/// QuestManager.kt (2); equal.
pub const MAX_PERSONAL: usize = 2;
/// Rounds a contest quest lasts at Standard speed. Unciv: Quests.json
/// (Gods & Kings, duration 30); equal.
pub const CONTEST_LENGTH: u32 = 30;
/// Distance (tiles) from the city-state's capital within which a barbarian
/// camp may be given as a quest. Unciv: logic/civilization/managers/quests/
/// QuestTargetHelpers.kt (8); equal. The nearest camp is chosen here,
/// Unciv picks a random one (own rule).
pub const KILL_CAMP_RANGE: i32 = 8;
/// Bullying this recent revokes personal quests (rounds).
pub const RECENT_BULLY_ROUNDS: u32 = 20;

/// Influence reward of a completed quest, by kind. Unciv: Quests.json
/// (Gods & Kings: camp 50, find player 35, gold and pledge 20, invest 0,
/// default 40); equal.
pub fn reward(kind: QuestKind) -> i32 {
    match kind {
        QuestKind::KillCamp(_) => 50,
        QuestKind::FindPlayer(_) => 35,
        QuestKind::GiveGold { .. } | QuestKind::PledgeToProtect => 20,
        QuestKind::Invest => 0,
        QuestKind::ConnectResource(_)
        | QuestKind::ConstructWonder(_)
        | QuestKind::FindNaturalWonder { .. }
        | QuestKind::BullyCityState(_)
        | QuestKind::DenounceMajor(_)
        | QuestKind::ContestTechs { .. } => 40,
    }
}

/// The candidate quest kinds (by tag) a city-state may give.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tag {
    KillCamp,
    ConnectResource,
    ConstructWonder,
    FindPlayer,
    FindNaturalWonder,
    GiveGold,
    PledgeToProtect,
    BullyCityState,
    DenounceMajor,
    ContestTechs,
    Invest,
}

const PERSONAL_TAGS: [Tag; 8] = [
    Tag::ConnectResource,
    Tag::ConstructWonder,
    Tag::FindPlayer,
    Tag::FindNaturalWonder,
    Tag::GiveGold,
    Tag::PledgeToProtect,
    Tag::BullyCityState,
    Tag::DenounceMajor,
];
const GLOBAL_TAGS: [Tag; 3] = [Tag::KillCamp, Tag::ContestTechs, Tag::Invest];

/// The bag weight of a quest kind, by personality and trait. Unciv:
/// Quests.json (`weightForCityStateType`); equal for the kinds implemented
/// here.
fn weight(tag: Tag, minor_trait: Option<CityStateTrait>, personality: Personality) -> i32 {
    use CityStateTrait as T;
    use Personality as P;
    let mut w = 1000;
    let mut by = |m: i32| w = w * m / 100;
    match tag {
        Tag::ConnectResource => match minor_trait {
            Some(T::Maritime) => by(200),
            Some(T::Mercantile) => by(300),
            _ => {}
        },
        Tag::ConstructWonder if minor_trait == Some(T::Cultured) => by(300),
        Tag::FindPlayer => match minor_trait {
            Some(T::Maritime) => by(300),
            Some(T::Mercantile) => by(200),
            _ => {}
        },
        Tag::FindNaturalWonder => {
            if minor_trait == Some(T::Militaristic) {
                by(50);
            }
            if personality == P::Hostile {
                by(30);
            }
        }
        Tag::GiveGold => by(match minor_trait {
            Some(T::Militaristic) => 200,
            Some(T::Mercantile) => 350,
            _ => 300,
        }),
        Tag::PledgeToProtect => by(match minor_trait {
            Some(T::Militaristic) => 200,
            Some(T::Cultured) => 350,
            _ => 300,
        }),
        Tag::BullyCityState => by(match personality {
            P::Hostile => 200,
            P::Irrational => 150,
            P::Friendly => 30,
            P::Neutral => 100,
        }),
        Tag::DenounceMajor => by(match minor_trait {
            Some(T::Religious) => 250,
            Some(T::Maritime) => 200,
            _ => 150,
        }),
        // Unciv: Quests.json, Contest Technologies weight 0.5 for Religious.
        Tag::ContestTechs if minor_trait == Some(T::Religious) => by(50),
        Tag::KillCamp => {
            by(300);
            if minor_trait == Some(T::Militaristic) {
                by(300);
            }
        }
        Tag::Invest if minor_trait == Some(T::Mercantile) => by(150),
        _ => {}
    }
    w / 10
}

/// The last civilization that bullied the city-state (most recent round).
fn last_bully(auth: &AuthWorld, city_state: PlayerId) -> Option<(PlayerId, u32)> {
    majors(auth)
        .into_iter()
        .filter_map(|p| {
            auth.module().city_states()
                .standing(city_state, p)
                .last_bullied
                .map(|r| (p, r))
        })
        .max_by_key(|(p, r)| (*r, std::cmp::Reverse(p.0)))
}

fn bullied_recently_by(auth: &AuthWorld, city_state: PlayerId, major: PlayerId) -> bool {
    let round = auth.turn().round;
    auth.module().city_states()
        .standing(city_state, major)
        .last_bullied
        .is_some_and(|r| r + RECENT_BULLY_ROUNDS >= round)
}

fn capital_tile(auth: &AuthWorld, rules: &CompiledRuleset, player: PlayerId) -> Option<TileId> {
    crate::combat::city::capital_of(auth, rules, player)
        .and_then(|c| auth.cities().get(c))
        .map(|c| c.tile)
}

/// Does `major` own (work) a tile with the resource improved?
pub fn has_resource(
    auth: &AuthWorld,
    rules: &CompiledRuleset,
    major: PlayerId,
    resource: ResourceId,
) -> bool {
    let map = auth.map();
    let Some(def) = rules.resources.get(resource.0 as usize) else {
        return false;
    };
    (0..map.tile_count() as u32).map(TileId).any(|t| {
        map.owner(t).flatten() == Some(major)
            && map.resource(t).flatten() == Some(resource)
            && (def.improvements.is_empty()
                || map.improvement(t).flatten().is_some_and(|i| def.improvements.contains(&i)))
    })
}

fn owns_building(auth: &AuthWorld, major: PlayerId, building: BuildingKindId) -> bool {
    auth.indexes()
        .cities_of(major)
        .iter()
        .filter_map(|c| auth.cities().get(*c))
        .any(|c| c.buildings.contains(&building))
}

fn built_anywhere(auth: &AuthWorld, building: BuildingKindId) -> Option<PlayerId> {
    auth.cities()
        .iter()
        .find(|(_, c)| c.buildings.contains(&building))
        .map(|(_, c)| c.owner)
}

/// `major` has seen land of `target` (its memory holds an owned tile).
fn found_territory(auth: &AuthWorld, major: PlayerId, target: PlayerId) -> bool {
    let Some(knowledge) = auth.knowledge().get(major.0 as usize) else {
        return false;
    };
    (0..auth.map().tile_count() as u32)
        .map(TileId)
        .filter(|t| knowledge.is_explored(*t))
        .any(|t| {
            knowledge
                .memory(t)
                .is_some_and(|m| m.layers().owner == Some(target))
        })
}

fn natural_wonders_known(auth: &AuthWorld, major: PlayerId) -> u16 {
    let Some(knowledge) = auth.knowledge().get(major.0 as usize) else {
        return 0;
    };
    (0..auth.map().tile_count() as u32)
        .map(TileId)
        .filter(|t| auth.map().natural_wonder(*t).is_some() && knowledge.is_explored(*t))
        .count() as u16
}

fn natural_wonders_total(auth: &AuthWorld) -> u16 {
    (0..auth.map().tile_count() as u32)
        .map(TileId)
        .filter(|t| auth.map().natural_wonder(*t).is_some())
        .count() as u16
}

/// Is the quest done by its civilization?
fn complete(auth: &AuthWorld, rules: &CompiledRuleset, q: &Quest) -> bool {
    let states = auth.module().city_states();
    match q.kind {
        QuestKind::KillCamp(tile) => {
            let gone = !auth.module().barbarians().camps().iter().any(|c| c.tile == tile);
            gone && auth
                .module()
                .barbarians()
                .cleared()
                .iter()
                .any(|c| c.tile == tile && c.player == q.major)
        }
        QuestKind::ConnectResource(r) => has_resource(auth, rules, q.major, r),
        QuestKind::ConstructWonder(b) => owns_building(auth, q.major, b),
        QuestKind::FindPlayer(target) => found_territory(auth, q.major, target),
        QuestKind::FindNaturalWonder { known } => natural_wonders_known(auth, q.major) > known,
        QuestKind::GiveGold { gifted } => states.major(q.major).gold_gifted > gifted,
        QuestKind::PledgeToProtect => states.standing(q.city_state, q.major).pledged.is_some(),
        QuestKind::BullyCityState(target) => states
            .standing(target, q.major)
            .last_bullied
            .is_some_and(|r| r >= q.start),
        QuestKind::DenounceMajor(target) => {
            auth.module()
                .relations()
                .denounced(q.major, target, auth.turn().round)
        }
        QuestKind::ContestTechs { .. } | QuestKind::Invest => false,
    }
}

/// The quest can no longer be done (expired, revoked, target gone).
fn void(auth: &AuthWorld, q: &Quest) -> bool {
    let round = auth.turn().round;
    if q.end.is_some_and(|end| round > end) {
        return true;
    }
    let revocable = !matches!(
        q.kind,
        QuestKind::KillCamp(_) | QuestKind::ContestTechs { .. }
    );
    if revocable
        && bullied_recently_by(auth, q.city_state, q.major)
        && !matches!(q.kind, QuestKind::BullyCityState(_))
    {
        return true;
    }
    match q.kind {
        QuestKind::KillCamp(tile) => !auth.module().barbarians().camps().iter().any(|c| c.tile == tile),
        QuestKind::ConstructWonder(b) => built_anywhere(auth, b).is_some_and(|p| p != q.major),
        QuestKind::FindPlayer(t) | QuestKind::DenounceMajor(t) | QuestKind::BullyCityState(t) => {
            !auth.players().is_alive(t)
        }
        _ => false,
    }
}

impl<'a> WorldTransaction<'a> {
    /// Countdowns, new quests, completion and expiry for one city-state.
    pub(crate) fn quests_turn(&mut self, city_state: PlayerId, rules: &CompiledRuleset) {
        let round = self.world.turn.round;
        if round == FIRST_QUEST_ROUND {
            self.seed_countdowns(city_state, true);
        }
        self.tick_countdowns(city_state);
        if round > FIRST_QUEST_ROUND {
            self.try_global_quest(city_state, rules);
            for major in majors(self.world) {
                self.try_personal_quest(city_state, major, rules);
            }
        }
        self.check_quests(city_state, rules);
        self.finish_contests(city_state, rules);
    }

    fn seed_countdowns(&mut self, city_state: PlayerId, first: bool) {
        let n = self.world.players.count();
        for p in 0..n {
            let delay = self.quest_delay(city_state, first, PERSONAL_MIN_BETWEEN);
            if let Some(s) = self
                .world
                .module
                .city_states
                .standing_mut(city_state, PlayerId(p as u8))
            {
                s.quest_countdown = delay;
            }
        }
        let delay = self.quest_delay(city_state, first, GLOBAL_MIN_BETWEEN);
        if let Some(m) = self.world.module.city_states.minor_mut(city_state) {
            m.global_countdown = delay;
        }
    }

    /// Rounds until the next quest: rand(20) at first, then min + rand(25)
    /// (doubled random part for hostile city-states).
    fn quest_delay(&mut self, city_state: PlayerId, first: bool, min: i32) -> i32 {
        if first {
            return self.world.rng.events.below(FIRST_QUEST_RAND) as i32;
        }
        let hostile = self.world.module.city_states.minor(city_state).personality == Personality::Hostile;
        let random = if hostile {
            RAND_BETWEEN * 2
        } else {
            RAND_BETWEEN
        };
        min + self.world.rng.events.below(random) as i32
    }

    fn tick_countdowns(&mut self, city_state: PlayerId) {
        for p in 0..self.world.players.count() {
            if let Some(s) = self
                .world
                .module
                .city_states
                .standing_mut(city_state, PlayerId(p as u8))
            {
                s.quest_countdown = (s.quest_countdown - 1).max(0);
            }
        }
        if let Some(m) = self.world.module.city_states.minor_mut(city_state) {
            m.global_countdown = (m.global_countdown - 1).max(0);
        }
    }

    fn eligible(&self, city_state: PlayerId, major: PlayerId) -> bool {
        crate::systems::diplomacy::has_met(self.world, city_state, major)
            && !self.world.diplomacy.at_war(city_state, major)
            && capital_tile_any(self.world, major)
    }

    fn try_global_quest(&mut self, city_state: PlayerId, rules: &CompiledRuleset) {
        let active = self
            .world
            .module
            .city_states
            .quests
            .iter()
            .any(|q| q.city_state == city_state && q.kind.is_global());
        if active || self.world.module.city_states.minor(city_state).global_countdown > 0 {
            return;
        }
        let mut bag = Vec::new();
        for tag in GLOBAL_TAGS {
            let takers = self.global_takers(city_state, tag, rules);
            if !takers.is_empty() {
                bag.extend(std::iter::repeat_n(
                    tag,
                    self.tag_weight(city_state, tag, rules),
                ));
            }
        }
        if bag.is_empty() {
            return;
        }
        let tag = bag[self.world.rng.events.below(bag.len() as u64) as usize];
        for (major, kind) in self.global_takers(city_state, tag, rules) {
            self.start_quest(city_state, major, kind);
        }
        let delay = self.quest_delay(city_state, false, GLOBAL_MIN_BETWEEN);
        if let Some(m) = self.world.module.city_states.minor_mut(city_state) {
            m.global_countdown = delay;
        }
    }

    fn tag_weight(&self, city_state: PlayerId, tag: Tag, rules: &CompiledRuleset) -> usize {
        let minor_trait = type_of(self.world, rules, city_state).and_then(|t| t.minor_trait);
        let personality = self.world.module.city_states.minor(city_state).personality;
        weight(tag, minor_trait, personality).max(0) as usize
    }

    /// Civilizations a global quest would go to, with its payload; empty
    /// when fewer than the minimum take part.
    fn global_takers(
        &self,
        city_state: PlayerId,
        tag: Tag,
        rules: &CompiledRuleset,
    ) -> Vec<(PlayerId, QuestKind)> {
        let kind = match tag {
            Tag::KillCamp => match self.camp_for_quest(city_state, rules) {
                Some(tile) => QuestKind::KillCamp(tile),
                None => return Vec::new(),
            },
            Tag::Invest => QuestKind::Invest,
            _ => QuestKind::ContestTechs { start: 0 },
        };
        let takers: Vec<(PlayerId, QuestKind)> = majors(self.world)
            .into_iter()
            .filter(|m| self.eligible(city_state, *m))
            .filter(|m| tag != Tag::Invest || !bullied_recently_by(self.world, city_state, *m))
            .map(|m| (m, self.with_start(m, kind)))
            .collect();
        let minimum = match tag {
            Tag::ContestTechs => 3,
            Tag::Invest => 2,
            _ => 1,
        };
        let need = minimum.min(majors(self.world).len()).max(1);
        if takers.len() >= need {
            takers
        } else {
            Vec::new()
        }
    }

    fn with_start(&self, major: PlayerId, kind: QuestKind) -> QuestKind {
        match kind {
            QuestKind::ContestTechs { .. } => QuestKind::ContestTechs {
                start: self.world.players.techs_of(major).len() as u16,
            },
            other => other,
        }
    }

    /// An unowned camp within 12 tiles of the
    /// capital, the nearest first.
    fn camp_for_quest(&self, city_state: PlayerId, rules: &CompiledRuleset) -> Option<TileId> {
        let capital = capital_tile(self.world, rules, city_state)?;
        let dims = self.world.map.dimensions;
        self.world
            .module
            .barbarians
            .camps()
            .iter()
            .map(|c| c.tile)
            .filter(|t| self.world.map.owner(*t).flatten().is_none())
            .filter_map(|t| dims.distance(t, capital).map(|d| (d, t)))
            .filter(|(d, _)| *d <= KILL_CAMP_RANGE)
            .min_by_key(|(d, t)| (*d, t.0))
            .map(|(_, t)| t)
    }

    fn try_personal_quest(
        &mut self,
        city_state: PlayerId,
        major: PlayerId,
        rules: &CompiledRuleset,
    ) {
        let standing = self.world.module.city_states.standing(city_state, major);
        let active = self
            .world
            .module
            .city_states
            .quests
            .iter()
            .filter(|q| q.city_state == city_state && q.major == major && !q.kind.is_global())
            .count();
        if standing.quest_countdown > 0
            || active >= MAX_PERSONAL
            || !self.eligible(city_state, major)
        {
            return;
        }
        let mut bag = Vec::new();
        for tag in PERSONAL_TAGS {
            if let Some(kind) = self.personal_payload(city_state, major, tag, rules) {
                bag.extend(std::iter::repeat_n(
                    kind,
                    self.tag_weight(city_state, tag, rules),
                ));
            }
        }
        if bag.is_empty() {
            return;
        }
        let kind = bag[self.world.rng.events.below(bag.len() as u64) as usize];
        self.start_quest(city_state, major, kind);
        let delay = self.quest_delay(city_state, false, PERSONAL_MIN_BETWEEN);
        if let Some(s) = self.world.module.city_states.standing_mut(city_state, major) {
            s.quest_countdown = delay;
        }
    }

    /// The quest of `tag` for `major` with its payload, None when not valid
    /// now (one quest of a kind per pair; bullies get nothing).
    fn personal_payload(
        &mut self,
        city_state: PlayerId,
        major: PlayerId,
        tag: Tag,
        rules: &CompiledRuleset,
    ) -> Option<QuestKind> {
        let has = |q: &Quest, t: Tag| {
            q.city_state == city_state && q.major == major && tag_of(q.kind) == t
        };
        if self.world.module.city_states.quests.iter().any(|q| has(q, tag)) {
            return None;
        }
        let bullied_by_major = bullied_recently_by(self.world, city_state, major);
        if bullied_by_major {
            return None;
        }
        match tag {
            Tag::ConnectResource => self
                .resource_for_quest(major, rules)
                .map(QuestKind::ConnectResource),
            Tag::ConstructWonder => self
                .wonder_for_quest(major, rules)
                .map(QuestKind::ConstructWonder),
            Tag::FindPlayer => self.player_to_find(major).map(QuestKind::FindPlayer),
            Tag::FindNaturalWonder => {
                let known = natural_wonders_known(self.world, major);
                (known < natural_wonders_total(self.world))
                    .then_some(QuestKind::FindNaturalWonder { known })
            }
            Tag::GiveGold => {
                self.bullied_by_other(city_state, major)
                    .then_some(QuestKind::GiveGold {
                        gifted: self.world.module.city_states.major(major).gold_gifted,
                    })
            }
            Tag::PledgeToProtect => (self.bullied_by_other(city_state, major)
                && super::gifts::pledge_problem(self.world, city_state, major, true).is_none())
            .then_some(QuestKind::PledgeToProtect),
            Tag::BullyCityState => self
                .city_state_to_bully(city_state, major)
                .map(QuestKind::BullyCityState),
            Tag::DenounceMajor => self
                .major_to_denounce(city_state, major)
                .map(QuestKind::DenounceMajor),
            _ => None,
        }
    }

    /// Someone other than `major` bullied the city-state recently and
    /// `major` is not the last bully.
    fn bullied_by_other(&self, city_state: PlayerId, major: PlayerId) -> bool {
        last_bully(self.world, city_state).is_some_and(|(p, round)| {
            p != major && round + RECENT_BULLY_ROUNDS >= self.world.turn.round
        })
    }

    fn resource_for_quest(
        &mut self,
        major: PlayerId,
        rules: &CompiledRuleset,
    ) -> Option<ResourceId> {
        let techs = self.world.players.techs_of(major).to_vec();
        let map = &self.world.map;
        let mut present = vec![false; rules.resources.len()];
        for t in 0..map.tile_count() as u32 {
            if let Some(Some(r)) = map.resource(TileId(t)) {
                present[r.0 as usize] = true;
            }
        }
        let candidates: Vec<ResourceId> = (0..rules.resources.len())
            .map(|i| ResourceId(i as u16))
            .filter(|r| present[r.0 as usize])
            .filter(|r| {
                let def = &rules.resources[r.0 as usize];
                def.class.is_some_and(|c| c != ResourceClass::Bonus)
                    && def.required_tech.is_none_or(|t| techs.contains(&t))
            })
            .filter(|r| !has_resource(self.world, rules, major, *r))
            .collect();
        self.pick(&candidates)
    }

    fn wonder_for_quest(
        &mut self,
        major: PlayerId,
        rules: &CompiledRuleset,
    ) -> Option<BuildingKindId> {
        let techs = self.world.players.techs_of(major).to_vec();
        let candidates: Vec<BuildingKindId> = rules
            .buildings
            .iter()
            .enumerate()
            .filter(|(_, d)| d.kind == BuildingKind::WorldWonder)
            .filter(|(_, d)| d.required_tech.is_none_or(|t| techs.contains(&t)))
            .map(|(i, _)| BuildingKindId(i as u16))
            .filter(|b| built_anywhere(self.world, *b).is_none())
            .collect();
        self.pick(&candidates)
    }

    fn player_to_find(&mut self, major: PlayerId) -> Option<PlayerId> {
        let candidates: Vec<PlayerId> = majors(self.world)
            .into_iter()
            .filter(|p| *p != major && crate::systems::diplomacy::has_met(self.world, major, *p))
            .filter(|p| !found_territory(self.world, major, *p))
            .collect();
        self.pick(&candidates)
    }

    /// The nearest other city-state `major` has met.
    fn city_state_to_bully(&self, city_state: PlayerId, major: PlayerId) -> Option<PlayerId> {
        let from = capital_tile_any_of(self.world, city_state)?;
        let dims = self.world.map.dimensions;
        super::city_states(self.world)
            .into_iter()
            .filter(|cs| *cs != city_state && self.world.players.is_alive(*cs))
            .filter(|cs| crate::systems::diplomacy::has_met(self.world, major, *cs))
            .filter_map(|cs| {
                capital_tile_any_of(self.world, cs)
                    .and_then(|t| dims.distance(t, from))
                    .map(|d| (d, cs))
            })
            .min_by_key(|(d, cs)| (*d, cs.0))
            .map(|(_, cs)| cs)
    }

    fn major_to_denounce(&self, city_state: PlayerId, major: PlayerId) -> Option<PlayerId> {
        let (bully, round) = last_bully(self.world, city_state)?;
        let ok = bully != major
            && round + RECENT_BULLY_ROUNDS >= self.world.turn.round
            && self.world.players.is_alive(bully)
            && crate::systems::diplomacy::has_met(self.world, major, bully)
            && !self.world.diplomacy.at_war(major, bully)
            && !self
                .world
                .module
                .relations()
                .denounced(major, bully, self.world.turn.round);
        ok.then_some(bully)
    }

    fn pick<T: Copy>(&mut self, candidates: &[T]) -> Option<T> {
        if candidates.is_empty() {
            return None;
        }
        Some(candidates[self.world.rng.events.below(candidates.len() as u64) as usize])
    }

    fn start_quest(&mut self, city_state: PlayerId, major: PlayerId, kind: QuestKind) {
        let start = self.world.turn.round;
        let timed = matches!(
            kind,
            QuestKind::GiveGold { .. }
                | QuestKind::PledgeToProtect
                | QuestKind::BullyCityState(_)
                | QuestKind::DenounceMajor(_)
                | QuestKind::ContestTechs { .. }
                | QuestKind::Invest
        );
        let end = timed.then_some(start + CONTEST_LENGTH);
        self.world.module.city_states.quests.push(Quest {
            city_state,
            major,
            kind,
            start,
            end,
        });
        self.relations_changed(major);
    }

    /// Completed quests pay their reward (most influence first); void ones
    /// go without reward.
    pub(crate) fn check_quests(&mut self, city_state: PlayerId, rules: &CompiledRuleset) {
        let quests = std::mem::take(&mut self.world.module.city_states.quests);
        let mut done = Vec::new();
        let mut keep = Vec::with_capacity(quests.len());
        for q in quests {
            if q.city_state != city_state {
                keep.push(q);
            } else if complete(self.world, rules, &q) {
                done.push(q);
            } else if !void(self.world, &q) {
                keep.push(q);
            }
        }
        self.world.module.city_states.quests = keep;
        done.sort_by_key(|q| {
            (
                std::cmp::Reverse(
                    self.world
                        .module
                        .city_states
                        .standing(q.city_state, q.major)
                        .influence,
                ),
                q.major.0,
            )
        });
        for q in done {
            self.change_influence(q.city_state, q.major, reward(q.kind) * 100, rules);
        }
    }

    /// Contests end on their last round: the most technologies gained win.
    fn finish_contests(&mut self, city_state: PlayerId, rules: &CompiledRuleset) {
        let round = self.world.turn.round;
        let ending: Vec<Quest> = self
            .world
            .module
            .city_states
            .quests
            .iter()
            .filter(|q| q.city_state == city_state && q.end == Some(round))
            .filter(|q| matches!(q.kind, QuestKind::ContestTechs { .. } | QuestKind::Invest))
            .copied()
            .collect();
        let gained = |tx: &Self, q: &Quest| match q.kind {
            QuestKind::ContestTechs { start } => {
                tx.world.players.techs_of(q.major).len() as i32 - i32::from(start)
            }
            _ => 0,
        };
        let best = ending.iter().map(|q| gained(self, q)).max().unwrap_or(0);
        for q in &ending {
            let wins = matches!(q.kind, QuestKind::ContestTechs { .. }) && gained(self, q) == best;
            if wins {
                self.change_influence(q.city_state, q.major, reward(q.kind) * 100, rules);
            }
        }
        self.world.module.city_states.quests.retain(|q| {
            !(q.city_state == city_state && q.end == Some(round) && ending.contains(q))
        });
    }
}

fn tag_of(kind: QuestKind) -> Tag {
    match kind {
        QuestKind::KillCamp(_) => Tag::KillCamp,
        QuestKind::ConnectResource(_) => Tag::ConnectResource,
        QuestKind::ConstructWonder(_) => Tag::ConstructWonder,
        QuestKind::FindPlayer(_) => Tag::FindPlayer,
        QuestKind::FindNaturalWonder { .. } => Tag::FindNaturalWonder,
        QuestKind::GiveGold { .. } => Tag::GiveGold,
        QuestKind::PledgeToProtect => Tag::PledgeToProtect,
        QuestKind::BullyCityState(_) => Tag::BullyCityState,
        QuestKind::DenounceMajor(_) => Tag::DenounceMajor,
        QuestKind::ContestTechs { .. } => Tag::ContestTechs,
        QuestKind::Invest => Tag::Invest,
    }
}

fn capital_tile_any(auth: &AuthWorld, player: PlayerId) -> bool {
    !auth.indexes().cities_of(player).is_empty()
}

fn capital_tile_any_of(auth: &AuthWorld, player: PlayerId) -> Option<TileId> {
    auth.indexes()
        .cities_of(player)
        .first()
        .and_then(|c| auth.cities().get(*c))
        .map(|c| c.tile)
}
