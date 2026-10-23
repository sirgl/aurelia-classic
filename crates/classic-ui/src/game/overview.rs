//! Advisor data: the cities, units and demographics overviews, the city
//! banners and the top-bar breakdowns. Every number is a core query
//! (`city_income`, `player_income`, `maintenance::upkeep`, `unit_modifiers`); views
//! of the whole empire are rebuilt only when the game changed (`revision`).
use classic_rules::derived::income::{city_income, growth_threshold, player_income, player_income_from, CityIncome};
use classic_rules::derived::maintenance::{upkeep, Upkeep};
use classic_rules::effects::unit::{unit_modifiers, CombatRole};
use classic_rules::ids::{PlayerId, TileId};
use classic_rules::rules::content::BuildingKind;
use classic_rules::world::city::CityCore;
use classic_rules::world::unit::UnitCore;

use i18n::{t, tf};

use super::names;
use super::session::Session;
use crate::ui::overview_model::{
    CityBanner, CityRow, DemographicRow, HudExtras, OverviewView, TopBarTips, UnitRow, UnitTip,
    YieldTip,
};

/// Rows a breakdown tooltip lists before "N more cities".
const TIP_CITIES: usize = 6;

/// Views of the whole empire, valid for one game revision.
#[derive(Default)]
pub(super) struct ViewCache {
    revision: Option<u64>,
    overview: Option<OverviewView>,
    top_bar: TopBarTips,
    banners: Vec<CityBanner>,
}

impl Session {
    fn cache_for_revision(&mut self) {
        if self.cache.revision != Some(self.revision) {
            self.cache = ViewCache { revision: Some(self.revision), ..ViewCache::default() };
        }
    }

    /// Drop the cached empire views (benches; the game itself bumps the revision).
    pub fn invalidate_views(&mut self) {
        self.revision += 1;
    }

    /// Cities, units, demographics, the notification log and victory progress.
    pub fn overview_view(&mut self) -> OverviewView {
        self.cache_for_revision();
        if let Some(view) = &self.cache.overview {
            return view.clone();
        }
        let view = OverviewView {
            turn: self.turn(),
            cities: self.city_rows(),
            units: self.unit_rows(),
            demographics: self.demographics(),
            notices: self.notices.clone(),
            victory: Some(self.victory_view()),
            rivals: self.rivals(),
            gold: self.top_bar_tips().yields.first().cloned(),
            resources: self.resource_rows(),
            trade_routes: self.trade_route_rows(),
            trade_units: self.trade_unit_rows(),
            trade_summary: self.trade_summary(),
        };
        self.cache.overview = Some(view.clone());
        view
    }

    /// Banners, top-bar breakdowns and the units on the hovered badge.
    pub fn hud_extras(&mut self, badge: Option<u32>) -> HudExtras {
        self.cache_for_revision();
        if self.cache.banners.is_empty() && !self.game().view().auth.cities().is_empty() {
            self.cache.banners = self.city_banners();
        }
        HudExtras {
            top_bar: self.top_bar_tips(),
            banners: self.cache.banners.clone(),
            units: badge.map(|tile| (tile, self.unit_tips(TileId(tile)))),
            player_names: self.player_names(),
        }
    }

    fn player_names(&self) -> Vec<String> {
        let count = self.game().view().auth.players().count();
        (0..count).map(|p| self.civilization_name(PlayerId(p as u8))).collect()
    }

    fn own_cities(&self) -> Vec<CityCore> {
        let auth = self.game().view().auth;
        let handles = auth.indexes().cities_of(self.me());
        handles.iter().filter_map(|h| auth.cities().get(*h)).cloned().collect()
    }

    fn income_of(&self, core: &CityCore) -> CityIncome {
        city_income(self.game().view().auth, self.rules(), core)
    }

    fn city_rows(&self) -> Vec<CityRow> {
        self.own_cities()
            .iter()
            .map(|core| {
                let income = self.income_of(core);
                CityRow {
                    tile: core.tile.0,
                    name: core.name.clone(),
                    capital: self.is_capital(core),
                    population: core.population,
                    food: income.food_surplus.max(0) as u32,
                    production: income.production.max(0) as u32,
                    gold: income.gold,
                    science: income.science,
                    culture: income.culture,
                    faith: income.faith,
                    strength: self.city_strength(core),
                    producing: self.producing(core, &income),
                    grows_in: grows_in(core, &income),
                }
            })
            .collect()
    }

    pub(super) fn is_capital(&self, core: &CityCore) -> bool {
        let rules = self.rules();
        core.buildings
            .iter()
            .any(|b| rules.buildings.get(b.0 as usize).is_some_and(|d| d.kind == BuildingKind::Palace))
    }

    fn producing(&self, core: &CityCore, income: &CityIncome) -> String {
        let Some(item) = core.queue.first() else {
            return t!("classic.game.overview.producing-nothing").into();
        };
        let cost = names::item_cost(self.rules(), *item);
        let left = cost.saturating_sub(core.production);
        let turns = left.div_ceil(income.production.max(1) as u32).max(1);
        format!("{} ({turns})", names::item(self.rules(), *item)) // i18n-ok: a name and a number
    }

    fn unit_rows(&self) -> Vec<UnitRow> {
        let auth = self.game().view().auth;
        let rules = self.rules();
        let mut rows: Vec<UnitRow> = auth
            .units()
            .iter_handles()
            .filter(|(_, u)| u.owner == self.me())
            .map(|(unit, core)| {
                let def = rules.units.get(core.kind.0 as usize);
                UnitRow {
                    unit,
                    tile: core.tile.0,
                    kind: core.kind,
                    name: names::unit(rules, core.kind),
                    military: !rules.is_civilian(core.kind),
                    hp: core.hp,
                    strength: unit_modifiers(auth, rules, core).strength(rules, core.kind, CombatRole::Defense),
                    ranged: def.map(|d| d.ranged_strength).unwrap_or(0),
                    moves: format!("{}/{}", moves_text(core.moves_left), moves_text(core.moves_max)),
                    status: unit_status(auth, rules, core),
                }
            })
            .collect();
        rows.sort_by(|a, b| (!a.military, &a.name).cmp(&(!b.military, &b.name)));
        rows
    }

    fn unit_tips(&self, tile: TileId) -> Vec<UnitTip> {
        let view = self.view();
        let visible = view.visible.get(tile.0 as usize).copied().unwrap_or(0) != 0;
        if !visible {
            return Vec::new();
        }
        let auth = self.game().view().auth;
        let rules = self.rules();
        let handles = auth.indexes().units_on(tile).to_vec();
        handles
            .iter()
            .filter_map(|h| auth.units().get(*h).map(|unit| (*h, unit)))
            .map(|(handle, unit)| {
                let core = unit.core;
                let own = core.owner == self.me();
                let modifiers = unit_modifiers(auth, rules, core);
                let def = rules.units.get(core.kind.0 as usize);
                UnitTip {
                    kind: core.kind,
                    name: names::unit(rules, core.kind),
                    owner: self.civilization_name(core.owner),
                    player: core.owner.0,
                    own,
                    hp: core.hp,
                    attack: modifiers.strength(rules, core.kind, CombatRole::Attack),
                    defense: modifiers.strength(rules, core.kind, CombatRole::Defense),
                    ranged: def.map(|d| d.ranged_strength).unwrap_or(0),
                    range: def.map(|d| d.range).unwrap_or(0),
                    moves: own.then(|| format!("{}/{}", moves_text(core.moves_left), moves_text(core.moves_max))),
                    sight: own.then(|| self.game().derived().visibility.sight_of(handle)),
                    status: own.then(|| unit_status(auth, rules, core)),
                    promotions: promotions(rules, core),
                }
            })
            .collect()
    }

    fn city_banners(&self) -> Vec<CityBanner> {
        let auth = self.game().view().auth;
        let view = self.view();
        auth.cities()
            .iter()
            .filter(|(_, core)| view.visible.get(core.tile.0 as usize).copied().unwrap_or(0) != 0 || core.owner == self.me())
            .map(|(_, core)| self.banner(core))
            .collect()
    }

    /// Combat strength of a city as the game shows it.
    fn city_strength(&self, core: &CityCore) -> i32 {
        let auth = self.game().view().auth;
        (classic_rules::combat::city::city_strength(auth, self.rules(), core) / 100) as i32
    }

    fn banner(&self, core: &CityCore) -> CityBanner {
        let own = core.owner == self.me();
        let strength = self.city_strength(core);
        let auth = self.game().view().auth;
        let max_hp = classic_rules::combat::city::max_hit_points(auth, self.rules(), core);
        let hp = max_hp.saturating_sub(core.damage);
        let base = CityBanner { tile: core.tile.0, own, strength, hp, max_hp, can_strike: false, growth: None, production: None, tip: None, visitors: self.spies_in(core.tile) };
        if !own {
            return base;
        }
        let can_strike = auth.indexes().city_on_tile(core.tile).is_some_and(|city| self.city_can_strike(city));
        let income = self.income_of(core);
        let production = core.queue.first().map(|item| {
            let cost = names::item_cost(self.rules(), *item);
            let left = cost.saturating_sub(core.production);
            let per_turn = income.production.max(0) as u32;
            let turns = (per_turn > 0).then(|| left.div_ceil(per_turn).max(1));
            (names::item(self.rules(), *item), core.production.min(cost), cost, turns)
        });
        CityBanner {
            can_strike,
            growth: Some((
                core.food_times100 / 100,
                growth_threshold(core.population),
                grows_in(core, &income),
            )),
            production,
            ..base
        }
    }

    /// Gold, science, culture, faith, food and production with their sources.
    pub fn top_bar_tips(&mut self) -> TopBarTips {
        self.cache_for_revision();
        if !self.cache.top_bar.yields.is_empty() {
            return self.cache.top_bar.clone();
        }
        let tips = self.build_top_bar_tips();
        self.cache.top_bar = tips.clone();
        tips
    }

    fn build_top_bar_tips(&self) -> TopBarTips {
        let empire = self.empire_income();
        let economy = self.economy();
        let total = &empire.total;
        let mut gold = empire.lines(|i| i.gold as i64);
        let upkeep = &empire.upkeep;
        push_modifier(&mut gold, t!("classic.game.overview.modifiers-empire"), (total.gold + upkeep.net_cost()) as i64 - empire.sum(|i| i.gold as i64));
        push_modifier(&mut gold, t!("classic.game.overview.maintenance-units"), -upkeep.units as i64);
        push_modifier(&mut gold, t!("classic.game.overview.maintenance-buildings"), -upkeep.buildings as i64);
        push_modifier(&mut gold, t!("classic.game.overview.maintenance-roads"), -upkeep.routes as i64);
        push_modifier(&mut gold, t!("classic.game.overview.city-connections"), upkeep.connections as i64);
        let mut science = empire.lines(|i| i.science as i64);
        push_modifier(&mut science, t!("classic.game.overview.modifiers-empire"), total.science as i64 - empire.sum(|i| i.science as i64));
        let mut faith = empire.lines(|i| i.faith as i64);
        push_modifier(&mut faith, t!("classic.game.overview.modifiers-empire"), total.faith as i64 - empire.sum(|i| i.faith as i64));
        let food = tf!("classic.game.overview.food-surplus", value = format!("{:+}", empire.sum(|i| i.food_surplus as i64)));
        let production = tf!("classic.game.overview.per-turn", value = empire.sum(|i| i.production as i64));
        TopBarTips {
            yields: vec![
                tip(t!("classic.game.overview.tip-gold"), tf!("classic.game.overview.total-with-rate", stock = economy.gold, rate = format!("{:+}", total.gold)), gold, None),
                tip(t!("classic.game.overview.tip-food"), food, empire.lines(|i| i.food_surplus as i64), Some(t!("classic.game.overview.note-food"))),
                tip(t!("classic.game.overview.tip-production"), production, empire.lines(|i| i.production as i64), None),
                tip(t!("classic.game.overview.tip-science"), tf!("classic.game.overview.total-stored", stock = economy.science, rate = format!("{:+}", total.science)), science, None),
                tip(t!("classic.game.overview.tip-culture"), tf!("classic.game.overview.total-with-rate", stock = economy.culture, rate = format!("{:+}", total.culture)), empire.lines(|i| i.culture as i64), Some(t!("classic.game.overview.note-culture"))),
                tip(t!("classic.game.overview.tip-faith"), tf!("classic.game.overview.total-with-rate", stock = economy.faith, rate = format!("{:+}", total.faith)), faith, Some(t!("classic.game.overview.note-faith"))),
            ],
        }
    }

    /// Every own city's income and the player's, from the core's income queries.
    fn empire_income(&self) -> EmpireIncome {
        let auth = self.game().view().auth;
        let cities = self.own_cities();
        let incomes: Vec<CityIncome> = cities.iter().map(|c| self.income_of(c)).collect();
        let total = player_income_from(auth, self.rules(), self.me(), &incomes);
        let upkeep = upkeep(auth, self.rules(), self.me());
        let names = cities.into_iter().map(|c| c.name).collect();
        EmpireIncome { names, incomes, total, upkeep }
    }

    fn demographics(&self) -> Vec<DemographicRow> {
        let auth = self.game().view().auth;
        let rules = self.rules();
        let players: Vec<PlayerId> = (0..auth.players().count() as u8)
            .map(PlayerId)
            .filter(|p| auth.players().is_alive(*p))
            .collect();
        let mut land = vec![0i64; auth.players().count()];
        for t in 0..auth.map().tile_count() as u32 {
            if let Some(Some(owner)) = auth.map().owner(TileId(t)) {
                if let Some(count) = land.get_mut(owner.0 as usize) {
                    *count += 1;
                }
            }
        }
        let techs = rules.technologies.len().max(1) as i64;
        let derived = &self.game().view().derived.yields;
        let food = |p: PlayerId| derived.of_player(p).food as i64;
        let production = |p: PlayerId| derived.of_player(p).production as i64;
        let measures: [Measure<'_>; 7] = [
            (t!("classic.game.overview.demo-population"), t!("classic.game.overview.demo-population-unit"), Box::new(|p| population(auth, p))),
            (t!("classic.game.overview.demo-crop"), t!("classic.game.overview.demo-crop-unit"), Box::new(food)),
            (t!("classic.game.overview.demo-goods"), t!("classic.game.overview.demo-goods-unit"), Box::new(production)),
            (t!("classic.game.overview.demo-gnp"), t!("classic.game.overview.demo-gnp-unit"), Box::new(|p| player_income(auth, rules, p).gold as i64)),
            (t!("classic.game.overview.demo-literacy"), t!("classic.game.overview.demo-literacy-unit"), Box::new(|p| auth.players().techs_of(p).len() as i64 * 100 / techs)),
            (t!("classic.game.overview.demo-land"), t!("classic.game.overview.demo-land-unit"), Box::new(|p| land.get(p.0 as usize).copied().unwrap_or(0))),
            (t!("classic.game.overview.demo-soldiers"), t!("classic.game.overview.demo-soldiers-unit"), Box::new(|p| soldiers(auth, rules, p))),
        ];
        measures
            .iter()
            .map(|(label, unit, measure)| demographic(label, unit, &players, self.me(), measure.as_ref()))
            .collect()
    }


}

fn population(auth: &classic_rules::world::state::AuthWorld, p: PlayerId) -> i64 {
    let cities = auth.indexes().cities_of(p);
    cities.iter().filter_map(|c| auth.cities().get(*c)).map(|c| c.population as i64).sum()
}

fn soldiers(auth: &classic_rules::world::state::AuthWorld, rules: &classic_rules::CompiledRuleset, p: PlayerId) -> i64 {
    let units = auth.indexes().units_of(p);
    units
        .iter()
        .filter_map(|u| auth.units().get(*u))
        .filter(|u| !rules.is_civilian(u.core.kind))
        .count() as i64
}

fn demographic(
    label: &'static str,
    unit: &'static str,
    players: &[PlayerId],
    me: PlayerId,
    measure: &dyn Fn(PlayerId) -> i64,
) -> DemographicRow {
    let values: Vec<(PlayerId, i64)> = players.iter().map(|p| (*p, measure(*p))).collect();
    let mine = values.iter().find(|(p, _)| *p == me).map(|(_, v)| *v).unwrap_or(0);
    let rank = 1 + values.iter().filter(|(_, v)| *v > mine).count();
    let best = values.iter().map(|(_, v)| *v).max().unwrap_or(0);
    let worst = values.iter().map(|(_, v)| *v).min().unwrap_or(0);
    let average = values.iter().map(|(_, v)| *v).sum::<i64>() / values.len().max(1) as i64;
    DemographicRow { label, unit, value: mine, rank, best, average, worst }
}

/// A demographics measure: label, unit and the value of a player.
type Measure<'a> = (&'static str, &'static str, Box<dyn Fn(PlayerId) -> i64 + 'a>);

/// City incomes of the player with the player's totals.
struct EmpireIncome {
    names: Vec<String>,
    incomes: Vec<CityIncome>,
    total: classic_rules::derived::income::PlayerIncome,
    upkeep: Upkeep,
}

impl EmpireIncome {
    fn sum(&self, value: impl Fn(&CityIncome) -> i64) -> i64 {
        self.incomes.iter().map(value).sum()
    }

    /// Breakdown rows of one yield by city.
    fn lines(&self, value: impl Fn(&CityIncome) -> i64) -> Vec<(String, String)> {
        let values = self.names.iter().cloned().zip(self.incomes.iter().map(value)).collect();
        city_lines(values)
    }
}

fn tip(title: &'static str, total: String, lines: Vec<(String, String)>, note: Option<&'static str>) -> YieldTip {
    YieldTip { title: title.into(), total, lines, note: note.map(Into::into) }
}

/// The largest city contributions, then the rest summed.
fn city_lines(mut values: Vec<(String, i64)>) -> Vec<(String, String)> {
    values.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut lines: Vec<(String, String)> = values
        .iter()
        .take(TIP_CITIES)
        .map(|(name, v)| (name.clone(), format!("{v:+}")))
        .collect();
    if values.len() > TIP_CITIES {
        let rest: i64 = values[TIP_CITIES..].iter().map(|(_, v)| *v).sum();
        lines.push((tf!("classic.game.overview.more-cities", n = values.len() - TIP_CITIES), format!("{rest:+}")));
    }
    if lines.is_empty() {
        lines.push((t!("classic.game.overview.no-cities").into(), "0".into()));
    }
    lines
}

fn push_modifier(lines: &mut Vec<(String, String)>, label: &str, value: i64) {
    if value != 0 {
        lines.push((label.into(), format!("{value:+}")));
    }
}

fn grows_in(core: &CityCore, income: &CityIncome) -> Option<u32> {
    if income.food_surplus_times100 <= 0 {
        return None;
    }
    let missing = (growth_threshold(core.population) * 100).saturating_sub(core.food_times100);
    Some(missing.div_ceil(income.food_surplus_times100 as u32).max(1))
}

fn unit_status(auth: &classic_rules::world::state::AuthWorld, rules: &classic_rules::CompiledRuleset, core: &UnitCore) -> String {
    if let Some(work) = core.work {
        let turns = classic_rules::world::builds::job_turns_left(auth, rules, core.owner, &work);
        return tf!("classic.game.overview.status-working", n = turns);
    }
    if core.stance.is_fortified() {
        return t!("classic.game.overview.status-fortified").into();
    }
    if core.stance == classic_rules::world::unit::Stance::Intercept {
        return t!("classic.game.overview.status-intercepting").into();
    }
    let done = core.moves_left == 0;
    if unit_modifiers(auth, rules, core).embarked_on(rules, auth.map(), core.kind, core.tile) {
        return if done { t!("classic.game.overview.status-done-embarked") } else { t!("classic.game.overview.status-ready-embarked") }.into();
    }
    if done { t!("classic.game.overview.status-done") } else { t!("classic.game.overview.status-ready") }.into()
}

fn promotions(rules: &classic_rules::CompiledRuleset, core: &UnitCore) -> Vec<String> {
    (0..rules.promotions.len())
        .filter(|p| core.promotions.contains(classic_rules::ids::PromotionId(*p as u16)))
        .map(|p| names::named("promotion", &rules.promotions[p].name, &rules.promotions[p].key))
        .collect()
}

impl Session {
    /// The next unit waiting for orders after the selected one ("next unit"), or the
    /// one before it when `forward` is false.
    pub fn cycle_idle_unit(&mut self, forward: bool) -> Option<u32> {
        let idle = self.idle_units();
        let current = self.selection().selected_unit();
        let at = current.and_then(|c| idle.iter().position(|u| *u == c));
        let next = match (at, forward) {
            (Some(i), true) => idle.get((i + 1) % idle.len()).copied(),
            (Some(i), false) => idle.get((i + idle.len() - 1) % idle.len()).copied(),
            (None, true) => idle.first().copied(),
            (None, false) => idle.last().copied(),
        };
        let unit = next?;
        self.select_unit(unit);
        self.selected_tile()
    }

    /// Select an own unit (an overview row); its tile for the camera.
    pub fn focus_unit(&mut self, unit: classic_rules::ids::UnitHandle) -> Option<u32> {
        let auth = self.game().view().auth;
        let owner = auth.units().get(unit).map(|u| u.core.owner)?;
        if owner != self.me() {
            return None;
        }
        self.select_unit(unit);
        self.selected_tile()
    }
}

pub(super) fn moves_text(points: u16) -> String {
    let denominator = classic_rules::world::unit::MOVE_DENOMINATOR;
    let (whole, part) = (points / denominator, points % denominator);
    match (whole, part) {
        (_, 0) => format!("{whole}"),
        (0, _) => format!("{part}/{denominator}"),
        _ => format!("{whole} {part}/{denominator}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::model::{Action, GameRequest};
    use crate::ui::screens::{game_rules, prepare_match, MapSize, MatchRequest};

    fn founded_match() -> Session {
        let request = MatchRequest {
            civilization: "core:nation/rome".into(),
            seed: 20260928,
            size: MapSize::Medium,
            players: 3,
            water: 25,
        };
        let prepared = prepare_match(request).unwrap();
        let mut session = Session::new(prepared.game, game_rules());
        session.request(GameRequest::Unit(Action::FoundCity));
        session
    }

    #[test]
    fn overviews_read_the_core_and_are_rebuilt_only_after_a_change() {
        let mut session = founded_match();
        let view = session.overview_view();
        assert_eq!(view.cities.len(), 1);
        assert!(view.cities[0].capital, "the first city holds the Palace");
        assert!(!view.units.is_empty());
        assert_eq!(view.demographics.len(), 7);
        for row in &view.demographics {
            assert!((1..=3).contains(&row.rank), "{}: rank {}", row.label, row.rank);
            assert!(row.worst <= row.value && row.value <= row.best, "{}", row.label);
        }
        assert_eq!(session.overview_view(), view, "same game, same view");
        let revision = session.revision;
        session.end_turn();
        assert!(session.revision > revision, "a turn is a change");
        assert_eq!(session.overview_view().turn, view.turn + 1);
    }

    #[test]
    fn top_bar_breakdowns_add_up_to_the_core_income() {
        let mut session = founded_match();
        session.end_turn();
        let auth = session.game().view().auth;
        let income = classic_rules::derived::income::player_income(auth, session.rules(), session.me());
        let tips = session.top_bar_tips();
        let gold = tips.yields.iter().find(|t| t.title == "Gold").unwrap();
        let sum: i64 = gold.lines.iter().map(|(_, v)| v.trim_start_matches('+').parse::<i64>().unwrap()).sum();
        assert_eq!(sum, income.gold as i64, "cities + modifiers + upkeep = income: {:?}", gold.lines);
        let titles: Vec<&str> = tips.yields.iter().map(|t| t.title.as_ref()).collect();
        assert_eq!(titles, ["Gold", "Food", "Production", "Science", "Culture", "Faith"]);
    }

    #[test]
    fn next_unit_cycles_through_every_idle_unit() {
        let mut session = founded_match();
        let tile = session.selected_tile().map(classic_rules::ids::TileId).unwrap();
        let (game, rules) = session.harness_game();
        let warrior = rules.units.iter().position(|u| u.key == "core:unit/warrior").unwrap();
        for _ in 0..2 {
            game.debug_spawn_unit(&rules, PlayerId(0), tile, classic_rules::ids::UnitKindId(warrior as u16));
        }
        session.refresh_after_harness();
        let idle = session.idle_units();
        assert!(idle.len() >= 3, "an army: {idle:?}");
        let mut seen = Vec::new();
        for _ in 0..idle.len() {
            session.cycle_idle_unit(true).expect("an idle unit");
            seen.extend(session.selection().selected_unit());
        }
        assert!(idle.iter().all(|u| seen.contains(u)), "each idle unit once per cycle");
    }

    #[test]
    fn an_own_unit_tip_names_the_sight_it_has_where_it_stands() {
        let mut session = founded_match();
        let tile = session.selected_tile().map(classic_rules::ids::TileId).unwrap();
        let (game, rules) = session.harness_game();
        let scout = rules.units.iter().position(|u| u.key == "core:unit/scout").unwrap();
        game.debug_spawn_unit(&rules, PlayerId(0), tile, classic_rules::ids::UnitKindId(scout as u16));
        let on_hills = game.view().auth.map().terrain(tile).is_some_and(|t| game.view().auth.map().terrain_kinds().is_hills(t.0));
        session.refresh_after_harness();
        let expected = i32::from(rules.units[scout].sight) + i32::from(on_hills);
        let tips = session.hud_extras(Some(tile.0)).units.unwrap().1;
        let scout_tip = tips.iter().find(|tip| tip.name == "Scout").expect("the scout");
        assert_eq!(scout_tip.sight, Some(expected));
    }

    #[test]
    fn banners_show_own_growth_and_production() {
        let mut session = founded_match();
        let extras = session.hud_extras(None);
        let own: Vec<_> = extras.banners.iter().filter(|b| b.own).collect();
        assert_eq!(own.len(), 1);
        assert!(own[0].growth.is_some());
        assert_eq!(extras.player_names.len(), 3);
        assert_eq!(extras.player_names[0], "Rome");
    }
}

#[cfg(test)]
mod victory_tests {
use classic_rules::ClassicGame;
    use super::*;
    use crate::ui::screens::{game_rules, prepare_match, MapSize, MatchRequest};
    use classic_rules::rules::content::ProjectId;
    use classic_rules::world::victory::{VictoryKind, VictoryOptions};

    #[test]
    fn overview_victory_uses_real_progress_options_and_observer_scores() {
        let prepared = prepare_match(MatchRequest {
            civilization: "core:nation/rome".into(), seed: 7, size: MapSize::Small, players: 3, water: 25,
        }).unwrap();
        let mut session = Session::new(prepared.game, game_rules());
        let expected = session.victory_view();
        assert_eq!(session.overview_view().victory.as_ref(), Some(&expected));
        assert_eq!(expected.scores.len(), 1 + session.met_civilizations().len());
        assert!(expected.scores.len() < 3, "fixture includes unknown nations");
        assert!(expected.scores.iter().all(|score| score.mine || session.met_civilizations().iter()
            .any(|r| session.civilization_name(r.other) == score.name)));
        let me = session.me();
        let (game, rules) = session.harness_game();
        for key in ["core:project/apollo_program", "core:project/ss_booster"] {
            let id = rules.module.projects.iter().position(|p| p.key == key).unwrap();
            game.debug_add_project(me, ProjectId(id as u16));
        }
        game.debug_set_victory_options(VictoryOptions { disabled: 0, max_rounds: 99 }
            .with(VictoryKind::Domination, false).with(VictoryKind::Cultural, false));
        session.refresh_after_harness();
        let actual = session.overview_view().victory.unwrap();
        assert_eq!(actual, session.victory_view(), "cached overview follows the current projection");
        assert_eq!(actual.rows.iter().filter(|row| row.enabled).count(), 3);
        let science = actual.rows.iter().find(|row| row.kind == "Science").unwrap();
        assert_eq!(science.have, 2, "Apollo plus one installed booster, not known technologies");
        assert!(science.progress.contains("Apollo Program built"));
        assert_eq!(actual.rows.iter().find(|row| row.kind == "Time").unwrap().need, 100);
    }
}
