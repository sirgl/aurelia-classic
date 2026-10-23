//! City screen data for the selected own city, and the city requests it sends back.
use client::selection::Selection;
use i18n::{t, tf};
use classic_rules::world::city::CITY_WORK_RADIUS;
use classic_rules::derived::citizens::{specialist_slots, specialist_yield_table};
use classic_rules::derived::great_people::{city_points, great_person_threshold};
use classic_rules::derived::income::{player_growth_threshold, player_income, CityIncome};
use classic_rules::derived::tile_yields::tile_yields_for;
use classic_rules::derived::yields::{city_breakdown, city_work};
use classic_rules::ids::{BuildingKindId, CityHandle, ProductionItem, TileId, UnitKindId};
use classic_rules::math::tiles_in_hex_radius;
use classic_rules::rules::content::SpecialistId;
use classic_rules::rules::content::Yields;
use classic_rules::world::citizens::CityFocus;
use classic_rules::world::city::{CityCore, PURCHASE_RADIUS_MAX};
use classic_rules::world::territory::tile_purchase_offer;
use classic_rules::Command;

use super::names;
use super::reasons;
use super::session::Session;
use crate::ui::model::{
    Availability, BuildingEntry, CityView, Economy, GameRequest, GreatPersonProgress, NoticeKind,
    ProductionOption, QueueEntry, SpecialistRow, TileOffer, WorkedTile,
};

/// Per-turn city income: the core's own query (the numbers the end turn applies).
fn city_income(session: &Session, core: &CityCore) -> CityIncome {
    classic_rules::derived::income::city_income(session.game().view().auth, session.rules(), core)
}

impl Session {
    pub fn economy(&self) -> Economy {
        let view = self.game().view();
        let me = self.me();
        let income = player_income(view.auth, self.rules(), me);
        let happiness = classic_rules::derived::happiness::player_happiness(view.auth, self.rules(), me);
        Economy {
            happiness: happiness.excess(),
            golden_age: golden_age_text(view.auth, me),
            gold: view.auth.players().treasury_of(me),
            science: view.auth.players().science_of(me),
            food: view.derived.yields.of_player(me).food.max(0) as u32,
            production: view.derived.yields.of_player(me).production.max(0) as u32,
            gold_per_turn: income.gold,
            science_per_turn: income.science.max(0) as u32,
            culture: view.auth.module().culture().of(me).culture,
            culture_per_turn: income.culture,
            faith: view.auth.module().religion().player(me).faith,
            faith_per_turn: income.faith,
            resources: self.resource_bar(),
        }
    }

    fn selected_city(&self) -> Option<(CityHandle, CityCore)> {
        let Selection::City(handle) = self.selection() else {
            return None;
        };
        let core = self.game().view().auth.cities().get(handle)?;
        if core.owner != self.me() {
            return None;
        }
        Some((handle, core.clone()))
    }

    /// City strength (hundredths), hit points and maximum hit points.
    fn city_defense(&self, core: &CityCore) -> (i64, u16, u16) {
        let auth = self.game().view().auth;
        let rules = self.rules();
        let max = classic_rules::combat::city::max_hit_points(auth, rules, core);
        let strength = classic_rules::combat::city::city_strength(auth, rules, core);
        (strength, max.saturating_sub(core.damage), max)
    }

    pub fn city_view(&mut self) -> Option<CityView> {
        let (handle, core) = self.selected_city()?;
        let income = city_income(self, &core);
        let hurry = self.hurry_state(handle, &core);
        let food_needed = player_growth_threshold(
            self.game().view().auth,
            self.rules(),
            core.owner,
            core.population,
        );
        Some(CityView {
            tile: core.tile.0,
            name: core.name.clone(),
            names: self.free_names(),
            subtitle: tf!("classic.game.city.subtitle",
                nation = self.civilization_name(self.me()).to_uppercase()
            ),
            population: core.population,
            growth: growth_text(&core, &income, food_needed),
            food_stored: core.food_times100 / 100,
            food_needed,
            yields: self.yield_rows(&core, &income),
            production_stored: core.production,
            production_per_turn: income.production,
            idle_production: self.idle_production(&core),
            treasury: self.game().view().auth.players().treasury_of(self.me()),
            queue: self.queue_entries(&core),
            options: self.production_options(handle, &core, &income),
            hurry,
            clear: self.clear_state(handle, &core),
            buildings: self.building_entries(&core),
            citizens: self.citizens_line(&core),
            focus: self.focus_options(handle, &core),
            avoid_growth: self.avoid_growth_option(handle, &core),
            specialists: self.specialist_rows(handle, &core),
            great_people: self.great_people_progress(handle, &core),
            processes: self.process_options(handle, &core),
            tiles: self.worked_tiles(handle, &core),
            offers: self.tile_offers(handle, &core),
        })
    }

    /// The unused names of the civilization's list, in the order they are given out.
    fn free_names(&self) -> Vec<(u16, String)> {
        let auth = self.game().view().auth;
        classic_rules::world::city::free_city_names(auth, self.rules(), self.me())
            .into_iter()
            .map(|(at, name)| (at, name.to_owned()))
            .collect()
    }

    fn idle_production(&self, core: &CityCore) -> String {
        use classic_rules::world::city_economy::Process;
        match core.economy.process {
            None => t!("classic.game.city.idle-none").into(),
            Some(Process::Wealth) => t!("classic.game.city.idle-wealth").into(),
            Some(Process::Research) => t!("classic.game.city.idle-research").into(),
            Some(Process::Contribute(id)) => {
                let project = &self.rules().module.projects[id.0 as usize];
                tf!("classic.game.city.idle-contribute", project = names::named("project", &project.name, &project.key))
            }
        }
    }

    fn queue_entries(&self, core: &CityCore) -> Vec<QueueEntry> {
        let mods =
            classic_rules::effects::city::city_modifiers(self.game().view().auth, self.rules(), core);
        core.queue
            .iter()
            .map(|item| QueueEntry {
                name: names::item(self.rules(), *item),
                art: match *item {
                    ProductionItem::Unit(id) => names::art(&self.rules().units[id.0 as usize].key),
                    ProductionItem::Building(id) => {
                        names::art(&self.rules().buildings[id.0 as usize].key)
                    }
                    ProductionItem::Project(id) => {
                        names::art(&self.rules().module.projects[id.0 as usize].key)
                    }
                },
                cost: mods.production_cost(self.rules(), *item).unwrap_or(0),
            })
            .collect()
    }

    fn production_options(
        &mut self,
        city: CityHandle,
        core: &CityCore,
        income: &CityIncome,
    ) -> Vec<ProductionOption> {
        let mut items: Vec<ProductionItem> = (0..self.rules().units.len())
            .map(|i| ProductionItem::Unit(UnitKindId(i as u16)))
            .collect();
        items.extend((0..self.rules().buildings.len()).filter_map(|i| {
            let kind = BuildingKindId(i as u16);
            (!core.buildings.contains(&kind)).then_some(ProductionItem::Building(kind))
        }));
        items.extend(
            (0..self.rules().module.projects.len())
                .map(|i| ProductionItem::Project(classic_rules::rules::content::ProjectId(i as u16))),
        );
        // Other civilizations' unique units and buildings (and items no one
        // builds) are not in this civilization's list.
        let civ = self.game().view().auth.players().civ_of(self.me());
        items.retain(|item| self.rules().production_gate(*item, civ).is_none());
        let mods =
            classic_rules::effects::city::city_modifiers(self.game().view().auth, self.rules(), core);
        let mut options: Vec<ProductionOption> = items
            .into_iter()
            .map(|item| {
                self.production_option(
                    city,
                    core,
                    item,
                    income,
                    mods.production_cost(self.rules(), item).unwrap_or(0),
                )
            })
            .collect();
        // Buildable first; the stable sort keeps content order inside each group.
        options.sort_by_key(|option| !option.availability.is_ready());
        options
    }

    fn production_option(
        &mut self,
        city: CityHandle,
        core: &CityCore,
        item: ProductionItem,
        income: &CityIncome,
        cost: u32,
    ) -> ProductionOption {
        let player = self.me();
        let availability = self.availability(&Command::EnqueueProduction { player, city, item });
        let (key, kind) = match item {
            ProductionItem::Unit(k) => (self.rules().units[k.0 as usize].key.clone(), t!("classic.game.city.kind-unit")),
            ProductionItem::Building(k) => {
                let def = &self.rules().buildings[k.0 as usize];
                (
                    def.key.clone(),
                    if def.wonder { t!("classic.game.city.kind-wonder") } else { t!("classic.game.city.kind-building") },
                )
            }
            ProductionItem::Project(id) => {
                (self.rules().module.projects[id.0 as usize].key.clone(), t!("classic.game.city.kind-project"))
            }
        };
        ProductionOption {
            item,
            name: names::item(self.rules(), item),
            art: names::art(&key),
            kind,
            cost,
            turns: (self.production_toward(core, item, income) > 0)
                .then(|| cost.div_ceil(self.production_toward(core, item, income) as u32)),
            availability,
        }
    }

    /// Production per turn toward `item`: the city's rate with the item's
    /// own percents (Forge, Aristocracy, ...) instead of the head's.
    fn production_toward(&self, core: &CityCore, item: ProductionItem, income: &CityIncome) -> i32 {
        if core.queue.first() == Some(&item) {
            return income.production;
        }
        let auth = self.game().view().auth;
        let percent =
            classic_rules::effects::city::item_production_percent(auth, self.rules(), core, item);
        income.production_rate.toward(percent)
    }

    fn hurry_state(&mut self, city: CityHandle, core: &CityCore) -> (String, Availability) {
        let command = Command::HurryProduction {
            player: self.me(),
            city,
        };
        let availability = self.availability(&command);
        let price =
            classic_rules::effects::city::head_hurry_price(self.game().view().auth, self.rules(), core);
        let label = match price {
            Some(price) => tf!("classic.game.city.buy-price", price = price),
            None => t!("classic.game.city.buy").to_string(),
        };
        (label, availability)
    }

    fn clear_state(&mut self, city: CityHandle, core: &CityCore) -> Availability {
        if core.queue.is_empty() {
            return Availability::Blocked(reasons::queue_empty().into());
        }
        self.availability(&Command::ClearProduction {
            player: self.me(),
            city,
        })
    }

    fn building_entries(&self, core: &CityCore) -> Vec<BuildingEntry> {
        core.buildings
            .iter()
            .filter_map(|kind| self.rules().buildings.get(kind.0 as usize))
            .map(|def| BuildingEntry {
                name: names::named("building", &def.name, &def.key),
                art: names::art(&def.key),
                effect: effect_text(self.rules(), def),
            })
            .collect()
    }

    /// Own tiles in the city's three rings: worked or not, locked or not.
    fn worked_tiles(&mut self, city: CityHandle, core: &CityCore) -> Vec<WorkedTile> {
        let auth = self.game().view().auth;
        let work = city_work(auth, self.rules(), core);
        let ring = tiles_in_hex_radius(&auth.map().dimensions, core.tile, CITY_WORK_RADIUS);
        let own: Vec<TileId> = ring
            .into_iter()
            .filter(|t| *t != core.tile && auth.map().owning_city(*t) == Some(city))
            .collect();
        // Reserve manual assignments before offering another map assignment.
        // Automatic workers may move; deliberately locked workers must not silently
        // lose their tile merely because the new tile sorts earlier in the allocator.
        let forced: u32 = core.citizens.specialists.iter().map(|(kind, count)| {
            u32::from((*count).min(work.specialists.get(kind.0 as usize).copied().unwrap_or(0)))
        }).sum();
        let locked = own.iter().filter(|tile| core.citizens.locked.contains(tile)
            && !core.unworked.contains(tile)).count() as u32;
        let full = forced.saturating_add(locked) >= core.population;
        let mut rows = Vec::new();
        for tile in own {
            let label = self.tile_label(tile);
            let player = self.me();
            let availability = self.availability(&Command::ToggleWorkedTile { player, city, tile });
            let lock = if full && !core.citizens.locked.contains(&tile) {
                Availability::Blocked(reasons::all_assigned().into())
            } else {
                self.availability(&Command::LockWorkedTile { player, city, tile })
            };
            rows.push(WorkedTile {
                tile,
                label,
                worked: work.works(tile),
                locked: core.citizens.locked.contains(&tile),
                availability,
                lock,
            });
        }
        // Worked tiles first, then the rest, each group in tile order.
        rows.sort_by_key(|row| !row.worked);
        rows
    }

    fn tile_label(&self, tile: TileId) -> String {
        let auth = self.game().view().auth;
        let terrain = auth.map().terrain(tile).map(|t| t.0).unwrap_or(u16::MAX);
        let yields = tile_yields_for(auth, self.rules(), tile, self.me());
        format!("{}  ·  {}", names::terrain(terrain), yields_text(&yields))
    }

    /// "4 on tiles · 1 specialist · 0 unemployed".
    fn citizens_line(&self, core: &CityCore) -> String {
        let rules = self.rules();
        let work = city_work(self.game().view().auth, rules, core);
        let on_tiles = work.tiles().count().saturating_sub(1);
        let unemployed = match rules.roles.default_specialist {
            Some(kind) => work.specialists[kind.0 as usize] as u32,
            None => 0,
        };
        let specialists = work.specialist_count() - unemployed;
        tf!("classic.game.city.citizens-line", tiles = on_tiles, specialists = specialists, unemployed = unemployed)
    }

    /// Specialist kinds with slots in this city (the unemployed citizen
    /// last): who works there now and whether one more or one fewer can be
    /// set by hand.
    fn specialist_rows(&mut self, city: CityHandle, core: &CityCore) -> Vec<SpecialistRow> {
        let rules = self.rules();
        let work = city_work(self.game().view().auth, rules, core);
        let stats = city_breakdown(self.game().view().auth, rules, core)
            .modifiers
            .stats;
        let table = specialist_yield_table(rules, &stats);
        let slots = specialist_slots(rules, core);
        let forced: u32 = core.citizens.specialists.iter().map(|(kind, count)| {
            u32::from((*count).min(work.specialists.get(kind.0 as usize).copied().unwrap_or(0)))
        }).sum();
        let locked = work.tiles().filter(|tile| core.citizens.locked.contains(tile)).count() as u32;
        let full = forced.saturating_add(locked) >= core.population;
        let mut rows = Vec::new();
        for (i, def) in rules.specialists.iter().enumerate().take(slots.len()) {
            let kind = SpecialistId(i as u16);
            let unemployed = rules.roles.default_specialist == Some(kind);
            if slots[i] == 0 && !unemployed {
                continue;
            }
            rows.push((
                kind,
                names::named("specialist", &def.name, &def.key),
                work.specialists[i],
                slots[i],
                table[i],
                unemployed,
            ));
        }
        rows.into_iter()
            .map(|(kind, name, working, slots, yields, unemployed)| {
                let forced = core.citizens.forced(kind);
                let player = self.me();
                let set = |count: u8| Command::SetSpecialists {
                    player,
                    city,
                    specialist: kind,
                    count,
                };
                SpecialistRow {
                    kind,
                    name,
                    working,
                    slots,
                    forced,
                    yields: yields_text(&yields),
                    more: if unemployed {
                        Availability::Blocked(reasons::city_chooses().into())
                    } else if full {
                        Availability::Blocked(reasons::all_assigned().into())
                    } else {
                        self.availability(&set(forced.saturating_add(1)))
                    },
                    fewer: if forced == 0 {
                        Availability::Blocked(reasons::no_manual_specialist().into())
                    } else {
                        self.availability(&set(forced - 1))
                    },
                }
            })
            .collect()
    }

    /// Great person points of this city per kind with points, against the
    /// player's threshold.
    fn great_people_progress(&self, handle: CityHandle, core: &CityCore) -> Vec<GreatPersonProgress> {
        let auth = self.game().view().auth;
        let state = auth.cities().module_of(handle);
        let rules = self.rules();
        let yields = city_breakdown(auth, rules, core);
        let player_stats =
            classic_rules::effects::player::player_modifiers(auth, rules, core.owner).stats.clone();
        let per_turn = city_points(rules, &yields, &player_stats);
        let mut rows = Vec::new();
        for def in &rules.module.great_people {
            let Some(s) = def.specialist else { continue };
            let s = s.0 as usize;
            let points = state.and_then(|state| state.great_person_points.get(s)).copied().unwrap_or(0);
            let rate = per_turn.get(s).copied().unwrap_or(0);
            if points == 0 && rate == 0 {
                continue;
            }
            rows.push(GreatPersonProgress {
                name: names::unit(rules, def.unit),
                points: points / 100,
                threshold: great_person_threshold(auth, core.owner, def.kind),
                per_turn: rate / 100,
            });
        }
        rows
    }

    /// No process, Wealth, Research and the World Congress projects under
    /// way: whether the city can switch to each.
    fn process_options(
        &mut self,
        city: CityHandle,
        core: &CityCore,
    ) -> Vec<(
        Option<classic_rules::world::city_economy::Process>,
        String,
        bool,
        Availability,
    )> {
        use classic_rules::world::city_economy::Process;
        let player = self.me();
        let mut options = vec![
            (None, t!("classic.game.city.process-none").to_string()),
            (Some(Process::Wealth), t!("classic.game.city.process-wealth").to_string()),
            (Some(Process::Research), t!("classic.game.city.process-research").to_string()),
        ];
        for progress in self.game().view().auth.module().congress().projects() {
            let project = &self.rules().module.projects[progress.project.0 as usize];
            options.push((Some(Process::Contribute(progress.project)), names::named("project", &project.name, &project.key)));
        }
        options
            .into_iter()
            .map(|(process, label)| {
                let command = Command::SetProcess {
                    player,
                    city,
                    process,
                };
                (
                    process,
                    label,
                    core.economy.process == process,
                    self.availability(&command),
                )
            })
            .collect()
    }

    /// «Avoid Growth»: whether it is on, and whether the city can toggle it.
    fn avoid_growth_option(&mut self, city: CityHandle, core: &CityCore) -> (bool, Availability) {
        let player = self.me();
        let command = Command::SetAvoidGrowth { player, city, avoid: !core.citizens.avoid_growth };
        (core.citizens.avoid_growth, self.availability(&command))
    }

    /// Every focus with whether the city can switch to it.
    fn focus_options(
        &mut self,
        city: CityHandle,
        core: &CityCore,
    ) -> Vec<(CityFocus, bool, Availability)> {
        let player = self.me();
        CityFocus::ALL
            .into_iter()
            .map(|focus| {
                let command = Command::SetCityFocus {
                    player,
                    city,
                    focus,
                };
                (
                    focus,
                    core.citizens.focus == focus,
                    self.availability(&command),
                )
            })
            .collect()
    }

    /// Tiles this city can buy, from the core's own purchase rule and price.
    fn tile_offers(&mut self, city: CityHandle, core: &CityCore) -> Vec<TileOffer> {
        let auth = self.game().view().auth;
        let radius = PURCHASE_RADIUS_MAX as i32;
        let mut offers = Vec::new();
        for tile in tiles_in_hex_radius(&auth.map().dimensions, core.tile, radius) {
            let explored = self.view().explored.get(tile.0 as usize) == Some(&1);
            let Some(offer) = tile_purchase_offer(auth, self.rules(), city, tile) else {
                continue;
            };
            if explored {
                offers.push((tile, offer.price));
            }
        }
        offers
            .into_iter()
            .map(|(tile, price)| self.tile_offer(city, tile, price))
            .collect()
    }

    fn tile_offer(&mut self, city: CityHandle, tile: TileId, price: u32) -> TileOffer {
        let command = Command::PurchaseTile {
            player: self.me(),
            city,
            tile,
        };
        TileOffer {
            tile,
            label: self.purchase_tile_label(tile),
            price,
            availability: self.availability(&command),
        }
    }

    /// Use the observer projection for offered land, including remembered terrain.
    /// Resource names and yields must not reveal hidden deposits or current unseen changes.
    fn purchase_tile_label(&self, tile: TileId) -> String {
        let world = self.view();
        if world.explored.get(tile.0 as usize) != Some(&1) {
            return t!("classic.game.city.land-unexplored").into();
        }
        let Some(cell) = world.terrain.get(tile.0 as usize) else {
            return names::terrain(u16::MAX).into();
        };
        let mut label = names::terrain(cell.terrain).to_string();
        if let Some(resource) = cell
            .resource
            .filter(|r| world.unlocked_resources.contains(r))
            .and_then(|r| self.rules().resources.get(r as usize))
        {
            label.push_str(&format!(" · {}", names::named("resource", &resource.name, &resource.key))); // i18n-ok: a separator between names
        }
        if world.visible.get(tile.0 as usize) == Some(&1) {
            if let Some(y) = world.yields.get(tile.0 as usize) {
                let parts: Vec<_> = y
                    .iter()
                    .zip([
                        i18n::key!("classic.game.city.amount-food"),
                        i18n::key!("classic.game.city.amount-production"),
                        i18n::key!("classic.game.city.amount-gold"),
                        i18n::key!("classic.game.city.amount-science"),
                        i18n::key!("classic.game.city.amount-culture"),
                        i18n::key!("classic.game.city.amount-faith"),
                    ])
                    .filter(|(amount, _)| **amount != 0)
                    .map(|(amount, key)| i18n::tr_args(key, &[("n", (*amount).into())]))
                    .collect();
                if !parts.is_empty() {
                    label.push_str(&format!(" · {}", parts.join(", ")));
                }
            }
        }
        label
    }

    fn rename_city(&mut self, city: CityHandle, name: u16) {
        let player = self.me();
        let old = self.game().view().auth.cities().get(city).map(|c| c.name.clone());
        if self.submit(Command::ChooseCityName { player, city, name }) {
            let new = self.game().view().auth.cities().get(city).map(|c| c.name.clone());
            if let (Some(old), Some(new)) = (old, new) {
                self.notify(NoticeKind::Growth, tf!("classic.game.city.renamed", old = old, new = new));
            }
        }
    }

    pub(super) fn city_request(&mut self, request: GameRequest) {
        let Some((city, _)) = self.selected_city() else {
            return;
        };
        let player = self.me();
        let command = match request {
            GameRequest::Enqueue(item) => Command::EnqueueProduction { player, city, item },
            GameRequest::ClearQueue => Command::ClearProduction { player, city },
            GameRequest::Hurry => Command::HurryProduction { player, city },
            GameRequest::ToggleTile(tile) => Command::ToggleWorkedTile { player, city, tile },
            GameRequest::LockTile(tile) => Command::LockWorkedTile { player, city, tile },
            GameRequest::SetFocus(focus) => Command::SetCityFocus {
                player,
                city,
                focus,
            },
            GameRequest::SetAvoidGrowth(avoid) => Command::SetAvoidGrowth { player, city, avoid },
            GameRequest::SetProcess(process) => Command::SetProcess {
                player,
                city,
                process,
            },
            GameRequest::SetSpecialists(specialist, count) => Command::SetSpecialists {
                player,
                city,
                specialist,
                count,
            },
            GameRequest::BuyTile(tile) => Command::PurchaseTile { player, city, tile },
            GameRequest::RenameCity(name) => return self.rename_city(city, name),
            _ => return,
        };
        self.submit(command);
    }
}

/// "Golden age · 7 turns" while one runs, else the meter.
fn golden_age_text(
    auth: &classic_rules::world::state::AuthWorld,
    player: classic_rules::ids::PlayerId,
) -> String {
    let Some(economy) = auth.players().economy_of(player) else {
        return String::new();
    };
    if economy.golden_age_turns > 0 {
        return tf!("classic.game.city.golden-age-turns", n = economy.golden_age_turns);
    }
    let threshold = classic_rules::derived::happiness::golden_age_threshold(auth, player);
    tf!("classic.game.city.golden-age-meter", meter = economy.golden_age_meter, threshold = threshold)
}

fn growth_text(core: &CityCore, income: &CityIncome, threshold: u32) -> String {
    if income.food_surplus < 0 {
        return tf!("classic.game.city.starving", n = income.food_surplus);
    }
    if income.food_surplus_times100 <= 0 {
        return t!("classic.game.city.not-growing").into();
    }
    let missing = (threshold * 100).saturating_sub(core.food_times100);
    let turns = missing.div_ceil(income.food_surplus_times100 as u32);
    tf!("classic.game.city.grows-in", n = turns.max(1))
}

/// "2 food  1 prod  1 gold" (zero channels left out).
pub(super) fn yields_text(y: &Yields) -> String {
    let parts = [
        (y.food, i18n::key!("classic.game.city.short-food")),
        (y.production, i18n::key!("classic.game.city.short-production")),
        (y.gold, i18n::key!("classic.game.city.short-gold")),
        (y.science, i18n::key!("classic.game.city.short-science")),
        (y.culture, i18n::key!("classic.game.city.short-culture")),
        (y.module, i18n::key!("classic.game.city.short-faith")),
    ];
    let shown: Vec<String> = parts
        .iter()
        .filter(|(v, _)| *v != 0)
        .map(|(v, key)| i18n::tr_args(key, &[("n", (*v).into())]))
        .collect();
    if shown.is_empty() {
        t!("classic.game.city.nothing").into()
    } else {
        shown.join("  ")
    }
}

impl Session {
    fn yield_rows(&self, core: &CityCore, income: &CityIncome) -> Vec<(&'static str, String)> {
        let auth = self.game().view().auth;
        let (_, local) =
            classic_rules::derived::happiness::city_local_happiness(auth, self.rules(), core);
        let unhappy = classic_rules::derived::happiness::city_unhappiness(auth, self.rules(), core);
        let mut rows = base_yield_rows(income, self.city_defense(core));
        rows.push((t!("classic.game.city.row-happiness"), format!("+{local} / -{unhappy}")));
        let upkeep: u32 = core
            .buildings
            .iter()
            .filter_map(|b| self.rules().buildings.get(b.0 as usize))
            .map(|d| d.maintenance)
            .sum();
        rows.push((t!("classic.game.city.row-upkeep"), format!("-{upkeep}")));
        rows
    }
}

fn base_yield_rows(income: &CityIncome, defense: (i64, u16, u16)) -> Vec<(&'static str, String)> {
    let (strength, hp, max_hp) = defense;
    vec![
        (t!("classic.game.city.row-food"), format!("{:+}", income.food_surplus)),
        (t!("classic.game.city.row-production"), format!("{}", income.production)),
        (t!("classic.game.city.row-science"), format!("+{}", income.science)),
        (t!("classic.game.city.row-gold"), format!("{:+}", income.gold)),
        (t!("classic.game.city.row-culture"), format!("+{}", income.culture)),
        (t!("classic.game.city.row-faith"), format!("+{}", income.faith)),
        (
            t!("classic.game.city.row-defence"),
            i18n::number(strength as f64 / 100.0, 2, 2),
        ),
        (t!("classic.game.city.row-hit-points"), format!("{hp}/{max_hp}")),
    ]
}

/// The building's effects in Encyclopedia words (`ui::pedia::describe`).
fn effect_text(
    rules: &classic_rules::rules::CompiledRuleset,
    def: &classic_rules::rules::compiled::BuildingDefinition,
) -> String {
    let effects = &def.effects;
    let passive = [
        &effects.passive_player,
        &effects.passive_city,
        &effects.passive_unit,
        &effects.passive_tile,
    ];
    let parts: Vec<String> = passive
        .into_iter()
        .chain(effects.triggered.iter())
        .flatten()
        .map(|effect| crate::ui::pedia::describe::effect(rules, effect))
        .collect();
    if parts.is_empty() {
        t!("classic.game.city.no-effect").into()
    } else {
        parts.join("; ")
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    use crate::ui::model::Action;
    use crate::ui::screens::{
        game_rules, prepare_match, MapSize, MatchRequest, Screen, ScreenState,
    };
    use render::camera::CameraController;

    #[test]
    fn city_frame_snapshot_keeps_camera_equivalent_and_refreshes_after_purchase() {
        let rules = game_rules();
        let prepared = prepare_match(MatchRequest {
            civilization: "core:nation/rome".into(),
            seed: 20260928,
            size: MapSize::Small,
            players: 3,
            water: 25,
        })
        .unwrap();
        let mut save = classic_rules::persistence::encode::encode(&prepared.game, &rules);
        save.world.player_gold[0] = 5000;
        let mut session = Session::new(
            classic_rules::persistence::decode::decode(&save, &rules).unwrap(),
            rules,
        );
        session.request(GameRequest::Unit(Action::FoundCity));
        let before = session.screen_model().city.unwrap();
        let offer = before
            .offers
            .iter()
            .find(|o| o.availability.is_ready())
            .unwrap()
            .clone();
        for purchased in [false, true] {
            if purchased {
                session.request(GameRequest::BuyTile(offer.tile));
            }
            let snapshot = session.screen_model().city.unwrap();
            let separate = session.city_view().unwrap();
            assert_eq!(
                snapshot, separate,
                "preparing once does not change camera/UI data"
            );
            let world = session.view();
            let style =
                render::scene::style::compile(render::scene::style::default_style())
                    .unwrap();
            let surface = render::scene::terrain::surface::build(world, &style);
            let mut state = ScreenState::default();
            state.open(Screen::City);
            state.capture_city_buy(&snapshot);
            let (x, z) = render::scene::layout::tile_to_world(
                snapshot.tile % world.width,
                snapshot.tile / world.width,
            );
            for size in [egui::vec2(960., 640.), egui::vec2(1440., 900.)] {
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                let mut once = CameraController { x, y: z, zoom: 3. };
                let mut twice = once.clone();
                state.fit_city_buy_camera(world, &mut once, Some(&surface), screen, &snapshot);
                state.fit_city_buy_camera(world, &mut twice, Some(&surface), screen, &separate);
                assert_eq!(
                    once.view_proj(screen.aspect_ratio()),
                    twice.view_proj(screen.aspect_ratio())
                );
            }
            if purchased {
                assert_eq!(snapshot.treasury, before.treasury - offer.price);
                assert!(
                    snapshot.offers.iter().all(|o| o.tile != offer.tile),
                    "next frame must drop purchased offer"
                );
            }
        }
        session.client.selection = Selection::None;
        assert!(
            session.screen_model().city.is_none(),
            "selection loss must not retain cached city data"
        );
    }
}

#[cfg(test)]
#[path = "city_assignment_tests.rs"]
mod city_assignment_tests;
