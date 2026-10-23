//! Unit orders: which buttons the selected unit has, why some are blocked, and what they do.
use client::selection::Selection;
use classic_rules::command::CommandError;
use classic_rules::effects::unit::unit_modifiers;
use classic_rules::derived::great_people::{great_person_of, GreatPersonAction};
use classic_rules::ids::{ImprovementId, PlayerId, TileId, UnitHandle};
use classic_rules::rules::content::UnitAbility;
use classic_rules::world::tourism::GreatWorkClass;
use classic_rules::world::unit::{UnitCore, UNIT_MAX_HP};
use classic_rules::Command;

use i18n::{t, tf};

use super::names;
use super::reasons;
use super::session::Session;
use crate::ui::model::{Action, ActionState, Availability, NoticeKind, UnitOrders};

const SETTLER: &[Action] = &[Action::Move, Action::FoundCity, Action::Skip, Action::Sleep];
const WORKER: &[Action] = &[
    Action::Move,
    Action::Improve,
    Action::Road,
    Action::Chop,
    Action::Skip,
    Action::Sleep,
];
const GREAT_PERSON: &[Action] = &[
    Action::Move,
    Action::GreatImprovement,
    Action::Discover,
    Action::HurryWonder,
    Action::GoldenAge,
    Action::TradeMission,
    Action::Skip,
    Action::Sleep,
];
const MISSIONARY: &[Action] = &[Action::Move, Action::Spread, Action::Skip, Action::Sleep];
const INQUISITOR: &[Action] = &[Action::Move, Action::RemoveHeresy, Action::Skip, Action::Sleep];
const PROPHET: &[Action] = &[
    Action::Move,
    Action::Spread,
    Action::HolySite,
    Action::Skip,
    Action::Sleep,
];
const WRITER: &[Action] = &[
    Action::Move,
    Action::GreatWork,
    Action::Treatise,
    Action::Skip,
    Action::Sleep,
];
const ARTIST: &[Action] = &[
    Action::Move,
    Action::GreatWork,
    Action::GoldenAge,
    Action::Skip,
    Action::Sleep,
];
const MUSICIAN: &[Action] = &[
    Action::Move,
    Action::GreatWork,
    Action::Concert,
    Action::Skip,
    Action::Sleep,
];
const ARCHAEOLOGIST: &[Action] = &[Action::Move, Action::Excavate, Action::Skip, Action::Sleep];
const CIVILIAN: &[Action] = &[Action::Move, Action::Skip, Action::Sleep];
const TRADE: &[Action] = &[Action::TradeRoute, Action::Skip, Action::Sleep];
const MILITARY: &[Action] = &[
    Action::Move,
    Action::Attack,
    Action::Fortify,
    Action::Heal,
    Action::Skip,
    Action::Sleep,
];
/// A combat unit that also builds (the Legion's roads and forts, the
/// Samurai's fishing boats).
const MILITARY_BUILDER: &[Action] = &[
    Action::Move,
    Action::Attack,
    Action::Improve,
    Action::Road,
    Action::Fortify,
    Action::Heal,
    Action::Skip,
    Action::Sleep,
];

/// The Merchant of Venice's order.
fn buy_city_state(unit: UnitHandle) -> Command {
    Command::GreatPersonAction { unit, action: GreatPersonAction::BuyCityState }
}

impl Session {
    pub fn unit_orders(&mut self) -> Option<UnitOrders> {
        let (unit, core) = self.selected_unit()?;
        let core = *core;
        let mut list = self.actions_for(&core).to_vec();
        list.extend(self.situational_actions(unit));
        let actions = list
            .iter()
            .map(|action| self.action_state(unit, &core, *action))
            .collect();
        Some(UnitOrders {
            unit,
            tile: core.tile.0,
            name: names::unit(self.rules(), core.kind),
            status: self.unit_status(&core),
            actions,
            targeting: self.targeting,
            promotions: self.promotion_choices(unit),
        })
    }

    fn actions_for(&self, core: &UnitCore) -> &'static [Action] {
        let rules = self.rules();
        let Some(def) = rules.units.get(core.kind.0 as usize) else {
            return MILITARY;
        };
        let has = |a: UnitAbility| def.module.abilities.contains(&a);
        match classic_rules::derived::tourism::work_class(rules, core.kind) {
            Some(GreatWorkClass::Literature) => return WRITER,
            Some(GreatWorkClass::Art) => return ARTIST,
            Some(GreatWorkClass::Music) => return MUSICIAN,
            _ => {}
        }
        if def.module.economy.trade {
            TRADE
        } else if def.founder {
            SETTLER
        } else if def.builder {
            WORKER
        } else if has(UnitAbility::FoundReligion) {
            PROPHET
        } else if has(UnitAbility::SpreadReligion) {
            MISSIONARY
        } else if has(UnitAbility::RemoveHeresy) {
            INQUISITOR
        } else if has(UnitAbility::Excavates) {
            ARCHAEOLOGIST
        } else if great_person_of(rules, core.kind).is_some() {
            GREAT_PERSON
        } else if rules.is_civilian(core.kind) {
            CIVILIAN
        } else if def.module.builds_roads || !def.module.builds.is_empty() {
            MILITARY_BUILDER
        } else {
            MILITARY
        }
    }

    /// The great person command behind an action button (Construct: the
    /// first of its improvements the core accepts here).
    fn great_person_command(&mut self, unit: UnitHandle, core: &UnitCore, action: Action) -> Option<Command> {
        let action = match action {
            Action::Discover => GreatPersonAction::DiscoverTechnology,
            Action::HurryWonder => GreatPersonAction::HurryProduction,
            Action::GoldenAge => GreatPersonAction::StartGoldenAge,
            Action::TradeMission => GreatPersonAction::TradeMission,
            Action::GreatImprovement => {
                let improvements = great_person_of(self.rules(), core.kind)?.improvements.clone();
                let accepted = improvements.iter().copied().find(|i| {
                    let command = Command::GreatPersonAction { unit, action: GreatPersonAction::Construct(*i) };
                    self.check(&command).is_ok()
                });
                GreatPersonAction::Construct(accepted.or(improvements.first().copied())?)
            }
            _ => return None,
        };
        Some(Command::GreatPersonAction { unit, action })
    }

    fn great_person_state(&mut self, unit: UnitHandle, core: &UnitCore, action: Action) -> (Availability, Option<String>) {
        let Some(command) = self.great_person_command(unit, core, action) else {
            return (Availability::Blocked(reasons::not_for_this_person().into()), None);
        };
        let detail = match command {
            Command::GreatPersonAction { action: GreatPersonAction::Construct(i), .. } => {
                let def = &self.rules().improvements[i.0 as usize];
                Some(tf!("classic.game.orders.build", name = names::named("improvement", &def.name, &def.key)))
            }
            _ => None,
        };
        (self.availability(&command), detail)
    }

    fn unit_status(&self, core: &UnitCore) -> String {
        if let Some(work) = core.work {
            let turns = classic_rules::world::builds::job_turns_left(self.game().view().auth, self.rules(), core.owner, &work);
            return tf!("classic.game.orders.status-working", n = turns);
        }
        let auth = self.game().view().auth;
        let afloat = unit_modifiers(auth, self.rules(), core).embarked_on(self.rules(), auth.map(), core.kind, core.tile);
        let (moves_left, moves_max) = (moves_text(core.moves_left), moves_text(core.moves_max));
        if core.stance == classic_rules::world::unit::Stance::Intercept && !afloat {
            return tf!("classic.game.orders.status-intercepting", hp = core.hp, left = moves_left, max = moves_max);
        }
        match (afloat, core.stance.is_fortified()) {
            (false, false) => tf!("classic.game.orders.status", hp = core.hp, left = moves_left, max = moves_max),
            (true, false) => tf!("classic.game.orders.status-embarked", hp = core.hp, left = moves_left, max = moves_max),
            (false, true) => tf!("classic.game.orders.status-fortified", hp = core.hp, left = moves_left, max = moves_max),
            (true, true) => tf!("classic.game.orders.status-embarked-fortified", hp = core.hp, left = moves_left, max = moves_max),
        }
    }

    fn action_state(&mut self, unit: UnitHandle, core: &UnitCore, action: Action) -> ActionState {
        let (availability, detail) = match action {
            Action::Move | Action::Attack => (needs_moves(core), None),
            Action::Fortify => (self.availability(&Command::Fortify { unit }), None),
            Action::Heal => (self.heal_availability(unit, core), None),
            Action::Skip | Action::Sleep => (Availability::Ready, None),
            Action::FoundCity => (
                self.availability(&Command::FoundCity { settler: unit }),
                None,
            ),
            Action::Improve => self.improvement_choice(unit),
            Action::Road => (self.availability(&Command::BuildRoad { unit }), None),
            Action::Chop => self.removal_choice(unit, core),
            Action::Spread => (self.availability(&Command::SpreadReligion { unit }), None),
            Action::RemoveHeresy => (self.availability(&Command::RemoveHeresy { unit }), None),
            Action::HolySite => (self.availability(&Command::CreateGreatImprovement { unit }), None),
            Action::GreatWork => (self.availability(&Command::CreateGreatWork { unit }), None),
            Action::Treatise => (self.availability(&Command::WriteTreatise { unit }), None),
            Action::Concert => (self.availability(&Command::PerformConcert { unit }), None),
            Action::Excavate => (self.availability(&Command::Excavate { unit }), None),
            Action::GreatImprovement
            | Action::Discover
            | Action::HurryWonder
            | Action::GoldenAge
            | Action::TradeMission => self.great_person_state(unit, core, action),
            Action::Promote
            | Action::Upgrade
            | Action::SetUp
            | Action::Pillage
            | Action::Intercept
            | Action::Sweep => self.combat_action_state(unit, action),
            Action::TradeRoute => (self.trade_route_state(unit), None),
            Action::Plunder => (self.availability(&Command::PlunderTradeRoute { unit }), None),
            Action::SellGoods => (self.availability(&Command::SellExoticGoods { unit }), None),
            Action::BuyCityState => (self.availability(&buy_city_state(unit)), None),
        };
        ActionState {
            action,
            availability,
            detail,
        }
    }

    fn heal_availability(&mut self, unit: UnitHandle, core: &UnitCore) -> Availability {
        if core.hp >= UNIT_MAX_HP {
            return Availability::Blocked(reasons::full_health().into());
        }
        self.availability(&Command::Fortify { unit })
    }

    /// The improvement the core accepts on this tile that adds the most
    /// (weighted tile yields per worker turn), with its name and turns.
    fn improvement_choice(&mut self, unit: UnitHandle) -> (Availability, Option<String>) {
        match self.best_improvement(unit) {
            Ok((improvement, turns)) => (
                Availability::Ready,
                Some(self.improvement_label(improvement, turns)),
            ),
            Err(reason) => (Availability::Blocked(reason), None),
        }
    }

    /// Best accepted improvement for the worker's tile: (id, worker turns),
    /// or the first reason the core gave.
    fn best_improvement(&mut self, unit: UnitHandle) -> Result<(ImprovementId, u16), String> {
        let mut first_reason = None;
        let mut best: Option<(ImprovementId, u16, i32)> = None;
        for index in 0..self.rules().improvements.len() {
            let improvement = ImprovementId(index as u16);
            if let Err(error) = self.check(&Command::BuildImprovement { unit, improvement }) {
                if matches!(error, CommandError::Refused(_)) {
                    first_reason.get_or_insert(names::reason(self.rules(), &error));
                }
                continue;
            }
            let Some((turns, gain)) = self.build_gain(unit, improvement) else {
                continue;
            };
            if best.map(|(_, _, g)| gain > g).unwrap_or(true) {
                best = Some((improvement, turns, gain));
            }
        }
        match best {
            Some((improvement, turns, _)) => Ok((improvement, turns)),
            None => Err(first_reason.unwrap_or_else(|| reasons::nothing_to_build().into())),
        }
    }

    /// Worker turns and weighted yield gain per turn of a build on the
    /// worker's tile (the core's own site and yield rules).
    fn build_gain(&self, unit: UnitHandle, improvement: ImprovementId) -> Option<(u16, i32)> {
        use classic_rules::derived::tile_yields::{plot_yields, plot_yields_with};
        let auth = self.client.game.view().auth;
        let tile = auth.units().get(unit)?.core.tile;
        let rules = self.rules();
        let plan = classic_rules::world::builds::improvement_site(auth, rules, self.me(), tile, improvement).ok()?;
        let facts = classic_rules::world::facts::TruthFacts::new(auth, rules, self.me());
        let now = plot_yields(&facts, rules, tile, &facts.techs).at_least(0);
        let removes = plan.removes.is_some();
        let then = plot_yields_with(&facts, rules, tile, &facts.techs, improvement, removes).at_least(0);
        let value = classic_rules::ai::improvements::yield_value;
        let gain = (value(&then) - value(&now)) * 100 / i32::from(plan.work_turns.max(1));
        Some((plan.work_turns, gain))
    }

    fn improvement_label(&self, improvement: ImprovementId, turns: u16) -> String {
        let def = &self.rules().improvements[improvement.0 as usize];
        tf!("classic.game.orders.build-turns", name = names::named("improvement", &def.name, &def.key), n = turns)
    }

    /// "Remove Forest (4 turns, +20 production)" when the core accepts it.
    fn removal_choice(&mut self, unit: UnitHandle, core: &UnitCore) -> (Availability, Option<String>) {
        let availability = self.availability(&Command::ChopForest { unit });
        if !availability.is_ready() {
            return (availability, None);
        }
        let auth = self.client.game.view().auth;
        let site = classic_rules::world::builds::removal_site(auth, self.rules(), self.me(), core.tile);
        let Ok((feature, removal)) = site else {
            return (availability, None);
        };
        let feature_def = &self.rules().features[feature.0 as usize];
        let name = names::named("feature", &feature_def.name, &feature_def.key);
        let turns = classic_rules::world::builds::work_turns(removal.work_time);
        let chop = classic_rules::world::builds::chop_production(auth, self.me(), core.tile, removal.production);
        let detail = match chop {
            Some((_, amount)) => tf!("classic.game.orders.remove-production", name = name, n = turns, amount = amount),
            None => tf!("classic.game.orders.remove", name = name, n = turns),
        };
        (availability, Some(detail))
    }

    pub fn order(&mut self, action: Action) {
        let Some((unit, core)) = self.selected_unit() else {
            return;
        };
        let tile = core.tile;
        let core = *core;
        match action {
            // Move, Attack and Sweep wait for the click on a tile.
            Action::Move | Action::Attack | Action::Sweep => self.toggle_targeting(action),
            Action::Fortify | Action::Heal => self.order_and_continue(Command::Fortify { unit }),
            Action::Skip => self.rest(unit, false),
            Action::Sleep => self.rest(unit, true),
            Action::FoundCity => self.found_city(unit, tile),
            Action::Improve => self.build_improvement(unit),
            Action::Road => self.order_and_continue(Command::BuildRoad { unit }),
            Action::Chop => self.order_and_continue(Command::ChopForest { unit }),
            Action::Spread => self.order_and_continue(Command::SpreadReligion { unit }),
            Action::RemoveHeresy => self.order_and_continue(Command::RemoveHeresy { unit }),
            Action::HolySite => self.order_and_continue(Command::CreateGreatImprovement { unit }),
            Action::GreatWork => self.spend_great_person(Command::CreateGreatWork { unit }, t!("classic.game.orders.great-work-created")),
            Action::Treatise => self.spend_great_person(Command::WriteTreatise { unit }, t!("classic.game.orders.treatise-written")),
            Action::Concert => self.spend_great_person(Command::PerformConcert { unit }, t!("classic.game.orders.concert-performed")),
            Action::Excavate => self.order_and_continue(Command::Excavate { unit }),
            Action::GreatImprovement
            | Action::Discover
            | Action::HurryWonder
            | Action::GoldenAge
            | Action::TradeMission => {
                if let Some(command) = self.great_person_command(unit, &core, action) {
                    self.order_and_continue(command);
                }
            }
            Action::Promote
            | Action::Upgrade
            | Action::SetUp
            | Action::Pillage
            | Action::Intercept => self.combat_order(unit, action),
            // The app opens the trade panel (its routes are the orders).
            Action::TradeRoute => {}
            Action::Plunder => self.order_and_continue(Command::PlunderTradeRoute { unit }),
            Action::SellGoods => self.order_and_continue(Command::SellExoticGoods { unit }),
            Action::BuyCityState => self.order_and_continue(buy_city_state(unit)),
        }
    }

    fn trade_route_state(&self, unit: UnitHandle) -> Availability {
        let auth = self.game().view().auth;
        if classic_rules::derived::trade::trade_offers(auth, self.rules(), unit).is_empty() {
            Availability::Blocked(reasons::no_trade_target().into())
        } else {
            Availability::Ready
        }
    }

    fn toggle_targeting(&mut self, action: Action) {
        self.targeting = if self.targeting == Some(action) {
            None
        } else {
            Some(action)
        };
    }

    /// A great person's one-time act: the unit is spent, the notice tells it.
    fn spend_great_person(&mut self, command: Command, done: &'static str) {
        if self.submit(command) {
            self.notify(NoticeKind::Culture, done.to_string());
            self.select_next_idle_unit();
        }
    }

    fn order_and_continue(&mut self, command: Command) {
        if self.submit(command) {
            self.select_next_idle_unit();
        }
    }

    fn rest(&mut self, unit: UnitHandle, until_woken: bool) {
        if until_woken {
            self.sleeping.push(unit);
        } else {
            self.skipped.push(unit);
        }
        self.select_next_idle_unit();
    }

    fn found_city(&mut self, unit: UnitHandle, tile: TileId) {
        if !self.submit(Command::FoundCity { settler: unit }) {
            return;
        }
        if let Some(city) = self.own_city_at(tile) {
            self.client.selection = Selection::City(city);
        }
    }

    fn build_improvement(&mut self, unit: UnitHandle) {
        match self.best_improvement(unit) {
            Ok((improvement, _)) => {
                self.order_and_continue(Command::BuildImprovement { unit, improvement })
            }
            Err(reason) => self.message = Some(reason),
        }
    }

    /// Selects the next own unit that still needs orders this turn, or nothing.
    pub fn select_next_idle_unit(&mut self) {
        let next = self.idle_units().into_iter().next();
        self.client.selection = match next {
            Some(unit) => Selection::Unit(unit),
            None => Selection::None,
        };
        self.targeting = None;
    }

    pub fn idle_units(&self) -> Vec<UnitHandle> {
        let auth = self.client.game.view().auth;
        let mut idle = Vec::new();
        for &handle in auth.indexes().units_of(self.me()) {
            let Some(unit) = auth.units().get(handle) else {
                continue;
            };
            let resting = self.skipped.contains(&handle) || self.sleeping.contains(&handle);
            let busy = unit.core.stance.is_fortified() || unit.core.work.is_some();
            if unit.core.moves_left > 0 && !resting && !busy {
                idle.push(handle);
            }
        }
        idle
    }

    /// An aircraft never walks: its move order is a rebase to an own city or
    /// to an own carrier on the tile.
    pub(super) fn air_move_order(&self, unit: UnitHandle, tile: TileId) -> Option<Command> {
        let auth = self.game().view().auth;
        let core = auth.units().get(unit)?.core;
        let def = self.rules().units.get(core.kind.0 as usize)?;
        if def.domain != classic_rules::rules::content::Domain::Air {
            return None;
        }
        if let Some(city) = self.own_city_at(tile) {
            return Some(Command::Rebase { unit, city });
        }
        // An own ship that carries this kind of aircraft.
        let carrier = auth.indexes().units_on(tile).iter().copied().find(|c| {
            let Some(host) = auth.units().get(*c) else { return false };
            let host_def = self.rules().units.get(host.core.kind.0 as usize);
            host.core.owner == core.owner
                && def.combat.carried_as.is_some()
                && host_def.is_some_and(|h| h.combat.carries == def.combat.carried_as)
        });
        carrier.map(|carrier| Command::RebaseToCarrier { unit, carrier })
    }

    /// Range of the selected unit's air sweep, in tiles (the unit's own range, as the core checks it).
    pub(super) fn sweep_range(&self, unit: UnitHandle) -> i32 {
        let core = self.game().view().auth.units().get(unit).map(|u| *u.core);
        let kind = core.and_then(|c| self.rules().units.get(c.kind.0 as usize));
        kind.map_or(0, |def| i32::from(def.range))
    }

    /// May the unit sweep now: the core's verdict for a sweep of the tile it stands on
    /// (any tile in range is as legal as that one).
    pub(super) fn sweep_availability(&mut self, unit: UnitHandle) -> Availability {
        let Some(tile) = self.game().view().auth.units().get(unit).map(|u| u.core.tile) else {
            return Availability::Blocked(t!("classic.game.reasons.unavailable").into());
        };
        match self.check(&Command::AirSweep { unit, target: tile }) {
            Ok(()) => Availability::Ready,
            Err(error) => Availability::Blocked(self.air_refusal(&error)),
        }
    }

    /// The words of a refusal of an air order: its own wording for «out of range»
    /// (the shared English text belongs to the trade reason).
    pub(super) fn air_refusal(&self, error: &CommandError) -> String {
        if let CommandError::RuleViolation(text) = error {
            if let Some(words) = super::reasons_rules::air_order_text(text) {
                return words.into();
            }
        }
        names::reason(self.rules(), error)
    }

    /// The target click of the sweep mode: the selected aircraft sweeps the tile.
    pub(super) fn sweep(&mut self, tile: TileId) {
        let Some((unit, _)) = self.selected_unit() else {
            return;
        };
        let order = Command::AirSweep { unit, target: tile };
        match self.check(&order) {
            Ok(()) => {
                self.submit(order);
            }
            Err(error) => self.message = Some(self.air_refusal(&error)),
        }
    }

    /// The special strikes of a right click that are not an attack on a
    /// visible enemy: a nuclear missile launches at the tile, a paratrooper
    /// jumps there.
    pub(super) fn special_strike(&self, unit: UnitHandle, tile: TileId) -> Option<Command> {
        let auth = self.game().view().auth;
        let core = auth.units().get(unit)?.core;
        let def = self.rules().units.get(core.kind.0 as usize)?;
        if def.combat.nuke_level > 0 {
            return Some(Command::Nuke { unit, target: tile });
        }
        if classic_rules::world::military::paradrop_range(auth, self.rules(), unit) > 0 {
            return Some(Command::Paradrop { unit, destination: tile });
        }
        None
    }

    /// Right click or Attack mode: strike a visible foreign unit or city on the tile.
    pub(super) fn attack(&mut self, tile: TileId) {
        let Some((unit, _)) = self.selected_unit() else {
            return;
        };
        if let Some(order) = self.special_strike(unit, tile) {
            let enemy_here = self.attack_target(unit, tile).is_some();
            // A paratrooper attacks a visible enemy on foot, it jumps to free ground.
            if !(enemy_here && matches!(order, Command::Paradrop { .. })) {
                match self.check(&order) {
                    Ok(()) => {
                        self.submit(order);
                    }
                    Err(error) => self.message = Some(names::reason(self.rules(), &error)),
                }
                return;
            }
        }
        let Some((command, enemy)) = self.attack_target(unit, tile) else {
            self.message = Some(t!("classic.game.orders.no-enemy").into());
            return;
        };
        let at_war = self.game().view().auth.diplomacy().at_war(self.me(), enemy);
        if at_war {
            return self.strike(command);
        }
        match self.legal_after_war(enemy, &command) {
            Ok(()) => self.declare_war_and_strike(enemy, command),
            Err(error) => self.message = Some(names::reason(self.rules(), &error)),
        }
    }

    /// Target from the observer's view only: hidden units and cities are not in it.
    pub(super) fn attack_target(
        &self,
        attacker: UnitHandle,
        tile: TileId,
    ) -> Option<(Command, PlayerId)> {
        let me = self.me().0;
        let auth = self.game().view().auth;
        if self
            .view()
            .units
            .iter()
            .any(|u| u.tile == tile && u.owner != me)
        {
            for &defender in auth.indexes().units_on(tile) {
                let owner = auth.units().get(defender)?.core.owner;
                if owner.0 != me {
                    return Some((Command::AttackUnit { attacker, defender }, owner));
                }
            }
        }
        if !self
            .view()
            .cities
            .iter()
            .any(|c| c.tile == tile && c.owner != me)
        {
            return None;
        }
        let city = auth.indexes().city_on_tile(tile)?;
        let owner = auth.cities().get(city)?.owner;
        Some((Command::AttackCity { attacker, city }, owner))
    }

    /// At peace the core rejects attacks before checking range. Ask it on a copy at war,
    /// so a war is declared only when the strike that follows is legal.
    fn legal_after_war(&mut self, enemy: PlayerId, command: &Command) -> Result<(), CommandError> {
        let mut probe = self.client.game.clone();
        let war = Command::DeclareWar {
            player: self.me(),
            enemy,
        };
        probe.execute(&self.client.rules, war)?;
        let rules = self.client.rules.clone();
        classic_rules::command::validate(probe.view(), &rules, command.clone(), &mut self.scratch)
            .map(|_| ())
    }

    fn declare_war_and_strike(&mut self, enemy: PlayerId, command: Command) {
        let war = Command::DeclareWar {
            player: self.me(),
            enemy,
        };
        if self.submit(war) {
            let name = self.civilization_name(enemy);
            self.notify(NoticeKind::Diplomacy, tf!("classic.game.orders.declared-war", name = name));
            self.strike(command);
        }
    }

    fn strike(&mut self, command: Command) {
        let before = self.combat_report(&command);
        if self.submit(command.clone()) {
            let after = self.combat_report(&command);
            let text = tf!("classic.game.orders.attack", before = before, after = after);
            self.notify(NoticeKind::Combat, text);
        }
    }

    fn combat_report(&self, command: &Command) -> String {
        let auth = self.game().view().auth;
        let hp = |unit: UnitHandle| match auth.units().get(unit) {
            Some(u) => tf!("classic.game.orders.hp", n = u.core.hp),
            None => t!("classic.game.orders.destroyed").to_string(),
        };
        match command {
            Command::AttackUnit { attacker, defender } => {
                tf!("classic.game.orders.report-units", mine = hp(*attacker), theirs = hp(*defender))
            }
            Command::AttackCity { attacker, city } => {
                let city_hp = match auth.cities().get(*city) {
                    Some(c) if c.owner == self.me() => t!("classic.game.orders.captured").to_string(),
                    Some(c) => {
                        let max = classic_rules::combat::city::max_hit_points(auth, self.rules(), c);
                        tf!("classic.game.orders.hp", n = max.saturating_sub(c.damage))
                    }
                    None => t!("classic.game.orders.gone").to_string(),
                };
                tf!("classic.game.orders.report-city", mine = hp(*attacker), city = city_hp)
            }
            _ => String::new(),
        }
    }
}

fn needs_moves(core: &UnitCore) -> Availability {
    if core.moves_left > 0 {
        Availability::Ready
    } else {
        Availability::Blocked(reasons::no_moves().into())
    }
}

/// Move points as the player reads them: whole points and a fraction
/// (a road step is half a point).
fn moves_text(points: u16) -> String {
    let denominator = classic_rules::world::unit::MOVE_DENOMINATOR;
    let (whole, part) = (points / denominator, points % denominator);
    match (whole, part) {
        (_, 0) => format!("{whole}"),
        (0, _) => format!("{part}/{denominator}"),
        _ => format!("{whole} {part}/{denominator}"),
    }
}
