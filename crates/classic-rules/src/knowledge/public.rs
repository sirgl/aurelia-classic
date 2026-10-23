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

    /// What kind of player the player is (civilization, city-state,
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

    /// The luxuries a civilization the player met could give in a deal
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
    /// civilizations it met, and every original capital the domination
    /// victory counts with its holder (its tile where the player explored
    /// it).
    fn race(&self) -> Race;

    /// The capitals of the victory progress screen alone (`race` without
    /// the scores).
    fn original_capitals(&self) -> Vec<CapitalStanding>;
}

/// A city-state's friend and ally bonuses for one civilization
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
    /// The player first, then the civilizations it met, by id.
    pub scores: Vec<(crate::ids::PlayerId, i64)>,
    pub capitals: Vec<CapitalStanding>,
}

/// An original capital the domination victory counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapitalStanding {
    /// The civilization that founded it.
    pub founder: crate::ids::PlayerId,
    pub holder: crate::ids::PlayerId,
    /// Where it is, when the player explored its tile.
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
    fn race(&self) -> Race {
        let auth = self.world(Sensor).auth;
        let me = self.player();
        let met = |p: crate::ids::PlayerId| self.contact(p).met_round.is_some() || auth.diplomacy().at_war(me, p);
        let players = auth.players();
        let rivals = (0..players.count() as u8).map(crate::ids::PlayerId).filter(|p| *p != me && players.is_major(*p) && players.is_alive(*p) && met(*p));
        let scores = std::iter::once(me).chain(rivals).map(|p| (p, crate::systems::victory::score(auth, self.rules(), p))).collect();
        Race { scores, capitals: self.original_capitals() }
    }

    /// One standing for every living major civilization (the screen lists them all, the
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

    /// What kind of player the player is (civilization, city-state,
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
}

fn attack_kind(view: &PlayerView<'_>, unit: UnitHandle) -> Option<AttackKind> {
    crate::systems::combat::validate::attack_kind(view.world(Sensor), view.rules(), unit)
}
