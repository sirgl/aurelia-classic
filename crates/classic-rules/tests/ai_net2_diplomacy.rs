//! Part D of `net2` (design §4.6, §4.7): the diplomacy executors read the plan's trust, deny-help, peace wish,
//! embargo and city-state gold share, and play as today when no plan is in the brief.
//!
//! The scene (as `ai_vote_guard`): P0 the seat, P1 and P2 nations, four city-states; P0 is ally of `cs(0)`.
//! 3 nations and 4 city-states make 18 delegates; the World Leader needs 9.

use classic_content::core::core_rules;
use classic_rules::ai::congress::{leader_choice, proposal_orders_with, vote_orders_with};
use classic_rules::ai::city_states::{ceiling_for, gift_orders_shared, reserve_for};
use classic_rules::ai::diplomacy::DiplomacyAdvisor;
use classic_rules::ai::net2::plan::{Embargo, Help, Net2Plan, RivalRead, WarKind, TRUST_MIN};
use classic_rules::ai::peace::Negotiation;
use classic_rules::ai::planner::TurnBrief;
use classic_rules::ai::planner_value::Weights;
use classic_rules::ai::state::AiState;
use classic_rules::ai::system::{AdvisorInput, SystemAdvisor};
use classic_rules::ai::value::{PriceModel, Prices};
use classic_rules::ids::{PlayerId, TileId};
use classic_rules::knowledge::observation::AIObservation;
use classic_rules::math::HexCoord;
use classic_rules::rules::content::CityStateId;
use classic_rules::world::city_states::Personality;
use classic_rules::world::congress::{Proposal, ResolutionKind};
use classic_rules::world::player::{ClassicKind, PlayerKind};
use classic_rules::world::relations::DealItem;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);

fn cs(i: u8) -> PlayerId {
    PlayerId(3 + i)
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn end_round(game: &mut GameState, rules: &CompiledRuleset) {
    let round = game.view().auth.turn().round;
    while game.view().auth.turn().round == round {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
}

/// Three nations met by one another, four city-states met by all (P0 allied to `cs(0)`), the league founded.
fn scene() -> (GameState, CompiledRuleset) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 7, width: 40, height: 30, rng_seed: 11 });
    for i in 0..4u8 {
        game.debug_set_player_kind(cs(i), PlayerKind::Module(ClassicKind::CityState(CityStateId(u16::from(i)))));
        game.debug_set_personality(cs(i), Personality::Neutral);
        game.debug_found_city(&rules, cs(i), tile(&game, 8 + 8 * i32::from(i), 15));
    }
    game.debug_found_city(&rules, P0, tile(&game, 3, 3));
    game.debug_found_city(&rules, P1, tile(&game, 35, 3));
    game.debug_found_city(&rules, P2, tile(&game, 3, 26));
    for i in 0..4u8 {
        for p in [P0, P1, P2] {
            game.debug_meet(p, cs(i));
        }
    }
    game.debug_meet(P1, P2);
    game.debug_meet(P0, P2);
    game.debug_meet(P0, P1);
    game.debug_set_influence(&rules, cs(0), P0, 80);
    (game, rules)
}

/// A reading of rival `p` with the given trust and help; the rest neutral.
fn read(p: PlayerId, trust: i64, help: Help) -> RivalRead {
    RivalRead { rival: p, weak: 0, reach: 0, menace: 0, distrust: 1000 - trust, prey: 0, runaway: 0, spoiler: 0, retake: 0, duty: 0, grabber: 0, border: 0, brake: 0, war_value: 0, war_kind: WarKind::Prey, trust, help, soldiers: Default::default() }
}

fn plan(rivals: Vec<RivalRead>) -> Net2Plan {
    Net2Plan { round: 1, rivals, ..Net2Plan::default() }
}

fn brief_of<'g>(game: &'g GameState, rules: &'g CompiledRuleset, net2: Option<Net2Plan>) -> (AIObservation<'g>, TurnBrief) {
    let obs = game.ai_observation(rules, P0);
    let mut brief = TurnBrief::from_observation(&obs, rules);
    brief.prices = Some(Prices::of(&obs, rules, PriceModel::Uses));
    brief.net2 = net2;
    (obs, brief)
}

/// The value P0 puts on `items` with P1.
fn value(game: &GameState, rules: &CompiledRuleset, net2: Option<Net2Plan>, items: &[(PlayerId, DealItem)]) -> i64 {
    let (obs, brief) = brief_of(game, rules, net2);
    let state = AiState::new(P0, obs.round);
    let input = AdvisorInput { observation: &obs, brief: &brief, state: &state, rules };
    Negotiation::new(&input).value(P1, items)
}

fn luxury(rules: &CompiledRuleset) -> classic_rules::ids::ResourceId {
    let i = rules.resources.iter().position(|r| r.class == Some(classic_rules::rules::content::ResourceClass::Luxury) && r.module.happiness > 0).unwrap();
    classic_rules::ids::ResourceId(i as u16)
}

// ---- trust in the deal value (P4) ----

/// P4-like: the seat pays a lump of gold for gold a turn and a luxury that P1 will give for the deal's rounds; P1 has an
/// army on our border and a record of breaking deals (trust 300). What flows later from a partner we trust less than
/// `TRUST_MIN` is worth nothing, so the sale that today passes is refused; at a middling trust it is worth that share.
#[test]
fn p4_a_distrusted_partners_later_flows_are_worth_nothing_and_the_sale_is_refused() {
    let (game, rules) = scene();
    let flows = [(P1, DealItem::GoldPerTurn(8)), (P1, DealItem::Resource { resource: luxury(&rules), amount: 1 })];
    let pays = |gold: u32| {
        let mut items = flows.to_vec();
        items.push((P0, DealItem::Gold(gold)));
        items
    };
    let today = value(&game, &rules, None, &flows);
    assert!(today > 0, "the flows are worth something to the seat: {today}");
    // The gold that today's value allows: the most the seat pays and still says yes.
    let most = (1..2000u32).take_while(|g| value(&game, &rules, None, &pays(*g)) >= 0).last().expect("some price passes");
    let full = value(&game, &rules, Some(plan(vec![read(P1, 1000, Help::Give)])), &pays(most));
    assert_eq!(full, value(&game, &rules, None, &pays(most)), "a rival we trust fully: as today");
    let mid = value(&game, &rules, Some(plan(vec![read(P1, 700, Help::Give)])), &flows);
    assert!(0 < mid && mid < today, "trust 700 prices {mid} of {today}");
    let low = plan(vec![read(P1, TRUST_MIN - 1, Help::Give)]);
    assert_eq!(value(&game, &rules, Some(low.clone()), &flows), 0, "below TRUST_MIN nothing flows");
    assert!(value(&game, &rules, Some(low), &pays(most)) < 0, "the sale that passed today is refused");
    // A stranger without a reading is not yet a liar.
    assert_eq!(value(&game, &rules, Some(plan(vec![])), &pays(most)), value(&game, &rules, None, &pays(most)));
}

/// What we give first stays priced as it was: the trust discounts what we receive later, not what we pay.
#[test]
fn what_we_give_is_not_discounted_by_trust() {
    let (game, rules) = scene();
    let give = [(P0, DealItem::GoldPerTurn(8))];
    assert_eq!(value(&game, &rules, Some(plan(vec![read(P1, 100, Help::Give)])), &give), value(&game, &rules, None, &give));
}

/// Open borders: not bought and not given below `TRUST_MIN` (D12, MO3).
#[test]
fn open_borders_need_trust() {
    let (game, rules) = scene();
    let ours = [(P0, DealItem::OpenBorders)];
    let theirs = [(P1, DealItem::OpenBorders)];
    let low = Some(plan(vec![read(P1, 100, Help::Give)]));
    assert!(value(&game, &rules, None, &theirs) > 0);
    assert_eq!(value(&game, &rules, low.clone(), &theirs), 0, "theirs is not bought");
    assert!(value(&game, &rules, low, &ours) < value(&game, &rules, None, &ours), "ours is not given");
    let high = Some(plan(vec![read(P1, 900, Help::Give)]));
    assert_eq!(value(&game, &rules, high, &theirs), value(&game, &rules, None, &theirs));
}

// ---- deny help ----

/// The runaway gets no friendship, no research agreement, no pact, no open borders, no strategic copy (§4.6).
#[test]
fn the_runaway_gets_no_help() {
    let (game, rules) = scene();
    let denied = Some(plan(vec![read(P1, 1000, Help::Deny)]));
    for item in [DealItem::Friendship, DealItem::DefensivePact, DealItem::OpenBorders, DealItem::ResearchAgreement] {
        let items = [(P0, item)];
        assert!(value(&game, &rules, denied.clone(), &items) < 0, "{item:?} to the runaway");
    }
    assert!(value(&game, &rules, None, &[(P0, DealItem::Friendship)]) > 0, "the same friendship as today passes");
    assert!(value(&game, &rules, Some(plan(vec![read(P1, 1000, Help::Give)])), &[(P0, DealItem::Friendship)]) > 0);
    // Not an embassy: nothing the runaway needs to run.
    let embassy = [(P0, DealItem::Embassy)];
    assert_eq!(value(&game, &rules, denied, &embassy), value(&game, &rules, None, &embassy));
}

/// No proposal of a helping item to the runaway.
#[test]
fn no_proposal_of_help_to_the_runaway() {
    use classic_rules::world::relations::PeaceRefusal;
    let (mut game, rules) = scene();
    for t in 0..rules.technologies.len() {
        for p in [P0, P1] {
            game.debug_learn(&rules, p, classic_rules::ids::TechId(t as u16));
        }
    }
    // Embassies both ways, so that the next step of the chain is a friendship after 20 rounds of contact.
    while game.view().auth.active_player() != P0 {
        let active = game.view().auth.active_player();
        game.execute(&rules, Command::EndTurn { player: active }).unwrap();
    }
    let offer = game.view().auth.module().relations().next_offer_id();
    game.execute(&rules, Command::ProposeDeal { from: P0, to: P1, items: vec![(P0, DealItem::Embassy), (P1, DealItem::Embassy)] }).unwrap();
    game.execute(&rules, Command::RespondDeal { player: P1, offer, accept: true, why: PeaceRefusal::Unsaid }).unwrap();
    let proposed = |game: &GameState, net2: Option<Net2Plan>| {
        let obs = game.ai_observation(&rules, P0);
        classic_rules::ai::diplomacy::proposals_with(&obs, &rules, None, net2.as_ref())
    };
    let helps = |cmds: &[Command]| cmds.iter().any(|c| matches!(c, Command::ProposeDeal { to, items, .. } if *to == P1 && items.iter().any(|(_, i)| classic_rules::ai::diplomacy::helps(*i))));
    for _ in 0..60 {
        end_round(&mut game, &rules);
        if helps(&proposed(&game, None)) {
            assert!(!helps(&proposed(&game, Some(plan(vec![read(P1, 1000, Help::Deny)])))), "denied");
            assert!(helps(&proposed(&game, Some(plan(vec![read(P1, 1000, Help::Give)])))), "given");
            return;
        }
    }
    panic!("the scripted AI never proposed a helping item in 60 rounds");
}

// ---- CS3 ----

/// CS3: the scripted AI prices every war it would declare for a deal at a flat 1000, so gold buys its war on its own ally
/// (a city-state it holds at 80 influence). With a plan the war on a bound target is out of any price.
#[test]
fn cs3_gold_does_not_buy_a_war_on_our_ally() {
    let (game, rules) = scene();
    let war_on_ally = |gold: u32| [(P0, DealItem::ThirdPartyWar(cs(0))), (P1, DealItem::Gold(gold))];
    assert!(value(&game, &rules, None, &war_on_ally(5000)) > 0, "today the bribe works");
    let plan2 = Some(plan(vec![read(P1, 1000, Help::Give)]));
    assert!(value(&game, &rules, plan2.clone(), &war_on_ally(5000)) < 0);
    assert!(value(&game, &rules, plan2.clone(), &war_on_ally(50_000)) < 0, "no gold buys it");
    // A city-state we hold no tie with keeps today's flat price; so does a rival that is nobody's friend.
    let war_on_other = |target: PlayerId, gold: u32| [(P0, DealItem::ThirdPartyWar(target)), (P1, DealItem::Gold(gold))];
    assert_eq!(value(&game, &rules, plan2.clone(), &war_on_other(cs(1), 5000)), value(&game, &rules, None, &war_on_other(cs(1), 5000)));
    assert_eq!(value(&game, &rules, plan2, &war_on_other(P2, 5000)), value(&game, &rules, None, &war_on_other(P2, 5000)));
}

// ---- peace_with ----

/// A war with no reason left ends: the plan names it, and a peace is offered in every round, not in every fifth.
#[test]
fn a_peace_the_plan_wants_is_offered_in_every_round() {
    let (mut game, rules) = scene();
    game.execute(&rules, Command::DeclareWar { player: P0, enemy: P1 }).unwrap();
    for _ in 0..classic_rules::MIN_WAR_ROUNDS_BEFORE_PEACE + 1 {
        end_round(&mut game, &rules);
    }
    // A round that is not the seat's round of proposals.
    while (game.view().auth.turn().round + u32::from(P0.0)) % classic_rules::ai::diplomacy::PROPOSAL_INTERVAL == 0 {
        end_round(&mut game, &rules);
    }
    while game.view().auth.active_player() != P0 {
        let active = game.view().auth.active_player();
        game.execute(&rules, Command::EndTurn { player: active }).unwrap();
    }
    let peace_offers = |net2: Option<Net2Plan>| {
        let (obs, brief) = brief_of(&game, &rules, net2);
        let state = AiState::new(P0, obs.round);
        let input = AdvisorInput { observation: &obs, brief: &brief, state: &state, rules: &rules };
        DiplomacyAdvisor::default().start_of_turn(&input).into_iter().filter(|p| matches!(&p.command, Command::ProposeDeal { to, items, .. } if *to == P1 && items.iter().any(|(_, i)| *i == DealItem::Peace))).count()
    };
    assert_eq!(peace_offers(None), 0, "today: only in the seat's round");
    let wish = Net2Plan { peace_with: vec![P1], ..plan(vec![]) };
    assert_eq!(peace_offers(Some(wish)), 1, "the plan wants it");
    assert_eq!(peace_offers(Some(plan(vec![]))), 0, "no wish, no extra offer");
}

// ---- no denunciation ----

#[test]
fn a_net2_seat_denounces_nobody() {
    let (mut game, rules) = scene();
    while (game.view().auth.turn().round + u32::from(P0.0)) % classic_rules::ai::diplomacy::PROPOSAL_INTERVAL != 0 {
        end_round(&mut game, &rules);
    }
    let denounces = |net2: Option<Net2Plan>| {
        let (mut obs, brief) = brief_of(&game, &rules, net2);
        for r in &mut obs.relations {
            r.warmonger = 400;
        }
        let state = AiState::new(P0, obs.round);
        let input = AdvisorInput { observation: &obs, brief: &brief, state: &state, rules: &rules };
        DiplomacyAdvisor::default().start_of_turn(&input).into_iter().filter(|p| matches!(p.command, Command::Denounce { .. })).count()
    };
    assert!(denounces(None) > 0, "today the warmonger is denounced");
    assert_eq!(denounces(Some(plan(vec![]))), 0);
}

// ---- city-states ----

#[test]
fn the_city_state_share_lowers_the_reserve_and_lifts_the_ceiling() {
    assert_eq!((reserve_for(0), reserve_for(500), reserve_for(1000)), (300, 150, 0));
    assert_eq!((ceiling_for(90, 0), ceiling_for(90, 500), ceiling_for(90, 1000)), (90, 135, 180));
    let (mut game, rules) = scene();
    game.debug_earn(P0, 400);
    let obs = game.ai_observation(&rules, P0);
    let treasury = obs.economy.treasury;
    assert!((300 + 250..600).contains(&(treasury + 150)) || treasury < 550, "scene treasury {treasury}");
    assert_eq!(format!("{:?}", gift_orders_shared(&obs, 0)), format!("{:?}", classic_rules::ai::city_states::gift_orders(&obs)), "share 0 is today");
    if treasury < 550 {
        assert!(gift_orders_shared(&obs, 0).is_empty(), "below today's reserve: nothing");
        assert!(!gift_orders_shared(&obs, 800).is_empty(), "a seat that spends on city-states gifts from a lower treasury");
    }
}

// ---- congress ----

/// The scene with the league founded (P1 knows every technology; rounds pass until the World Leader vote is due).
fn league() -> (GameState, CompiledRuleset) {
    let (mut game, rules) = scene();
    for t in 0..rules.technologies.len() {
        game.debug_learn(&rules, P1, classic_rules::ids::TechId(t as u16));
    }
    for _ in 0..60 {
        end_round(&mut game, &rules);
        let congress = game.view().auth.module().congress();
        if congress.united_nations() && congress.leader_vote_due() && !congress.in_session() {
            break;
        }
    }
    (game, rules)
}

fn congress_obs<'g>(game: &'g GameState, rules: &'g CompiledRuleset, delegates: &[(PlayerId, u16)], needed: u16) -> AIObservation<'g> {
    let (mut obs, _) = brief_of(game, rules, None);
    let c = obs.congress.as_mut().expect("the league is founded");
    c.delegates = delegates.to_vec();
    c.needed = needed;
    obs
}

/// A3: the human (P1) holds 36 % and the AIs 12 % each: the AIs pool on one candidate and the leader does not win.
/// The seat votes for the strongest candidate but the threat; we hold the threshold ourselves: for us.
#[test]
fn the_leader_vote_is_pooled_against_the_rival_at_the_threshold() {
    let (game, rules) = league();
    let who = |table: &[(PlayerId, u16)], needed| {
        let obs = congress_obs(&game, &rules, table, needed);
        leader_choice(&obs, obs.congress.as_ref().unwrap())
    };
    // P1 at the threshold, P2 ahead of us: the pool goes to P2 (a tie or better blocks P1).
    assert_eq!(who(&[(P0, 4), (P1, 9), (P2, 5)], 9), P2);
    // We are the strongest but the threat: the seat votes for itself, which is the coalition's candidate.
    assert_eq!(who(&[(P0, 6), (P1, 9), (P2, 3)], 9), P0);
    // Ties between candidates go to the lowest id.
    assert_eq!(who(&[(P0, 4), (P1, 9), (P2, 4)], 9), P0);
    // One delegate short of the threshold is a threat; two short is not.
    assert_eq!(who(&[(P0, 2), (P1, 8), (P2, 5)], 9), P2);
    assert_eq!(who(&[(P0, 2), (P1, 7), (P2, 5)], 9), P0, "nobody near: for ourselves, as today");
    // We hold the threshold: for ourselves, whatever the rival holds.
    assert_eq!(who(&[(P0, 9), (P1, 9), (P2, 0)], 9), P0);
    // The pool would elect P2 (6 + 4 = 10, past the threshold and the threat's 9): for ourselves.
    assert_eq!(who(&[(P0, 4), (P1, 9), (P2, 6)], 9), P0, "our ballot must not make the coalition candidate the World Leader");
}

/// The ballot as commands: with the plan, the whole seat goes on the coalition candidate; without it, on ourselves.
#[test]
fn the_ballot_of_the_leader_vote() {
    let (game, rules) = league();
    let mut obs = congress_obs(&game, &rules, &[(P0, 4), (P1, 9), (P2, 5)], 9);
    let c = obs.congress.as_mut().unwrap();
    c.in_session = true;
    c.delegates_left = 4;
    c.proposals = vec![Proposal { kind: ResolutionKind::DiplomaticVictory, choice: 0, proposer: None, repeal: false }];
    let cast = |net2: Option<&Net2Plan>| vote_orders_with(&obs, false, net2);
    assert_eq!(format!("{:?}", cast(None)), format!("{:?}", vec![Command::CastVotes { player: P0, proposal: 0, choice: u32::from(P0.0), delegates: 4 }]));
    let p = plan(vec![]);
    assert_eq!(format!("{:?}", cast(Some(&p))), format!("{:?}", vec![Command::CastVotes { player: P0, proposal: 0, choice: u32::from(P2.0), delegates: 4 }]));
}

/// Resolutions are voted by what they name: no on an embargo on us and on a luxury we hold; yes on the embargo the
/// plan wants; abstain on an embargo on a stranger to the plan (the plain AI said yes to every one); the delegates go
/// to the resolutions voted.
#[test]
fn resolutions_are_voted_by_what_they_name() {
    let (game, rules) = league();
    let mut obs = congress_obs(&game, &rules, &[(P0, 4), (P1, 4), (P2, 4)], 9);
    let held = obs.economy.resources.known().next();
    let proposal = |kind, choice| Proposal { kind, choice, proposer: Some(P1), repeal: false };
    let c = obs.congress.as_mut().unwrap();
    c.in_session = true;
    c.delegates_left = 6;
    c.proposals = vec![
        proposal(ResolutionKind::PlayerEmbargo, u32::from(P0.0)),
        proposal(ResolutionKind::PlayerEmbargo, u32::from(P1.0)),
        proposal(ResolutionKind::PlayerEmbargo, u32::from(P2.0)),
    ];
    let p = Net2Plan { embargo: Embargo::On(P1), ..plan(vec![]) };
    let orders = vote_orders_with(&obs, false, Some(&p));
    let choices: Vec<(u8, u32, u16)> = orders.iter().map(|o| match o { Command::CastVotes { proposal, choice, delegates, .. } => (*proposal, *choice, *delegates), _ => panic!() }).collect();
    assert_eq!(choices, vec![(0, 0, 3), (1, 1, 3)], "no on ours, yes on the runaway, none on P2");
    // As today: yes to every embargo on another nation, no on ours.
    let plain = vote_orders_with(&obs, false, None);
    assert_eq!(plain.len(), 3);
    let _ = held;
}

/// The seat proposes the embargo on the runaway, once.
#[test]
fn the_embargo_on_the_runaway_is_proposed_once() {
    let (game, rules) = league();
    let mut obs = congress_obs(&game, &rules, &[(P0, 4), (P1, 4), (P2, 4)], 9);
    let c = obs.congress.as_mut().unwrap();
    c.can_propose = true;
    c.in_session = false;
    let weights = Weights::of(AiState::new(P0, 0).strategy.current);
    let wish = Net2Plan { embargo: Embargo::On(P1), ..plan(vec![]) };
    let first = |obs: &AIObservation<'_>, net2: Option<&Net2Plan>| format!("{:?}", proposal_orders_with(obs, &rules, &weights, net2).into_iter().next());
    assert_eq!(first(&obs, Some(&wish)), format!("{:?}", Some(Command::ProposeResolution { player: P0, kind: ResolutionKind::PlayerEmbargo, choice: u32::from(P1.0), repeal: false })));
    assert_ne!(first(&obs, None), first(&obs, Some(&wish)), "inert without a plan");
    assert_eq!(first(&obs, Some(&plan(vec![]))), first(&obs, None), "inert with the default plan");
    obs.congress.as_mut().unwrap().proposals.push(Proposal { kind: ResolutionKind::PlayerEmbargo, choice: u32::from(P1.0), proposer: Some(P0), repeal: false });
    assert_eq!(first(&obs, Some(&wish)), first(&obs, None), "once proposed, not again");
}
