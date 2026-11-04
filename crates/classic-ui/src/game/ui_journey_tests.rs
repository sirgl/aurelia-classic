//! One cross-screen journey through the same typed requests and save slot as the UI.
use super::*;
use client::shell::saves::SaveSlot;
use super::save_files::CLASSIC;
use crate::ui::model::DealDraft;
use classic_rules::ids::TechId;
use classic_rules::world::relations::DealItem;

/// Isolated from AURELIA_SAVE_DIR and all user saves, including on assertion failure.
struct JourneyDirectory(std::path::PathBuf);
impl JourneyDirectory {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("aurelia-ui-journey-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&path).expect("unique isolated test directory");
        Self(path)
    }
}
impl Drop for JourneyDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn cross_screen_journey_keeps_research_city_purchase_and_deal_through_disk_save() {
    let rules = game_rules();
    let prepared = prepare_match(MatchRequest {
        nation: "core:nation/rome".into(),
        seed: 20260928,
        size: MapSize::Small,
        players: 3,
        water: 25,
    })
    .expect("new match");
    // Economy fixture only: every action below still goes through UI request validation.
    let mut fixture = classic_rules::persistence::encode::encode(&prepared.game, &rules);
    fixture.world.player_gold[0] = 5000;
    let mut session = Session::new(
        classic_rules::persistence::decode::decode(&fixture, &rules).unwrap(),
        rules.clone(),
    );
    found_capital(&mut session);
    let city_tile = session.city_view().unwrap().tile;

    let target = session
        .research_view()
        .techs
        .into_iter()
        .filter(|card| card.availability.is_ready())
        .max_by_key(|card| card.plan.len())
        .expect("distant research target");
    assert!(
        target.plan.len() > 16,
        "exercise the complete prerequisite plan, not an adjacent choice"
    );
    let expected_queue: Vec<TechId> = target.plan.iter().map(|id| TechId(*id as u16)).collect();
    let science = session
        .game()
        .view()
        .auth
        .players()
        .science_of(session.me());
    session.request(GameRequest::Research(target.id));
    assert_eq!(
        session
            .game()
            .view()
            .auth
            .players()
            .research_queue_of(session.me()),
        expected_queue
    );
    assert_eq!(expected_queue.last(), Some(&target.id));
    assert_eq!(
        session
            .game()
            .view()
            .auth
            .players()
            .science_of(session.me()),
        science
    );

    let building = session
        .city_view()
        .unwrap()
        .options
        .into_iter()
        .find(|option| {
            matches!(option.item, ProductionItem::Building(_)) && option.availability.is_ready()
        })
        .expect("available city building");
    session.request(GameRequest::Enqueue(building.item));
    assert_eq!(
        session.city_view().unwrap().queue.last().unwrap().name,
        building.name
    );
    let before = session.city_view().unwrap();
    let offer = before
        .offers
        .iter()
        .find(|offer| offer.availability.is_ready())
        .expect("purchasable land")
        .clone();
    session.request(GameRequest::BuyTile(offer.tile));
    let after = session.city_view().unwrap();
    assert_eq!(after.treasury, before.treasury - offer.price);
    assert!(after
        .offers
        .iter()
        .all(|candidate| candidate.tile != offer.tile));
    assert_eq!(
        session.game().view().auth.map().owner(offer.tile).flatten(),
        Some(session.me())
    );

    // Meeting is the only diplomatic fixture; the offer itself is a real UI action.
    let partner = PlayerId(1);
    let me = session.me();
    session.harness_game().0.debug_meet(me, partner);
    session.refresh_after_harness();
    assert!(session
        .diplomacy_view()
        .nations
        .iter()
        .any(|c| c.player == partner));
    let draft = DealDraft {
        partner,
        items: vec![(session.me(), DealItem::GoldPerTurn(1))],
        amounts: vec![],
    };
    let before_preview = session.world_hash();
    assert_eq!(session.preview_deal(&draft), Availability::Ready);
    assert_eq!(
        session.world_hash(),
        before_preview,
        "preview must not create or pay an offer"
    );
    session.request(GameRequest::ProposeDeal {
        to: partner,
        items: draft.items.clone(),
    });
    let pending = session
        .game()
        .view()
        .auth
        .module()
        .relations()
        .offers()
        .to_vec();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].items, draft.items);
    assert_eq!(session.diplomacy_view().outgoing[0].items, draft.items);

    let before_save = session.city_view().unwrap();
    let original_hash = session.world_hash();
    let directory = JourneyDirectory::new();
    let slot = SaveSlot::at(directory.0.join("journey.aurelia"), CLASSIC);
    assert!(!slot.exists());
    slot.write(&session.save_bytes())
        .expect("save the real session to an isolated disk slot");
    let mut loaded = Session::from_save(&slot.read().unwrap(), rules).expect("load that session");
    assert_eq!(loaded.world_hash(), original_hash);
    loaded.request(GameRequest::SelectCity(city_tile));
    assert_eq!(loaded.city_view().unwrap(), before_save);
    assert_eq!(
        loaded
            .game()
            .view()
            .auth
            .players()
            .research_queue_of(loaded.me()),
        expected_queue
    );
    assert_eq!(
        loaded.game().view().auth.module().relations().offers(),
        pending
    );
    assert_eq!(
        loaded.diplomacy_view().outgoing,
        session.diplomacy_view().outgoing
    );

    let round = session.turn();
    for game in [&mut session, &mut loaded] {
        game.request(GameRequest::EndTurn);
        game.game()
            .validate(game.rules())
            .expect("valid continued world");
        assert_eq!(game.turn(), round + 1);
        game.request(GameRequest::SelectCity(city_tile));
    }
    assert_eq!(
        loaded.world_hash(),
        session.world_hash(),
        "AI response, research and city progress continue identically"
    );
    assert_eq!(loaded.city_view(), session.city_view());
    assert_eq!(
        loaded.game().view().auth.module().relations(),
        session.game().view().auth.module().relations()
    );
    assert!(
        !slot.path().with_extension("tmp").exists(),
        "atomic save leaves no temporary file"
    );
}
