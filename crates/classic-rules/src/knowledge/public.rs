//! What a player may ask about combat through its view (the queries the
//! interface answers for a human: the combat preview of an attack on
//! a visible target, the promotions and upgrade of its own units). The AI
//! and the UI ask the same questions; nothing hidden goes through: the
//! attacker must be the player's own unit and the target tile visible to
//! the player.

use crate::combat::exchange::{plan_city_strike, AttackKind, CombatPreview};
use crate::ids::{CityHandle, PromotionId, TileId, UnitHandle};
use crate::knowledge::view::PlayerView;
use crate::knowledge::Sensor;

/// The classic questions a player asks through its view (`PlayerView`).
pub trait PlayerQueries {
    /// The preview of an attack of the player's `unit` on the visible
    /// `target` (ranged units shoot, the others charge). None when the
    /// attack is not possible now.
    fn attack_preview(&self, unit: UnitHandle, target: TileId) -> Option<CombatPreview>;

    /// The preview of the same attack as if the unit stood on `from`
    /// ready to strike (the interface's preview of a move and attack; a
    /// siege unit counted as set up). None when it could not strike the
    /// visible `target` from there.
    fn attack_preview_from(&self, unit: UnitHandle, from: TileId, target: TileId) -> Option<CombatPreview>;

    /// The move preview of the player's `unit` on a turn of its own begun on
    /// `from` (the interface's preview of a move order there, every move
    /// point fresh): the tiles it would reach and the move points left there,
    /// `from` included. The next turn's reach of a unit after a move; empty
    /// for a unit not the player's.
    fn own_reachable_from(&self, unit: UnitHandle, from: TileId) -> Vec<(TileId, u16)>;

    /// [`attack_preview_from`](Self::attack_preview_from) as if the unit had
    /// taken `promotion` first (an instant heal healed it; None: as it is):
    /// what a promotion would change in a strike, before it is chosen.
    fn promoted_attack_preview(&self, unit: UnitHandle, promotion: Option<PromotionId>, from: TileId, target: TileId) -> Option<CombatPreview>;

    /// The preview of the visible foreign `foe`'s strike from `from` on the
    /// player's `unit` where it stands, as if the unit had taken `promotion`
    /// (None: as it is): the danger a promotion changes. The foe's reach
    /// from `from` counts, not its moves left now (the strike of its next
    /// turn). None when the foe could not strike the unit from there.
    fn promoted_defense_preview(&self, unit: UnitHandle, promotion: Option<PromotionId>, foe: UnitHandle, from: TileId) -> Option<CombatPreview>;

    /// [`promoted_defense_preview`](Self::promoted_defense_preview) of the
    /// player's `unit` as if it stood on `at` (as it is now otherwise): the
    /// danger of a tile the unit may step to, the foe striking from `from`.
    fn defense_preview_at(&self, unit: UnitHandle, at: TileId, foe: UnitHandle, from: TileId) -> Option<CombatPreview>;

    /// What an air strike of the player's aircraft on the visible `target`
    /// is expected to cost it: the answer of the target and the
    /// interception by the best interceptor the player sees (the interface
    /// shows the same warning). None when the aircraft cannot strike
    /// `target` now.
    fn air_strike_risk(&self, unit: UnitHandle, target: TileId) -> Option<crate::combat::exchange::AirRisk>;

    /// Would the rules accept this command of the player now (the check
    /// the interface greys its buttons with)? Commands of other players
    /// are never accepted here.
    fn accepts(&self, command: &crate::command::Command) -> bool {
        self.refusal(command).is_none()
    }

    /// Why the rules refuse the command of the player now (None: they
    /// accept it); `accepts` is `refusal(..).is_none()`.
    fn refusal(&self, command: &crate::command::Command) -> Option<crate::command::CommandError>;

    /// What kind of player the player is (nation, city-state,
    /// barbarians).
    fn own_kind(&self) -> crate::world::player::PlayerKind;

    /// Aircraft and missiles based on the player's carrier, and how many it
    /// may carry (the unit panel shows both).
    fn cargo_aboard(&self, carrier: UnitHandle) -> (usize, i64);

    /// How far the player's unit can paradrop from a city or fort (0: it
    /// cannot; the interface shows the range on the unit's panel).
    fn paradrop_range(&self, unit: UnitHandle) -> i32;

    /// The player's siege unit must set up before it can shoot.
    fn must_set_up_first(&self, unit: UnitHandle) -> bool;

    /// How the player's unit strikes a tile it is ordered to attack
    /// (`AttackUnit`, `AttackCity`): a ranged unit or an aircraft shoots,
    /// the others charge (`systems::combat::validate::attack_kind`). None
    /// for a unit not the player's.
    fn own_attack_kind(&self, unit: UnitHandle) -> Option<AttackKind>;

    /// The player's unit takes a city its blow brings to 0 hit points: it
    /// charges (a shot leaves a city at 1 HP), lacks `NoCapture`, and the
    /// player is no barbarian (barbarians take a ransom).
    fn captures_cities(&self, unit: UnitHandle) -> bool;

    /// Why `item` cannot pass from `giver` to `receiver` now (None: it can).
    fn deal_item_problem(
        &self,
        giver: crate::ids::PlayerId,
        receiver: crate::ids::PlayerId,
        item: crate::world::relations::DealItem,
    ) -> Option<crate::systems::diplomacy::deals::DealBlock>;

    /// Why the player's offer of `items` to `to` cannot pass now (None: it
    /// can) — the whole deal, so gold may pass at war beside peace.
    fn offer_problem(
        &self,
        to: crate::ids::PlayerId,
        items: &[(crate::ids::PlayerId, crate::world::relations::DealItem)],
    ) -> Option<&'static str>;

    /// Expected damage of the player's city striking the visible `target`.
    fn city_strike_damage(&self, city: CityHandle, target: TileId) -> Option<u16>;

    /// The preview of the player's city striking the visible `target`
    /// (strengths, damage range, verdict), as the odds panel shows it.
    fn city_strike_preview(&self, city: CityHandle, target: TileId) -> Option<CombatPreview>;

    /// A great engineer may hurry the head of the player's own `city` (a
    /// building, wonders included; false for another player's city).
    fn engineer_may_hurry(&self, city: CityHandle) -> bool;

    /// Promotions the player's unit may take now.
    fn promotions_available(&self, unit: UnitHandle) -> Vec<PromotionId>;

    /// The upgrade of the player's unit now: target kind and gold price.
    fn upgrade_offer(&self, unit: UnitHandle) -> Option<(crate::ids::UnitKindId, u32)>;

    /// The luxuries a nation the player met could give in a deal
    /// now, with their tradeable copies (the trade screen lists them);
    /// empty for a player not met.
    fn tradeable_luxuries(&self, other: crate::ids::PlayerId) -> Vec<(crate::ids::ResourceId, i32)>;

    /// The routes a new trade unit of `kind` in the player's own `city`
    /// could start now (`derived::trade::prospects`); empty for another
    /// player's city.
    fn trade_prospects(&self, city: CityHandle, kind: crate::ids::UnitKindId) -> Vec<crate::derived::trade::TradeOffer>;

    /// What a city-state gives the player as a friend and as an ally now,
    /// with the influence each needs (the city-state screen shows them).
    fn city_state_terms(&self, city_state: crate::ids::PlayerId) -> Option<CityStateTerms>;

    /// The victory progress screen: the scores of the player and of the
    /// nations it met, and every original capital the domination
    /// victory counts with its holder (its tile where the player explored
    /// it).
    fn race(&self) -> Race;

    /// The capitals of the victory progress screen alone (`race` without
    /// the scores).
    fn original_capitals(&self) -> Vec<CapitalStanding>;

    /// Every world wonder built, as the game announces it to every player:
    /// the wonder always; its builder (the city's owner now) when the
    /// player has met that nation, else "a far-away land"; the city's tile
    /// when the player sees the city or remembers it. By wonder id.
    fn wonder_news(&self) -> Vec<WonderNews>;

    /// The era (`player_era`: the latest era of its technologies) of every
    /// living nation the player has met, as the game announces a nation
    /// entering an era to those who know it. By player id.
    fn met_eras(&self) -> Vec<(crate::ids::PlayerId, u8)>;

    /// The faith price of `item` in the player's own `city` when the player
    /// may buy it with faith now (the city screen's faith button); None
    /// otherwise, and for another player's city.
    fn faith_price(&self, city: CityHandle, item: crate::ids::ProductionItem) -> Option<u32>;

    /// The player's own `city` as it would be with `building` (the yields, hit
    /// points and strength the city screen's building panel shows it adds);
    /// None for another player's city. Unciv's construction AI ranks a building
    /// by this difference (`ai::unciv`).
    fn building_preview(&self, city: CityHandle, building: crate::ids::BuildingKindId) -> Option<BuildingPreview>;

    /// The command as a tree of named values (`kernel::replay::codec::Value`):
    /// what a dataset or a tool reads.
    fn command_value(&self, command: &crate::command::Command) -> kernel::replay::codec::Value;

    /// The command a tree names (`command_value` backwards); an unknown
    /// variant, field or entity is an error.
    fn command_from_value(
        &self,
        value: &kernel::replay::codec::Value,
    ) -> Result<crate::command::Command, kernel::persistence::codec::CodecError>;
}

/// A city with and without a building (`PlayerQueries::building_preview`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildingPreview {
    pub now: crate::derived::income::CityIncome,
    pub with: crate::derived::income::CityIncome,
    pub max_hp_now: u16,
    pub max_hp_with: u16,
    pub strength_now: i64,
    pub strength_with: i64,
}

/// A city-state's friend and ally bonuses for one nation
/// (`PlayerQueries::city_state_terms`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CityStateTerms {
    pub friends_at: i32,
    pub allies_at: i32,
    pub friends: CityStateBonuses,
    /// The ally's bonuses on top of the friend's.
    pub allies: CityStateBonuses,
}

/// Bonuses a turn of one level (food in hundredths).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CityStateBonuses {
    pub capital_food: i32,
    pub city_food: i32,
    pub culture: i32,
    pub faith: i32,
    pub happiness: i32,
}

/// The victory progress screen of one player (shown to everyone:
/// the score list and who holds each original capital).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Race {
    /// The player first, then the nations it met, by id.
    pub scores: Vec<(crate::ids::PlayerId, i64)>,
    pub capitals: Vec<CapitalStanding>,
}

/// An original capital the domination victory counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapitalStanding {
    /// The nation that founded it.
    pub founder: crate::ids::PlayerId,
    pub holder: crate::ids::PlayerId,
    /// Where it is, when the player explored its tile.
    pub tile: Option<TileId>,
}

/// A world wonder as the announcement tells it
/// (`PlayerQueries::wonder_news`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WonderNews {
    pub building: crate::ids::BuildingKindId,
    /// The nation whose city holds it, when the player has met it.
    pub builder: Option<crate::ids::PlayerId>,
    /// The city's tile, when the player sees or remembers that city.
    pub tile: Option<TileId>,
}

impl Race {
    /// The player's own score.
    pub fn own_score(&self) -> i64 {
        self.scores.first().map_or(0, |(_, s)| *s)
    }

    /// The best score of the others the player met (0: none met).
    pub fn best_rival(&self) -> i64 {
        self.scores.iter().skip(1).map(|(_, s)| *s).max().unwrap_or(0)
    }
}

impl PlayerQueries for PlayerView<'_> {
    fn wonder_news(&self) -> Vec<WonderNews> {
        let auth = self.world(Sensor).auth;
        let rules = self.rules();
        let me = self.player();
        let met = |p: crate::ids::PlayerId| p == me || self.contact(p).met_round.is_some() || auth.diplomacy().at_war(me, p);
        let known_city = |tile: TileId| self.is_visible(tile) || self.remembered_cities().any(|(t, _)| t == tile);
        let mut out: Vec<WonderNews> = Vec::new();
        for (_, city) in auth.cities().iter() {
            for &building in &city.buildings {
                let wonder = rules.buildings.get(usize::from(building.0)).is_some_and(|d| d.wonder && d.kind != crate::rules::content::BuildingKind::NationalWonder);
                if wonder && !out.iter().any(|w| w.building == building) {
                    out.push(WonderNews { building, builder: met(city.owner).then_some(city.owner), tile: known_city(city.tile).then_some(city.tile) });
                }
            }
        }
        out.sort_by_key(|w| w.building.0);
        out
    }

    fn met_eras(&self) -> Vec<(crate::ids::PlayerId, u8)> {
        let auth = self.world(Sensor).auth;
        let me = self.player();
        let players = auth.players();
        (0..players.count() as u8)
            .map(crate::ids::PlayerId)
            .filter(|p| *p != me && players.is_major(*p) && players.is_alive(*p))
            .filter(|p| self.contact(*p).met_round.is_some() || auth.diplomacy().at_war(me, *p))
            .map(|p| (p, crate::world::player::player_era(auth, self.rules(), p)))
            .collect()
    }

    fn race(&self) -> Race {
        let auth = self.world(Sensor).auth;
        let me = self.player();
        let met = |p: crate::ids::PlayerId| self.contact(p).met_round.is_some() || auth.diplomacy().at_war(me, p);
        let players = auth.players();
        let rivals = (0..players.count() as u8).map(crate::ids::PlayerId).filter(|p| *p != me && players.is_major(*p) && players.is_alive(*p) && met(*p));
        let scores = std::iter::once(me).chain(rivals).map(|p| (p, crate::systems::victory::score(auth, self.rules(), p))).collect();
        Race { scores, capitals: self.original_capitals() }
    }

    /// One standing for every living major nation (the screen lists them all, the
    /// count is a constant of the match). Who holds a capital, and where it is, only as far as
    /// the player has SEEN that city: standing in sight now, or as last sighted. Otherwise the
    /// founder is taken to hold it and the tile is unknown. Nothing here depends on whether the
    /// capital has been founded yet, or on who took it behind the fog.
    fn original_capitals(&self) -> Vec<CapitalStanding> {
        let auth = self.world(Sensor).auth;
        let players = auth.players();
        let remembered: Vec<(TileId, crate::ids::PlayerId)> = self.remembered_cities().map(|(tile, s)| (tile, s.owner)).collect();
        let seen_holder = |tile: TileId, truth: crate::ids::PlayerId| -> Option<crate::ids::PlayerId> {
            if self.is_visible(tile) {
                Some(truth)
            } else {
                remembered.iter().find(|(t, _)| *t == tile).map(|(_, owner)| *owner)
            }
        };
        (0..players.count() as u8)
            .map(crate::ids::PlayerId)
            .filter(|f| players.is_major(*f))
            .filter_map(|founder| {
                let city = auth.cities().iter().map(|(_, c)| c).find(|c| c.original_capital && c.original_owner == founder);
                match city.and_then(|c| seen_holder(c.tile, c.owner).map(|holder| (c.tile, holder))) {
                    Some((tile, holder)) => players.is_alive(holder).then_some(CapitalStanding { founder, holder, tile: Some(tile) }),
                    None => players.is_alive(founder).then_some(CapitalStanding { founder, holder: founder, tile: None }),
                }
            })
            .collect()
    }

    /// The preview of an attack of the player's `unit` on the visible
    /// `target` (ranged units shoot, the others charge). None when the
    /// attack is not possible now.
    fn attack_preview(&self, unit: UnitHandle, target: TileId) -> Option<CombatPreview> {
        let world = self.world(Sensor);
        let core = world.auth.units().get(unit)?.core;
        if core.owner != self.player() || !self.is_visible(target) {
            return None;
        }
        let kind = attack_kind(self, unit)?;
        let plan = crate::combat::plan_attack(world, self.rules(), unit, target, kind).ok()?;
        Some(plan.preview())
    }

    fn attack_preview_from(&self, unit: UnitHandle, from: TileId, target: TileId) -> Option<CombatPreview> {
        let world = self.world(Sensor);
        let core = world.auth.units().get(unit)?.core;
        if core.owner != self.player() || !self.is_visible(target) {
            return None;
        }
        let kind = attack_kind(self, unit)?;
        let plan = crate::combat::exchange::plan_attack_from(world, self.rules(), unit, from, target, kind).ok()?;
        Some(plan.preview())
    }

    fn own_reachable_from(&self, unit: UnitHandle, from: TileId) -> Vec<(TileId, u16)> {
        let world = self.world(Sensor);
        if world.auth.units().get(unit).is_none_or(|u| u.core.owner != self.player()) {
            return Vec::new();
        }
        let Some(mut request) = crate::navigation::PathRequest::for_unit(world.auth, self.rules(), unit, from) else {
            return Vec::new();
        };
        request.from = from;
        // A fresh turn: the start's whole allowance (`Allowance::start` caps it).
        request.budget.left = u16::MAX;
        crate::navigation::reachable_this_turn(world, self.rules(), &request)
    }

    fn promoted_attack_preview(&self, unit: UnitHandle, promotion: Option<PromotionId>, from: TileId, target: TileId) -> Option<CombatPreview> {
        let world = self.world(Sensor);
        let core = *world.auth.units().get(unit)?.core;
        if core.owner != self.player() || !self.is_visible(target) {
            return None;
        }
        let kind = attack_kind(self, unit)?;
        let as_if = match promotion {
            Some(p) => crate::combat::experience::promoted(self.rules(), &core, p),
            None => core,
        };
        let plan = crate::combat::exchange::plan_attack_as(world, self.rules(), unit, as_if, from, target, kind).ok()?;
        Some(plan.preview())
    }

    fn promoted_defense_preview(&self, unit: UnitHandle, promotion: Option<PromotionId>, foe: UnitHandle, from: TileId) -> Option<CombatPreview> {
        let world = self.world(Sensor);
        let core = *world.auth.units().get(unit)?.core;
        let theirs = *world.auth.units().get(foe)?.core;
        if core.owner != self.player() || theirs.owner == self.player() || !self.is_visible(theirs.tile) {
            return None;
        }
        let def = self.rules().units.get(theirs.kind.0 as usize)?;
        let kind = if def.ranged_strength > 0 && def.range > 0 { AttackKind::Ranged } else { AttackKind::Melee };
        let as_if = match promotion {
            Some(p) => crate::combat::experience::promoted(self.rules(), &core, p),
            None => core,
        };
        let plan = crate::combat::exchange::plan_strike_on(world, self.rules(), foe, from, unit, as_if, kind).ok()?;
        Some(plan.preview())
    }

    fn defense_preview_at(&self, unit: UnitHandle, at: TileId, foe: UnitHandle, from: TileId) -> Option<CombatPreview> {
        let world = self.world(Sensor);
        let mut core = *world.auth.units().get(unit)?.core;
        let theirs = *world.auth.units().get(foe)?.core;
        if core.owner != self.player() || theirs.owner == self.player() || !self.is_visible(theirs.tile) {
            return None;
        }
        let def = self.rules().units.get(theirs.kind.0 as usize)?;
        let kind = if def.ranged_strength > 0 && def.range > 0 { AttackKind::Ranged } else { AttackKind::Melee };
        core.tile = at;
        let plan = crate::combat::exchange::plan_strike_on(world, self.rules(), foe, from, unit, core, kind).ok()?;
        Some(plan.preview())
    }

    fn air_strike_risk(&self, unit: UnitHandle, target: TileId) -> Option<crate::combat::exchange::AirRisk> {
        let world = self.world(Sensor);
        let core = world.auth.units().get(unit)?.core;
        if core.owner != self.player() || !self.is_visible(target) {
            return None;
        }
        let plan = crate::combat::plan_attack(world, self.rules(), unit, target, AttackKind::Ranged).ok()?;
        plan.air.then(|| crate::systems::combat::air::strike_risk(world, self.rules(), &plan))
    }

    /// Would the rules accept this command of the player now (the check
    /// the interface greys its buttons with)? Commands of other players
    /// are never accepted here.
    fn refusal(&self, command: &crate::command::Command) -> Option<crate::command::CommandError> {
        let actor = crate::command::validate::acting_player(self.world(Sensor), command);
        if actor.is_some_and(|a| a != self.player()) {
            return Some(crate::command::CommandError::NotOwner);
        }
        let mut scratch = crate::navigation::PathScratch::default();
        let world = self.world(Sensor);
        if let Err(error) = crate::command::validate::validate(world, self.rules(), command.clone(), &mut scratch) {
            return Some(error);
        }
        // `validate` of a march is cheap on purpose (the route is found when
        // the order runs); a march that has no route is refused there, so it
        // is refused here too.
        if let crate::command::Command::MarchTo { unit, destination } = *command {
            let routed = crate::navigation::PathRequest::for_unit(world.auth, self.rules(), unit, destination)
                .is_some_and(|request| crate::navigation::find_path(world, self.rules(), request, &mut scratch).is_some());
            if !routed {
                return Some(crate::command::CommandError::RuleViolation("unreachable".into()));
            }
        }
        None
    }

    /// What kind of player the player is (nation, city-state,
    /// barbarians).
    fn own_kind(&self) -> crate::world::player::PlayerKind {
        self.world(Sensor).auth.players().kind_of(self.player())
    }

    fn cargo_aboard(&self, carrier: UnitHandle) -> (usize, i64) {
        let auth = self.world(Sensor).auth;
        match auth.units().get(carrier) {
            Some(view) if view.core.owner == self.player() => (
                crate::systems::cargo::cargo_of(auth, carrier).len(),
                crate::systems::cargo::cargo_space(auth, self.rules(), carrier),
            ),
            _ => (0, 0),
        }
    }

    fn paradrop_range(&self, unit: UnitHandle) -> i32 {
        let auth = self.world(Sensor).auth;
        match auth.units().get(unit) {
            Some(view) if view.core.owner == self.player() => crate::systems::paradrop::paradrop_range(auth, self.rules(), unit),
            _ => 0,
        }
    }

    /// The player's siege unit must set up before it can shoot (and is
    /// not set up yet).
    fn must_set_up_first(&self, unit: UnitHandle) -> bool {
        let auth = self.world(Sensor).auth;
        let Some(core) = auth.units().get(unit).map(|u| u.core) else {
            return false;
        };
        if core.owner != self.player() || core.stance == crate::world::unit::Stance::SetUp {
            return false;
        }
        crate::effects::unit::unit_modifiers(auth, self.rules(), &core)
            .has(crate::rules::effects::UnitAbility::MustSetUp)
    }

    fn own_attack_kind(&self, unit: UnitHandle) -> Option<AttackKind> {
        let auth = self.world(Sensor).auth;
        let core = auth.units().get(unit)?.core;
        if core.owner != self.player() {
            return None;
        }
        attack_kind(self, unit)
    }

    fn captures_cities(&self, unit: UnitHandle) -> bool {
        let auth = self.world(Sensor).auth;
        let Some(core) = auth.units().get(unit).map(|u| *u.core) else {
            return false;
        };
        if core.owner != self.player() || attack_kind(self, unit) != Some(AttackKind::Melee) {
            return false;
        }
        if crate::world::player::ClassicPlayers::is_barbarian(auth.players(), core.owner) {
            return false;
        }
        !crate::effects::unit::unit_modifiers(auth, self.rules(), &core).has(crate::rules::effects::UnitAbility::NoCapture)
    }

    /// Why `item` cannot pass from `giver` to `receiver` now (None: it can);
    /// one of them must be the player (the trade screen greys items out
    /// by this).
    fn deal_item_problem(
        &self,
        giver: crate::ids::PlayerId,
        receiver: crate::ids::PlayerId,
        item: crate::world::relations::DealItem,
    ) -> Option<crate::systems::diplomacy::deals::DealBlock> {
        if giver != self.player() && receiver != self.player() {
            return Some(crate::systems::diplomacy::deals::DealBlock::NotOurDeal);
        }
        let auth = self.world(Sensor).auth;
        crate::systems::diplomacy::deals::item_problem(auth, self.rules(), giver, receiver, item, false)
    }

    /// Why the player's offer of `items` to `to` cannot pass now (the
    /// trade screen greys the deal out by this).
    fn offer_problem(
        &self,
        to: crate::ids::PlayerId,
        items: &[(crate::ids::PlayerId, crate::world::relations::DealItem)],
    ) -> Option<&'static str> {
        crate::systems::diplomacy::deals::offer_problem(self.world(Sensor).auth, self.rules(), self.player(), to, items)
    }

    /// Expected damage of the player's city striking the visible `target`.
    fn city_strike_damage(&self, city: CityHandle, target: TileId) -> Option<u16> {
        let world = self.world(Sensor);
        let core = world.auth.cities().get(city)?;
        if core.owner != self.player() || !self.is_visible(target) {
            return None;
        }
        let plan = plan_city_strike(world, self.rules(), city, target).ok()?;
        Some(plan.damage(crate::combat::damage::Roll::Expected))
    }

    fn city_strike_preview(&self, city: CityHandle, target: TileId) -> Option<CombatPreview> {
        let world = self.world(Sensor);
        let core = world.auth.cities().get(city)?;
        if core.owner != self.player() || !self.is_visible(target) {
            return None;
        }
        let plan = plan_city_strike(world, self.rules(), city, target).ok()?;
        let max = crate::combat::city::max_hit_points(world.auth, self.rules(), core);
        Some(plan.preview(max.saturating_sub(core.damage)))
    }

    fn engineer_may_hurry(&self, city: CityHandle) -> bool {
        let auth = self.world(Sensor).auth;
        auth.cities()
            .get(city)
            .is_some_and(|core| core.owner == self.player() && crate::derived::great_people::engineer_may_hurry(auth, self.rules(), core))
    }

    /// Promotions the player's unit may take now (empty without enough
    /// experience).
    fn promotions_available(&self, unit: UnitHandle) -> Vec<PromotionId> {
        let auth = self.world(Sensor).auth;
        let Some(view) = auth.units().get(unit) else {
            return Vec::new();
        };
        let core = view.core;
        if core.owner != self.player() {
            return Vec::new();
        }
        if core.experience < crate::combat::experience::experience_needed_by(auth, self.rules(), core) {
            return Vec::new();
        }
        crate::combat::experience::available_promotions(auth, self.rules(), core)
    }

    /// The upgrade of the player's unit now: target kind and gold price.
    fn upgrade_offer(&self, unit: UnitHandle) -> Option<(crate::ids::UnitKindId, u32)> {
        let auth = self.world(Sensor).auth;
        let core = auth.units().get(unit)?.core;
        if core.owner != self.player() {
            return None;
        }
        let offer =
            crate::systems::combat::upgrade::upgrade_offer(auth, self.rules(), core).ok()?;
        Some((offer.to, offer.price))
    }

    fn tradeable_luxuries(&self, other: crate::ids::PlayerId) -> Vec<(crate::ids::ResourceId, i32)> {
        let auth = self.world(Sensor).auth;
        if other == self.player() || !crate::derived::tourism::met(auth, self.player(), other) {
            return Vec::new();
        }
        let ledger = crate::derived::resources::resource_ledger(auth, self.rules(), other);
        let luxury = |r: &crate::ids::ResourceId| {
            self.rules().resources.get(r.0 as usize).is_some_and(|d| d.class == Some(crate::rules::content::ResourceClass::Luxury))
        };
        ledger.known().filter(luxury).map(|r| (r, ledger.tradeable(r))).filter(|(_, n)| *n > 0).collect()
    }

    fn city_state_terms(&self, city_state: crate::ids::PlayerId) -> Option<CityStateTerms> {
        use crate::rules::content::{CityStateBonusKind as K, CityStateLevel};
        let auth = self.world(Sensor).auth;
        let rules = self.rules();
        let def = crate::systems::city_states::type_of(auth, rules, city_state)?;
        let era = crate::combat::city::current_era(auth, rules, self.player()).map_or(0, |e| e.index);
        let level = |level: CityStateLevel| {
            let amount = |kind: K| crate::systems::city_states::bonus_amount(rules, def, level, kind, era);
            CityStateBonuses {
                capital_food: amount(K::CapitalFood),
                city_food: amount(K::AllCitiesFood),
                culture: amount(K::Culture),
                faith: amount(K::Faith),
                happiness: amount(K::Happiness),
            }
        };
        Some(CityStateTerms {
            friends_at: def.friends_threshold,
            allies_at: def.allies_threshold,
            friends: level(CityStateLevel::Friends),
            allies: level(CityStateLevel::Allies),
        })
    }

    fn trade_prospects(&self, city: CityHandle, kind: crate::ids::UnitKindId) -> Vec<crate::derived::trade::TradeOffer> {
        let auth = self.world(Sensor).auth;
        match auth.cities().get(city) {
            Some(core) if core.owner == self.player() => crate::derived::trade::prospects(auth, self.rules(), city, kind),
            _ => Vec::new(),
        }
    }

    fn faith_price(&self, city: CityHandle, item: crate::ids::ProductionItem) -> Option<u32> {
        let auth = self.world(Sensor).auth;
        crate::derived::religion::faith_purchase_status(auth, self.rules(), self.player(), city, item).ok()
    }

    fn building_preview(&self, city: CityHandle, building: crate::ids::BuildingKindId) -> Option<BuildingPreview> {
        let auth = self.world(Sensor).auth;
        let rules = self.rules();
        let core = auth.cities().get(city)?;
        if core.owner != self.player() {
            return None;
        }
        let mut with = core.clone();
        if !with.buildings.contains(&building) {
            with.buildings.push(building);
        }
        Some(BuildingPreview {
            now: crate::derived::income::city_income(auth, rules, core),
            with: crate::derived::income::city_income(auth, rules, &with),
            max_hp_now: crate::combat::city::max_hit_points(auth, rules, core),
            max_hp_with: crate::combat::city::max_hit_points(auth, rules, &with),
            strength_now: crate::combat::city::city_strength(auth, rules, core),
            strength_with: crate::combat::city::city_strength(auth, rules, &with),
        })
    }

    fn command_value(&self, command: &crate::command::Command) -> kernel::replay::codec::Value {
        kernel::replay::codec::CommandField::value(command, self.world(Sensor).auth)
    }

    fn command_from_value(
        &self,
        value: &kernel::replay::codec::Value,
    ) -> Result<crate::command::Command, kernel::persistence::codec::CodecError> {
        kernel::replay::codec::CommandField::from_value(value, self.world(Sensor).auth)
    }
}

fn attack_kind(view: &PlayerView<'_>, unit: UnitHandle) -> Option<AttackKind> {
    crate::systems::combat::validate::attack_kind(view.world(Sensor), view.rules(), unit)
}
