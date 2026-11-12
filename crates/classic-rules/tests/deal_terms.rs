//! What a deal costs and what breaking it costs (classic/docs/classic/rules.md, Deals):
//! gold per turn the payer cannot pay breaks the deal, the creditor is told and remembers it,
//! and no deal takes on more gold a turn than the payer's income carries; the AI prices the
//! cities and resources it gives or takes by its own estimate (`ai::peace::Negotiation`), the
//! one its peace is priced by. The resources and units of the tests are picked from the rules'
//! data, never by name.

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::classic::ClassicChange;
use classic_rules::ids::{CityHandle, PlayerId, ResourceId, TerrainId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::rules::content::ResourceClass;
use classic_rules::knowledge::public::PlayerQueries;
use classic_rules::world::relations::{DealBlock, DealItem, PeaceRefusal};
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState};
use kernel::ai::driver::AiDriver;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);
const GRASSLAND: u16 = 1;

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

/// Cities of the world: P0's capital, a big city near it and a small one far off; P1's and
/// P2's capitals.
struct World {
    game: GameState,
    big: CityHandle,
    small: CityHandle,
}

/// Open grassland, three nations met, every one knowing every technology that reveals or
/// trades a resource (from the data).
fn world(rules: &CompiledRuleset) -> World {
    let mut game = GameState::new(rules, GameConfig { players: 3, width: 44, height: 20, rng_seed: 31 });
    let tiles = game.view().auth.map().dimensions.tile_count() as u32;
    let land: Vec<(TileId, TerrainId)> = (0..tiles).map(|t| (TileId(t), TerrainId(GRASSLAND))).collect();
    game.debug_set_terrain_batch(rules, &land);
    game.debug_found_city(rules, P0, tile(&game, 5, 10));
    let big = game.debug_found_city(rules, P0, tile(&game, 5, 3));
    game.debug_set_population(rules, big, 10);
    let small = game.debug_found_city(rules, P0, tile(&game, 33, 17));
    game.debug_found_city(rules, P1, tile(&game, 18, 10));
    game.debug_found_city(rules, P2, tile(&game, 40, 3));
    game.debug_meet_all();
    let techs: Vec<_> = rules.resources.iter().flat_map(|r| [r.required_tech, r.tradeable_with_tech]).flatten().collect();
    for p in [P0, P1, P2] {
        for t in &techs {
            game.debug_learn(rules, p, *t);
        }
    }
    World { game, big, small }
}

/// The first luxury of the rules that gives happiness.
fn luxury(rules: &CompiledRuleset) -> ResourceId {
    luxuries(rules)[0]
}

/// The luxuries of the rules that give happiness, in table order.
fn luxuries(rules: &CompiledRuleset) -> Vec<ResourceId> {
    let happy = |r: &&classic_rules::rules::compiled::ResourceDefinition| r.class == Some(ResourceClass::Luxury) && r.module.happiness > 0;
    rules.resources.iter().enumerate().filter(|(_, r)| happy(r)).map(|(i, _)| ResourceId(i as u16)).collect()
}

fn until_active(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    while game.view().auth.active_player() != player {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
}

/// `from` proposes on its turn and `to` accepts at once.
fn deal(game: &mut GameState, rules: &CompiledRuleset, from: PlayerId, to: PlayerId, items: Vec<(PlayerId, DealItem)>) {
    until_active(game, rules, from);
    let offer = game.view().auth.module().relations().next_offer_id();
    game.execute(rules, Command::ProposeDeal { from, to, items }).unwrap();
    game.execute(rules, Command::RespondDeal { player: to, offer, accept: true, why: PeaceRefusal::Unsaid }).unwrap();
}

/// Every player ends its turn until a new round begins (the deals are settled once a round,
/// after every income is in); the module changes of those turns.
fn end_round(game: &mut GameState, rules: &CompiledRuleset) -> Vec<ClassicChange> {
    let round = game.view().auth.turn().round;
    let mut changes = Vec::new();
    while game.view().auth.turn().round == round {
        let active = game.view().auth.active_player();
        changes.extend(game.execute(rules, Command::EndTurn { player: active }).unwrap().changes().module_changes.iter().cloned());
    }
    changes
}

/// `by` (at war with `victim`) takes every city of `victim`: the nation is gone.
fn eliminate(game: &mut GameState, rules: &CompiledRuleset, victim: PlayerId, by: PlayerId) {
    until_active(game, rules, by);
    game.execute(rules, Command::DeclareWar { player: by, enemy: victim }).unwrap();
    let cities: Vec<(CityHandle, TileId)> = game.view().auth.indexes().cities_of(victim).iter().map(|&c| (c, game.view().auth.cities().get(c).unwrap().tile)).collect();
    for (city, at) in cities {
        let hex = game.view().auth.map().dimensions.to_coord(at).unwrap();
        let beside = tile(game, hex.q - 1, hex.r);
        let attacker = game.debug_spawn_unit(rules, by, beside, warrior(rules));
        let core = game.view().auth.cities().get(city).unwrap();
        let max = classic_rules::combat::city::max_hit_points(game.view().auth, rules, core);
        game.debug_set_city_damage(city, max - 1);
        game.execute(rules, Command::AttackCity { attacker, city }).unwrap();
    }
    assert!(!game.view().auth.players().is_alive(victim), "the nation is gone");
}

/// The cheapest buildable land unit of the data.
fn warrior(rules: &CompiledRuleset) -> UnitKindId {
    rules
        .units
        .iter()
        .enumerate()
        .filter(|(_, u)| !u.unbuildable && u.cost > 0 && u.strength > 0 && u.required_resources.is_empty() && u.nation.is_none())
        .min_by_key(|(_, u)| u.cost)
        .map(|(i, _)| UnitKindId(i as u16))
        .unwrap()
}

fn trades_between(game: &GameState, a: PlayerId, b: PlayerId) -> usize {
    let pair = |x: PlayerId, y: PlayerId| (x, y) == (a, b) || (x, y) == (b, a);
    game.view().auth.module().relations().trades().iter().filter(|t| pair(t.from, t.to)).count()
}

fn gold_income(game: &GameState, rules: &CompiledRuleset, player: PlayerId) -> i32 {
    classic_rules::world::relations::gold_income(game.view().auth, rules, player)
}

/// Units that cost upkeep: warriors-like units of the data (the cheapest buildable land unit)
/// until the player's gold a turn falls below `below`.
fn burden(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId, below: i32) {
    let cheapest = rules
        .units
        .iter()
        .enumerate()
        .filter(|(_, u)| !u.unbuildable && u.cost > 0 && u.strength > 0 && u.required_resources.is_empty() && u.nation.is_none())
        .min_by_key(|(_, u)| u.cost)
        .map(|(i, _)| UnitKindId(i as u16))
        .unwrap();
    let capital = tile(game, 5, 10);
    for _ in 0..60 {
        if classic_rules::derived::income::player_income(game.view().auth, rules, player).gold < below {
            return;
        }
        game.debug_spawn_unit(rules, player, capital, cheapest);
    }
    panic!("upkeep never brought the income below {below}");
}

/// P0 pays P1 gold a turn for a luxury of P1 (the pair's trades both ways), then its income
/// falls and its treasury is empty: at its turn's end the payment is not made, every trade of
/// the deal ends, P1 is told (`DealBroken`) and remembers it (warmonger memory, read by the
/// AI's trust); nothing of the payment reaches P1. A treasury that covers the payment pays it
/// in full and the deal lives.
#[test]
fn an_unpaid_gold_per_turn_breaks_the_deal_and_the_creditor_remembers() {
    let rules = compile_rules(core_rules()).unwrap();
    let wine = luxury(&rules);
    let run = |treasury: u32| {
        let World { mut game, .. } = world(&rules);
        let p1_capital = tile(&game, 18, 10);
        game.debug_set_resource_batch(&rules, &[(p1_capital, wine)]);
        let gold = gold_income(&game, &rules, P0) as u32;
        assert!(gold > 0, "three cities earn gold");
        deal(&mut game, &rules, P0, P1, vec![(P0, DealItem::GoldPerTurn(gold)), (P1, DealItem::Resource { resource: wine, amount: 1 })]);
        assert_eq!(trades_between(&game, P0, P1), 2);
        burden(&mut game, &rules, P0, 0);
        game.debug_earn(P0, treasury);
        let p1_before = game.view().auth.players().treasury_of(P1);
        let p1_income = classic_rules::derived::income::player_income(game.view().auth, &rules, P1).gold.max(0) as u32;
        until_active(&mut game, &rules, P0);
        let changes = end_round(&mut game, &rules);
        let broken = changes.iter().find_map(|c| match *c {
            ClassicChange::DealBroken { debtor, creditor, gold } => Some((debtor, creditor, gold)),
            _ => None,
        });
        let received = game.view().auth.players().treasury_of(P1) - p1_before;
        (game, gold, broken, received, p1_income)
    };

    let (game, gold, broken, received, p1_income) = run(0);
    assert_eq!(broken, Some((P0, P1, gold)), "the payment it cannot make breaks the deal");
    assert_eq!(trades_between(&game, P0, P1), 0, "the deal ends both ways: the gold and the luxury");
    assert_eq!(received, p1_income, "nothing of the payment is taken: P1 has its own income only");
    let memory = game.view().auth.module().relations().warmonger(P1, P0);
    assert!(memory >= classic_rules::ai::diplomacy::DISTRUST_WARMONGER, "the creditor remembers: {memory}");
    assert_eq!(game.view().auth.module().relations().warmonger(P2, P0), 0, "only the creditor");
    let p1 = game.ai_observation(&rules, P1);
    let trust = p1.relations.iter().find(|r| r.other == P0).is_some_and(|r| classic_rules::ai::diplomacy::trusted(p1.round, r));
    assert!(!trust, "the AI no longer trusts the debtor");

    let (game, gold, broken, received, p1_income) = run(10_000);
    assert_eq!(broken, None, "a treasury that covers it pays");
    assert_eq!(received, gold + p1_income, "in full, beside its own income");
    assert_eq!(trades_between(&game, P0, P1), 2);
}

/// A nation that is gone pays nothing and takes nothing: its deals end both ways when it dies
/// (at the next turn end), and until then the creditor does not count the gold it will never
/// get (it used to lend on it and break its own deal).
#[test]
fn the_deals_of_a_dead_nation_end_and_its_creditor_stops_counting_them() {
    let rules = compile_rules(core_rules()).unwrap();
    let wine = luxury(&rules);
    let World { mut game, .. } = world(&rules);
    let p1_capital = tile(&game, 18, 10);
    game.debug_set_resource_batch(&rules, &[(p1_capital, wine)]);
    let own = gold_income(&game, &rules, P1);
    let gold = (gold_income(&game, &rules, P0) as u32).min(60);
    assert!(gold > 0);
    deal(&mut game, &rules, P0, P1, vec![(P0, DealItem::GoldPerTurn(gold)), (P1, DealItem::Resource { resource: wine, amount: 1 })]);
    assert_eq!(gold_income(&game, &rules, P1), own + gold as i32, "the deal counts while the payer lives");

    eliminate(&mut game, &rules, P0, P2);
    assert_eq!(gold_income(&game, &rules, P1), own, "a dead payer's gold is not counted");
    let before = game.view().auth.players().treasury_of(P1);
    let changes = end_round(&mut game, &rules);
    assert_eq!(trades_between(&game, P0, P1), 0, "the deal is closed both ways, the luxury too");
    assert!(changes.iter().all(|c| !matches!(c, ClassicChange::DealBroken { .. })), "a death is no breach of the payer");
    assert!(game.view().auth.players().treasury_of(P1) - before <= own.max(0) as u32 + 1, "nothing was paid by the dead");
    assert_eq!(gold_income(&game, &rules, P1), own);
}

/// The order of the players decides nothing: B (who moves first) pays C what A (who moves
/// later) pays B, and B's own treasury could not carry it. The deals are settled once a round,
/// after every income is in, so A's payment reaches B before B's is due and nothing breaks.
#[test]
fn a_debtor_covered_by_what_it_receives_pays_whatever_the_order_of_the_players() {
    let rules = compile_rules(core_rules()).unwrap();
    let World { mut game, .. } = world(&rules);
    let gold = gold_income(&game, &rules, P1) as u32;
    assert!(gold > 2);
    // P1 (later in the round than P0) pays P0; P0 (first) passes it on to P2.
    deal(&mut game, &rules, P1, P0, vec![(P1, DealItem::GoldPerTurn(gold))]);
    deal(&mut game, &rules, P0, P2, vec![(P0, DealItem::GoldPerTurn(gold))]);
    // P0's upkeep eats its income: its treasury after its own turn is short of the instalment,
    // but not of what P1 sends it.
    burden(&mut game, &rules, P0, -(gold as i32) - 1);
    let treasury = game.view().auth.players().treasury_of(P0);
    let income = classic_rules::derived::income::player_income(game.view().auth, &rules, P0).gold;
    let after_its_turn = treasury as i32 + income;
    assert!((0..gold as i32).contains(&after_its_turn), "P0 alone cannot carry {gold} but is not in debt: {treasury} + {income}");
    let p2_before = game.view().auth.players().treasury_of(P2);
    until_active(&mut game, &rules, P0);
    let changes = end_round(&mut game, &rules);
    assert!(changes.iter().all(|c| !matches!(c, ClassicChange::DealBroken { .. })), "{changes:?}");
    assert_eq!(trades_between(&game, P0, P2), 1);
    assert_eq!(trades_between(&game, P1, P0), 1);
    assert!(game.view().auth.players().treasury_of(P2) >= p2_before + gold, "the end of the chain is paid");
}

/// No deal takes on more gold a turn than the income carries: what the payer already pays by
/// deals counts (the original game's net income), so two deals of the whole income cannot
/// stack.
#[test]
fn gold_per_turn_cannot_exceed_the_income_with_the_deals_already_paid() {
    let rules = compile_rules(core_rules()).unwrap();
    let World { mut game, .. } = world(&rules);
    let income = gold_income(&game, &rules, P0);
    assert!(income > 1);
    deal(&mut game, &rules, P0, P1, vec![(P0, DealItem::GoldPerTurn(income as u32 - 1))]);
    assert_eq!(gold_income(&game, &rules, P0), 1, "the deal takes its share of the income");
    let view = game.ai_observation(&rules, P0).view;
    assert_eq!(view.deal_item_problem(P0, P2, DealItem::GoldPerTurn(2)), Some(DealBlock::NotEnoughIncome));
    assert_eq!(view.deal_item_problem(P0, P2, DealItem::GoldPerTurn(1)), None);
}

/// The AI's answer to `items` offered by `from` (a human) on the AI's turn; `before` runs on the
/// AI's turn before it thinks.
fn ask(game: &mut GameState, rules: &CompiledRuleset, ai: PlayerId, from: PlayerId, items: Vec<(PlayerId, DealItem)>, before: impl FnOnce(&mut GameState)) -> Option<bool> {
    until_active(game, rules, from);
    game.execute(rules, Command::ProposeDeal { from, to: ai, items }).unwrap();
    until_active(game, rules, ai);
    before(game);
    let settings = AiSettings::for_rules(rules, rules.default_difficulty());
    let humans: Vec<PlayerId> = [P0, P1, P2].into_iter().filter(|p| *p != ai).collect();
    let mut driver = AiDriver::with_mind(settings, humans, ClassicMind::new(Variant::Standard));
    driver.set_trace_capacity(100_000);
    driver.run(game, rules, u32::MAX);
    let rejected = driver.trace().entries().any(|e| matches!(e.command, Some(Command::RespondDeal { .. })) && matches!(e.outcome, kernel::ai::trace::Outcome::Rejected(_)));
    assert!(!rejected, "the AI sends no answer the rules refuse");
    let answer = driver.trace().entries().find_map(|e| match e.command {
        Some(Command::RespondDeal { accept, .. }) => Some(accept),
        _ => None,
    });
    answer
}

/// Friends: gold may pass in a deal.
fn friends(game: &mut GameState, rules: &CompiledRuleset) {
    deal(game, rules, P1, P0, vec![(P1, DealItem::Friendship)]);
}

/// The AI takes on gold a turn only within what its income carries, by the rules' own number
/// (`gold_income`): an offer it would take is refused once another deal has taken the income.
#[test]
fn the_ai_takes_on_no_more_gold_a_turn_than_its_income_carries() {
    let rules = compile_rules(core_rules()).unwrap();
    let offer = |game: &GameState| {
        let income = gold_income(game, &rules, P0) as u32;
        vec![(P0, DealItem::GoldPerTurn(income)), (P1, DealItem::Gold(100_000))]
    };
    let mut take = world(&rules).game;
    friends(&mut take, &rules);
    take.debug_earn(P1, 100_000);
    let items = offer(&take);
    assert_eq!(ask(&mut take, &rules, P0, P1, items, |_| {}), Some(true), "a fair price within the income is taken");

    let mut full = world(&rules).game;
    friends(&mut full, &rules);
    full.debug_earn(P1, 100_000);
    let items = offer(&full);
    let taken = |game: &mut GameState| {
        // On its turn, before it answers, the AI's income goes to another deal.
        let income = gold_income(game, &rules, P0) as u32;
        let id = game.view().auth.module().relations().next_offer_id();
        game.execute(&rules, Command::ProposeDeal { from: P0, to: P2, items: vec![(P0, DealItem::GoldPerTurn(income))] }).unwrap();
        game.execute(&rules, Command::RespondDeal { player: P2, offer: id, accept: true, why: PeaceRefusal::Unsaid }).unwrap();
    };
    assert_eq!(ask(&mut full, &rules, P0, P1, items, taken), Some(false), "beyond the income: refused, not sent to be refused");
}

/// The least gold `P1` pays for `item` given by the AI (P0), by halving the range: the AI
/// accepts at the price and refuses a gold less. None: not even at `ceiling`.
fn price(rules: &CompiledRuleset, item: impl Fn(&World) -> DealItem, setup: impl Fn(&mut World), ceiling: u32) -> Option<u32> {
    let accepts = |gold: u32| {
        let mut w = world(rules);
        setup(&mut w);
        friends(&mut w.game, rules);
        w.game.debug_earn(P1, ceiling);
        let mut items = vec![(P0, item(&w))];
        if gold > 0 {
            items.push((P1, DealItem::Gold(gold)));
        }
        ask(&mut w.game, rules, P0, P1, items, |_| {}) == Some(true)
    };
    if !accepts(ceiling) {
        return None;
    }
    let (mut lo, mut hi) = (0u32, ceiling);
    if accepts(0) {
        return Some(0);
    }
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if accepts(mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    Some(hi)
}

/// The AI prices its cities by its estimate: the capital is never given, a city of ten is
/// not given for 600 gold, a small far city is given for its worth (a gold less: refused).
#[test]
fn the_ai_gives_a_city_for_its_worth_and_never_its_capital() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut w = world(&rules);
    friends(&mut w.game, &rules);
    let capital = w.game.view().auth.indexes().city_on_tile(tile(&w.game, 5, 10)).unwrap();
    until_active(&mut w.game, &rules, P1);
    let refused = w.game.execute(&rules, Command::ProposeDeal { from: P1, to: P0, items: vec![(P0, DealItem::City(capital)), (P1, DealItem::Gold(1))] });
    assert!(refused.is_err(), "the capital is not a city to give");

    let ceiling = 200_000;
    let big = price(&rules, |w| DealItem::City(w.big), |_| {}, ceiling);
    let small = price(&rules, |w| DealItem::City(w.small), |_| {}, ceiling).expect("a small far city has a price");
    eprintln!("city prices: big (10) {big:?}, small (1) {small}");
    assert!(big.is_none_or(|b| b > 600), "a city of ten is not given for 600: {big:?}");
    assert!(big.is_none_or(|b| b > small), "a big city costs more than a small one");
    assert!(small > 0);
}

/// The AI prices a luxury by the happiness its kind gives for the deal: its only copy costs
/// more than a spare one, and a spare one still costs the rival's gain (not given away).
#[test]
fn the_ai_prices_a_luxury_by_what_its_kind_gives() {
    let rules = compile_rules(core_rules()).unwrap();
    let wine = luxury(&rules);
    let ceiling = 50_000;
    let item = |_: &World| DealItem::Resource { resource: wine, amount: 1 };
    let only = |w: &mut World| {
        let at = tile(&w.game, 5, 10);
        w.game.debug_set_resource_batch(&rules, &[(at, wine)]);
    };
    let spare = |w: &mut World| {
        let (a, b) = (tile(&w.game, 5, 10), tile(&w.game, 5, 3));
        w.game.debug_set_resource_batch(&rules, &[(a, wine), (b, wine)]);
    };
    let only = price(&rules, item, only, ceiling).expect("its only copy has a price");
    let spare = price(&rules, item, spare, ceiling).expect("a spare copy has a price");
    eprintln!("luxury prices: only copy {only}, spare copy {spare}");
    assert!(only > spare, "the last copy costs its happiness: {only} against {spare}");
    assert!(spare > 0, "a spare copy is the rival's gain");
}

/// A swap of the last copies of two luxuries is even for the AI (it gains a kind, loses one, and
/// so does the rival): it takes it, as before the estimate; a gift of its last copy it refuses.
#[test]
fn the_ai_swaps_last_copies_of_luxuries_and_gives_none_away() {
    let rules = compile_rules(core_rules()).unwrap();
    let kinds = luxuries(&rules);
    let (ours, theirs) = (kinds[0], kinds[1]);
    let mut setup = || {
        let mut w = world(&rules);
        let (a, b) = (tile(&w.game, 5, 10), tile(&w.game, 18, 10));
        w.game.debug_set_resource_batch(&rules, &[(a, ours), (b, theirs)]);
        w
    };
    let one = |resource| DealItem::Resource { resource, amount: 1 };
    let mut w = setup();
    let swap = ask(&mut w.game, &rules, P0, P1, vec![(P0, one(ours)), (P1, one(theirs))], |_| {});
    assert_eq!(swap, Some(true), "a swap of last copies");
    let mut w = setup();
    let gift = ask(&mut w.game, &rules, P0, P1, vec![(P0, one(ours))], |_| {});
    assert_eq!(gift, Some(false), "no luxury for nothing");
}

/// A strategic resource no unit the AI may build needs yet is not worth nothing: the AI prices
/// its copy by the unit the rules open next that needs it (the same way as one with a unit
/// now), so it is not given away for 0. Picked from the data: a resource whose every unit lies
/// beyond the technologies of the nations.
#[test]
fn the_ai_does_not_give_a_strategic_resource_without_a_unit_yet_for_nothing() {
    let rules = compile_rules(core_rules()).unwrap();
    let known = world(&rules).game.ai_observation(&rules, P0).economy.techs.clone();
    let needs = |u: &classic_rules::rules::compiled::UnitDefinition, r: ResourceId| u.required_resources.iter().any(|(x, _)| *x == r);
    let resource = rules
        .resources
        .iter()
        .enumerate()
        .filter(|(_, r)| r.class == Some(ResourceClass::Strategic))
        .map(|(i, _)| ResourceId(i as u16))
        .find(|&r| {
            let units: Vec<_> = rules.units.iter().filter(|u| !u.unbuildable && u.nation.is_none() && needs(u, r)).collect();
            !units.is_empty() && units.iter().all(|u| u.required_tech.is_some_and(|t| !known.contains(&t)))
        })
        .expect("the rules have a strategic resource whose units lie beyond the starting technologies");
    let only = |w: &mut World| {
        let at = tile(&w.game, 5, 10);
        w.game.debug_set_resource_batch(&rules, &[(at, resource)]);
    };
    let worth = price(&rules, |_| DealItem::Resource { resource, amount: 1 }, only, 200_000).expect("it has a price");
    assert!(worth > 0, "the copy is not given away: {worth}");
}
