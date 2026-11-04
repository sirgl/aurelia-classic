use super::*;

const FORM: Rect = Rect { min: pos2(20., 20.), max: pos2(375., 20. + FORM_HEIGHT) };

/// One frame of the world form alone (no scroll area, the layout's own rects).
fn form_frame(ctx: &egui::Context, setup: &mut MatchSetup, events: Vec<egui::Event>) {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400., 760.))),
            events,
            ..Default::default()
        },
        |ui| draw_world_form(ui, FORM, setup),
    );
    output.textures_delta.clear();
}

fn press(at: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn form_click(ctx: &egui::Context, setup: &mut MatchSetup, at: egui::Pos2) {
    form_frame(ctx, setup, vec![egui::Event::PointerMoved(at), press(at, true)]);
    form_frame(ctx, setup, vec![press(at, false)]);
}

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn plus(r: Rect) -> egui::Pos2 {
    r.right_center() - vec2(20., 0.)
}

fn minus(r: Rect) -> egui::Pos2 {
    r.left_center() + vec2(20., 0.)
}

#[test]
fn every_control_edits_the_new_game_request() {
    let ctx = egui::Context::default();
    let mut setup = MatchSetup::default();
    let layout = FormLayout::new(FORM);
    form_frame(&ctx, &mut setup, vec![]);
    form_frame(&ctx, &mut setup, vec![]);
    let pangaea = MapScript::ALL.iter().position(|m| *m == MapScript::Pangaea).unwrap();
    form_click(&ctx, &mut setup, layout.map[pangaea].center());
    form_click(&ctx, &mut setup, layout.size[0].center());
    assert_eq!(setup.size, WorldSize::Duel);
    assert_eq!(setup.ai_nations, 1, "Duel brings one rival");
    form_click(&ctx, &mut setup, plus(layout.ai));
    form_click(&ctx, &mut setup, plus(layout.difficulty));
    form_click(&ctx, &mut setup, layout.water[2].center());
    form_click(&ctx, &mut setup, layout.victories[1].center());
    form_click(&ctx, &mut setup, layout.victories[0].center());
    form_click(&ctx, &mut setup, layout.speed[0].center());
    form_click(&ctx, &mut setup, plus(layout.city_states));
    let new_game = setup.request().unwrap();
    assert_eq!(new_game.options.map, MapScript::Pangaea);
    assert_eq!(new_game.request.size, MapSize::World(WorldSize::Duel));
    assert_eq!(new_game.request.players, 3);
    assert_eq!(new_game.request.water, 40);
    assert_eq!(new_game.options.difficulty, "core:handicap/king");
    assert!(!new_game.options.victories.science, "Science toggled off");
    assert!(new_game.options.victories.domination, "Domination is always on");
    assert_eq!(new_game.options.speed, GameSpeed::Quick, "every speed runs: the first is Quick");
    assert_eq!(new_game.options.city_states, WorldSize::Duel.city_states() + 1, "Duel's 4, one more");
    assert!(new_game.options.barbarians, "barbarians are on");
    form_click(&ctx, &mut setup, layout.seed.center());
    form_frame(&ctx, &mut setup, vec![key(egui::Key::A, egui::Modifiers::COMMAND), egui::Event::Text("77".into())]);
    assert_eq!(setup.request().unwrap().request.seed, 77);
    form_click(&ctx, &mut setup, layout.random.center());
    assert_ne!(setup.request().unwrap().request.seed, 77);
}

#[test]
fn steppers_obey_their_limits() {
    let ctx = egui::Context::default();
    let mut setup = MatchSetup::default();
    let layout = FormLayout::new(FORM);
    form_frame(&ctx, &mut setup, vec![]);
    for _ in 0..20 {
        form_click(&ctx, &mut setup, plus(layout.ai));
    }
    assert_eq!(setup.ai_nations, MAX_NATIONS - 1);
    for _ in 0..20 {
        form_click(&ctx, &mut setup, minus(layout.ai));
    }
    assert_eq!(setup.ai_nations, 1);
    for _ in 0..10 {
        form_click(&ctx, &mut setup, minus(layout.difficulty));
    }
    assert_eq!(setup.difficulty, "core:handicap/settler");
    for _ in 0..10 {
        form_click(&ctx, &mut setup, plus(layout.difficulty));
    }
    assert_eq!(setup.difficulty, "core:handicap/deity");
}

#[test]
fn keyboard_focus_activates_choices() {
    let ctx = egui::Context::default();
    let mut setup = MatchSetup::default();
    form_frame(&ctx, &mut setup, vec![]);
    form_frame(&ctx, &mut setup, vec![key(egui::Key::Tab, egui::Modifiers::NONE)]);
    form_frame(&ctx, &mut setup, vec![key(egui::Key::Tab, egui::Modifiers::NONE)]);
    form_frame(&ctx, &mut setup, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    assert_eq!(setup.map, MapScript::ALL[1], "Tab, Tab and Enter select the second map type");
    form_frame(&ctx, &mut setup, vec![key(egui::Key::Tab, egui::Modifiers::NONE)]);
    form_frame(&ctx, &mut setup, vec![key(egui::Key::Space, egui::Modifiers::NONE)]);
    assert_eq!(setup.map, MapScript::ALL[2]);
}

#[test]
fn start_creates_once_and_shows_the_loading_screen() {
    let ctx = egui::Context::default();
    let mut state = ScreenState::default();
    state.screen = Screen::MatchSetup;
    let frame = |state: &mut ScreenState, events| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1600., 900.))),
                events,
                ..Default::default()
            },
            |ui| {
                crate::ui::screens::draw(ui.ctx(), state, &RenderWorld::default(), None);
            },
        );
        output.textures_delta.clear();
        output
    };
    frame(&mut state, vec![]);
    frame(&mut state, vec![]);
    let settings_width = (1552. * 0.3_f32).clamp(290., 380.);
    let panel = Rect::from_min_max(pos2(24. + 1552. - settings_width, 80.), pos2(1576., 863.));
    let start = create_button_rect(panel).center();
    frame(&mut state, vec![egui::Event::PointerMoved(start), press(start, true)]);
    frame(&mut state, vec![press(start, false)]);
    let new_game = state.take_match_request().expect("Start asks for one world");
    assert!(state.take_match_request().is_none());
    assert_eq!(new_game.options.map, MapScript::Continents);
    assert!(state.match_setup.loading);
    let output = frame(&mut state, vec![]);
    let texts: Vec<String> = output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
            _ => None,
        })
        .collect();
    assert!(texts.iter().any(|t| t.starts_with("Creating the world")), "{texts:?}");
    assert!(!texts.iter().any(|t| t == "Start game"), "the form is gone while loading");
}

#[test]
fn every_map_type_and_small_size_starts_a_valid_deterministic_game() {
    let rules = rules();
    for map in MapScript::ALL {
        for size in [WorldSize::Duel, WorldSize::Tiny, WorldSize::Small] {
            let new_game = NewGame {
                request: MatchRequest {
                    nation: "core:nation/england".into(),
                    seed: 12345,
                    size: MapSize::World(size),
                    players: size.nations(),
                    water: 25,
                },
                options: GameOptions { map, difficulty: "core:handicap/emperor".into(), ..GameOptions::default() },
            };
            let a = prepare_game(new_game.clone()).unwrap_or_else(|e| panic!("{map:?} {size:?}: {e}"));
            let b = prepare_game(new_game).unwrap();
            assert_eq!((a.world.width, a.world.height), size.dimensions());
            assert_eq!(a.game.view().auth.players().count(), size.nations() as usize);
            let nation = a.game.view().auth.players().nation_of(PlayerId(0));
            assert_eq!(rules.nations[nation as usize].key, "core:nation/england");
            assert_eq!(a.world.terrain, b.world.terrain, "{map:?} {size:?} is deterministic");
            assert_eq!(a.options.difficulty, "core:handicap/emperor");
            let settlers = a.game.view().auth.indexes().units_of(PlayerId(0)).len();
            assert!(settlers > 0, "{map:?} {size:?}: the player starts with units");
        }
    }
}

#[test]
fn unknown_or_unplayable_choices_are_refused() {
    let base = NewGame::from(MatchRequest {
        nation: "core:nation/rome".into(),
        seed: 1,
        size: MapSize::World(WorldSize::Duel),
        players: 2,
        water: 25,
    });
    let mut bad = base.clone();
    bad.request.nation = "core:nation/atlantis".into();
    assert!(prepare_game(bad).is_err());
    let mut bad = base.clone();
    bad.options.difficulty = "core:handicap/impossible".into();
    assert!(prepare_game(bad).is_err());
    // Every speed of the rules plays, its last round the time victory's.
    let mut marathon = base.clone();
    marathon.options.speed = GameSpeed::Marathon;
    let prepared = prepare_game(marathon).expect("a Marathon game");
    assert_eq!(prepared.game.view().auth.module().victory().options().max_rounds, 1500);
    assert_eq!(prepared.game.view().auth.settings().speed, "core:speed/marathon");
    let mut bad = base.clone();
    bad.request.players = MAX_NATIONS + 1;
    assert!(prepare_game(bad).is_err());
    let mut bad = base;
    bad.request.water = 33;
    assert!(prepare_game(bad).is_err());
}

#[test]
fn legacy_request_keeps_the_fractal_world_and_masks_unknown_tiles() {
    let request = MatchRequest {
        nation: "core:nation/england".into(),
        seed: 12345,
        size: MapSize::Small,
        players: 3,
        water: 25,
    };
    let a = prepare(request).unwrap();
    assert_eq!((a.world.width, a.world.height), (24, 20));
    assert_eq!(a.options, GameOptions::default());
    for cell in &a.world.terrain {
        let i = cell.tile.0 as usize;
        if a.world.explored[i] == 0 && a.world.visible[i] == 0 {
            assert_eq!(cell.terrain, 1);
            assert!(cell.resource.is_none() && cell.owner.is_none());
        }
    }
    assert!(a.world.units.iter().all(|u| a.world.visible[u.tile.0 as usize] != 0));
}

#[test]
fn rules_seed_and_start_remain_clickable_on_narrow_management_canvas() {
    for (width, height) in [(1440_f32, 900_f32), (960., 640.)] {
        let ctx = egui::Context::default();
        let mut state = ScreenState::default();
        state.screen = Screen::MatchSetup;
        // `screens::draw` builds a new shell state every frame, so each frame looks like opening the screen.
        state.capture_setup_seed();
        let frame = |state: &mut ScreenState, events| {
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(width, height))), events, ..Default::default() }, |ui| {
                super::super::draw(ui.ctx(), state, &RenderWorld::default(), None);
            });
            output.textures_delta.clear();
        };
        let body = vec2(width - 48., height - 117.);
        let scale = (body.x / 1150.).min(body.y / 640.).min(1.);
        let canvas = body / scale;
        let settings_width = (canvas.x * 0.3).clamp(290., 380.);
        let panel_x = canvas.x - settings_width;
        let point = |x, y| pos2(24., 80.) + vec2(x, y) * scale;
        let click = |state: &mut ScreenState, at| {
            frame(state, vec![egui::Event::PointerMoved(at), press(at, true)]);
            frame(state, vec![press(at, false)]);
        };
        frame(&mut state, vec![]); frame(&mut state, vec![]);
        click(&mut state, point(panel_x + settings_width * 0.75, 31.));
        assert!(state.match_setup.rules_tab);
        click(&mut state, point(panel_x + 80., 361.));
        frame(&mut state, vec![key(egui::Key::A, egui::Modifiers::COMMAND), egui::Event::Text("77".into())]);
        assert_eq!(state.match_setup.request().unwrap().request.seed, 77);
        click(&mut state, point(panel_x + settings_width * 0.5, canvas.y - 38.5));
        let game = state.take_match_request().expect("Start works without scrolling the form");
        assert_eq!(game.request.seed, 77);
        assert!(state.match_setup.loading);
        assert!(state.take_match_request().is_none());
    }
}

/// Every text the flat world form paints in one frame.
fn form_texts(ctx: &egui::Context, setup: &mut MatchSetup) -> Vec<String> {
    form_frame(ctx, setup, vec![]);
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400., 760.))),
            ..Default::default()
        },
        |ui| draw_world_form(ui, FORM, setup),
    );
    output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
            _ => None,
        })
        .collect()
}

#[test]
fn the_form_speaks_english_by_default() {
    let texts = form_texts(&egui::Context::default(), &mut MatchSetup::default());
    for want in ["MAP TYPE", "Pangaea", "Small Cont.", "Duel", "Standard", "Marathon", "Medium", "VICTORY CONDITIONS", "Randomize", "Prince"] {
        assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
    }
}

#[test]
fn the_form_speaks_russian_when_asked() {
    let texts = i18n::with_language("ru", || form_texts(&egui::Context::default(), &mut MatchSetup::default()));
    for want in [
        "\u{422}\u{418}\u{41f} \u{41a}\u{410}\u{420}\u{422}\u{42b}", // Russian: "MAP TYPE"
        "\u{41f}\u{430}\u{43d}\u{433}\u{435}\u{44f}", // Russian: "Pangaea"
        "\u{414}\u{443}\u{44d}\u{43b}\u{44c}", // Russian: "Duel"
        "\u{41a}\u{440}\u{43e}\u{448}\u{435}\u{447}\u{43d}\u{430}\u{44f}", // Russian: "Tiny"
        "\u{41c}\u{430}\u{440}\u{430}\u{444}\u{43e}\u{43d}", // Russian: "Marathon"
        "\u{421}\u{440}\u{435}\u{434}\u{43d}\u{438}\u{439}", // Russian: "Medium"
        "\u{423}\u{421}\u{41b}\u{41e}\u{412}\u{418}\u{42f} \u{41f}\u{41e}\u{411}\u{415}\u{414}\u{42b}", // Russian: "VICTORY CONDITIONS"
        "\u{413}\u{43e}\u{441}\u{43f}\u{43e}\u{434}\u{441}\u{442}\u{432}\u{43e}", // Russian: "Domination"
        "\u{421}\u{43b}\u{443}\u{447}\u{430}\u{439}\u{43d}\u{43e}\u{435}", // Russian: "Random"
        "\u{41f}\u{440}\u{438}\u{43d}\u{446}", // Russian: "Prince"
        "\u{417}\u{415}\u{420}\u{41d}\u{41e} \u{41c}\u{418}\u{420}\u{410}", // Russian: "WORLD SEED"
    ] {
        assert!(texts.iter().any(|t| t == want), "{want}: {texts:?}");
    }
    assert!(!texts.iter().any(|t| t == "MAP TYPE" || t == "Randomize"), "{texts:?}");
}

#[test]
fn errors_and_the_summary_follow_the_language() {
    let mut setup = MatchSetup::default();
    setup.seed = "not a number".into();
    assert_eq!(
        setup.request().unwrap_err(),
        "Enter a whole-number seed from 0 to 18446744073709551615"
    );
    let ru = i18n::with_language("ru", || setup.request().unwrap_err());
    assert!(ru.starts_with("\u{412}\u{432}\u{435}\u{434}\u{438}\u{442}\u{435} \u{446}\u{435}\u{43b}\u{43e}\u{435} \u{447}\u{438}\u{441}\u{43b}\u{43e}"), "{ru}"); // Russian: "Enter a whole number..."
    let summary = |players: u32| {
        tf!(
            "classic.ui.match-setup.summary",
            map = "Pangaea",
            size = "Small",
            width = 66,
            height = 42,
            players = players,
            difficulty = "Prince"
        )
    };
    assert_eq!(summary(1), "Pangaea · Small (66 × 42) · 1 nation · Prince");
    assert_eq!(summary(6), "Pangaea · Small (66 × 42) · 6 nations · Prince");
    for (n, word) in [(1, "\u{43d}\u{430}\u{446}\u{438}\u{44f}"), (2, "\u{43d}\u{430}\u{446}\u{438}\u{438}"), (5, "\u{43d}\u{430}\u{446}\u{438}\u{439}"), (11, "\u{43d}\u{430}\u{446}\u{438}\u{439}"), (21, "\u{43d}\u{430}\u{446}\u{438}\u{44f}"), (24, "\u{43d}\u{430}\u{446}\u{438}\u{438}")] { // Russian plural forms of "nation": 1 -> nominative singular, 2 -> genitive singular, 5 -> genitive plural
        let ru = i18n::with_language("ru", || summary(n));
        assert!(ru.contains(&format!("{n} {word} ·")), "{n}: {ru}");
    }
}

#[test]
fn a_bad_world_is_refused_in_the_active_language() {
    let mut request = MatchRequest {
        nation: "core:nation/rome".into(),
        seed: 1,
        size: MapSize::World(WorldSize::Duel),
        players: 2,
        water: 33,
    };
    assert_eq!(prepare(request.clone()).err().unwrap(), "Invalid world settings");
    let ru = i18n::with_language("ru", || prepare(request.clone()).err().unwrap());
    assert_eq!(ru, "\u{41d}\u{435}\u{43a}\u{43e}\u{440}\u{440}\u{435}\u{43a}\u{442}\u{43d}\u{44b}\u{435} \u{43d}\u{430}\u{441}\u{442}\u{440}\u{43e}\u{439}\u{43a}\u{438} \u{43c}\u{438}\u{440}\u{430}"); // Russian: "Invalid world settings"
    request.nation = "core:nation/atlantis".into();
    assert_eq!(prepare(request).err().unwrap(), "Choose an available nation");
}

#[test]
fn fresh_seeds_differ_from_call_to_call() {
    let seeds: std::collections::HashSet<u64> = (0..1000).map(|_| fresh_seed()).collect();
    assert_eq!(seeds.len(), 1000);
}

#[test]
fn default_seed_stays_deterministic_and_randomize_changes_it() {
    let mut setup = MatchSetup::default();
    assert_eq!(setup.seed, "20260928");
    assert_eq!(MatchSetup::default().seed, setup.seed);
    setup.randomize_seed();
    assert_ne!(setup.seed, "20260928");
    assert!(setup.seed.parse::<u64>().is_ok(), "the seed stays a u64 in digits: {}", setup.seed);
    assert_eq!(setup.request().unwrap().request.seed.to_string(), setup.seed);
}

#[test]
fn opening_the_setup_draws_a_new_seed_each_time() {
    let mut state = ScreenState::default();
    let mut seen = std::collections::HashSet::new();
    for _ in 0..5 {
        state.opened(Screen::MatchSetup, Screen::Menu);
        assert!(seen.insert(state.match_setup.seed.clone()), "a seed repeated: {}", state.match_setup.seed);
    }
    assert!(!seen.contains("20260928"));
}

#[test]
fn a_typed_seed_survives_a_visit_to_the_reference_and_captures_keep_theirs() {
    let mut state = ScreenState::default();
    state.match_setup.seed = "77".into();
    state.opened(Screen::MatchSetup, Screen::Encyclopedia);
    assert_eq!(state.match_setup.seed, "77");
    state.opened(Screen::Map, Screen::MatchSetup);
    assert_eq!(state.match_setup.seed, "77", "other screens do not touch the seed");
    state.capture_setup_seed();
    state.opened(Screen::MatchSetup, Screen::Menu);
    assert_eq!(state.match_setup.seed, "77");
}

#[test]
fn the_phone_form_has_finger_sized_rows_inside_its_width() {
    for rules_tab in [false, true] {
        let origin = Rect::from_min_size(pos2(10., 20.), vec2(450., 0.));
        let (layout, titles, height) = FormLayout::phone(origin, rules_tab);
        let cells: Vec<Rect> = if rules_tab {
            [&layout.speed, &layout.water, &layout.victories].into_iter().flatten().copied().chain([layout.seed, layout.random]).collect()
        } else {
            [&layout.map, &layout.size].into_iter().flatten().copied().chain([layout.difficulty, layout.ai, layout.city_states]).collect()
        };
        assert!(!titles.is_empty());
        for (i, cell) in cells.iter().enumerate() {
            assert!(cell.height() >= 44., "{cell:?} is below a finger's size");
            assert!(cell.left() >= origin.left() - 0.01 && cell.right() <= origin.right() + 0.01, "{cell:?} leaves the form");
            assert!(cell.top() >= origin.top() && cell.bottom() <= origin.top() + height + 0.01, "{cell:?} leaves the form's height");
            for other in &cells[i + 1..] {
                assert!(!cell.shrink(0.5).intersects(*other), "{cell:?} overlaps {other:?}");
            }
        }
        assert_eq!(layout.look, Look::PHONE);
        assert!(Look::PHONE.label >= 16. && Look::PHONE.value >= 16.);
    }
}
