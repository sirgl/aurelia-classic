//! Tests of the kernel's research planner and research cost on classic games.

mod tests {
    use crate::systems::research::{plan_research, ResearchPlanError};
    use crate::ids::TechId;
    use crate::rules::CompiledRuleset;
    use crate::ids::{PlayerId, TileId};
    use crate::persistence::{decode::decode, encode::encode};
    use crate::replay::authoritative_hash;
    use crate::{Command, GameConfig, GameState};

    fn graph(deps: &[Vec<usize>]) -> CompiledRuleset {
        let nodes = deps
            .iter()
            .enumerate()
            .map(|(i, refs)| {
                let refs = refs
                    .iter()
                    .map(|r| format!("\"test:tech/t{r:03}\""))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("(id:\"test:tech/t{i:03}\",cost:2,requires:[{refs}])")
            })
            .collect::<Vec<_>>()
            .join(",");
        crate::fixtures::compile_ron(&format!("(technologies:[{nodes}])"))
    }
    fn chain(n: usize) -> CompiledRuleset {
        graph(
            &(0..n)
                .map(|i| if i == 0 { vec![] } else { vec![i - 1] })
                .collect::<Vec<_>>(),
        )
    }
    fn game(rules: &CompiledRuleset) -> GameState {
        let mut game = GameState::new(
            rules,
            GameConfig {
                players: 2,
                width: 16,
                height: 16,
                rng_seed: 321,
            },
        );
        game.debug_found_city(rules, PlayerId(0), TileId(85));
        game.debug_found_city(rules, PlayerId(1), TileId(204));
        game
    }
    fn plan(target: u16) -> Command {
        Command::PlanResearch {
            player: PlayerId(0),
            target: TechId(target),
        }
    }

    #[test]
    fn deterministic_diamond_shared_dependencies_once_known_skipped() {
        let rules = graph(&[vec![], vec![0], vec![0], vec![2, 1]]);
        assert_eq!(
            plan_research(&rules, &[], TechId(3)).unwrap(),
            [TechId(0), TechId(2), TechId(1), TechId(3)]
        );
        assert_eq!(
            plan_research(&rules, &[TechId(0)], TechId(3)).unwrap(),
            [TechId(2), TechId(1), TechId(3)]
        );
        assert_eq!(
            plan_research(&rules, &[TechId(0), TechId(2)], TechId(3)).unwrap(),
            [TechId(1), TechId(3)]
        );
        assert_eq!(
            plan_research(&rules, &[TechId(3)], TechId(3)),
            Err(ResearchPlanError::AlreadyResearched(TechId(3)))
        );
    }
    #[test]
    fn rejects_unknown_ids_and_cycles() {
        let mut rules = chain(3);
        assert_eq!(
            plan_research(&rules, &[], TechId(99)),
            Err(ResearchPlanError::UnknownTechnology(TechId(99)))
        );
        rules.technologies[0].requires.push(TechId(2));
        assert!(matches!(
            plan_research(&rules, &[], TechId(2)),
            Err(ResearchPlanError::DependencyCycle(_))
        ));
        rules.technologies[0].requires = vec![TechId(99)];
        assert_eq!(
            plan_research(&rules, &[], TechId(2)),
            Err(ResearchPlanError::UnknownTechnology(TechId(99)))
        );
    }
    #[test]
    fn full_plan_save_load_replay_and_end_turn_completion() {
        let rules = chain(81);
        let mut initial = encode(&game(&rules), &rules);
        initial.world.player_science[0] = 1000;
        let mut live = decode(&initial, &rules).unwrap();
        let mut replayed = decode(&initial, &rules).unwrap();
        let commands = [
            plan(80),
            Command::EndTurn {
                player: PlayerId(0),
            },
        ];
        for (i, command) in commands.into_iter().enumerate() {
            let before = authoritative_hash(&live);
            live.execute(&rules, command.clone()).unwrap();
            replayed.execute(&rules, command).unwrap();
            assert_ne!(before, authoritative_hash(&live));
            assert_eq!(authoritative_hash(&live), authoritative_hash(&replayed));
            if i == 0 {
                assert_eq!(
                    live.view().auth.players().research_queue_of(PlayerId(0)),
                    &(0..81).map(TechId).collect::<Vec<_>>()
                );
                let restored = decode(&encode(&live, &rules), &rules).unwrap();
                assert_eq!(authoritative_hash(&live), authoritative_hash(&restored));
            }
        }
        assert_eq!(
            live.view().auth.players().techs_of(PlayerId(0)),
            &(0..81).map(TechId).collect::<Vec<_>>()
        );
        assert!(live
            .view()
            .auth
            .players()
            .research_queue_of(PlayerId(0))
            .is_empty());
    }
    #[test]
    fn replacing_target_is_atomic_science_preserved_and_repeat_idempotent() {
        let rules = chain(20);
        let mut game = game(&rules);
        crate::fixtures::play_round(&mut game, &rules);
        let science = game.view().auth.players().science_of(PlayerId(0));
        assert!(science > 0);
        game.execute(&rules, plan(19)).unwrap();
        game.execute(&rules, plan(2)).unwrap();
        assert_eq!(
            game.view().auth.players().research_queue_of(PlayerId(0)),
            [TechId(0), TechId(1), TechId(2)]
        );
        assert_eq!(game.view().auth.players().science_of(PlayerId(0)), science);
        let before = authoritative_hash(&game);
        game.execute(&rules, plan(2)).unwrap();
        assert_eq!(before, authoritative_hash(&game));
    }
    #[test]
    fn invalid_and_inactive_player_plans_have_no_side_effects() {
        let mut rules = chain(3);
        let mut game = game(&rules);
        game.execute(&rules, plan(1)).unwrap();
        let before = authoritative_hash(&game);
        for command in [
            plan(99),
            Command::PlanResearch {
                player: PlayerId(99),
                target: TechId(2),
            },
            Command::PlanResearch {
                player: PlayerId(1),
                target: TechId(2),
            },
        ] {
            assert!(game.execute(&rules, command).is_err());
            assert_eq!(before, authoritative_hash(&game));
        }
        rules.technologies[0].requires.push(TechId(2));
        assert!(game.execute(&rules, plan(2)).is_err());
        assert_eq!(before, authoritative_hash(&game));
        rules.technologies[0].requires.clear();
        let empty = GameState::new(
            &rules,
            GameConfig {
                players: 2,
                width: 16,
                height: 16,
                rng_seed: 321,
            },
        );
        let mut save = encode(&empty, &rules);
        save.world.players_alive[0] = false;
        let mut dead = decode(&save, &rules).unwrap();
        let before = authoritative_hash(&dead);
        assert!(dead.execute(&rules, plan(2)).is_err());
        assert_eq!(before, authoritative_hash(&dead));
    }
    #[test]
    fn free_grant_can_learn_queued_prerequisite_without_spending_science() {
        let rules = chain(3);
        let mut save = encode(&game(&rules), &rules);
        save.world.player_science[0] = 100;
        save.world.economy.players[0]
            .free_technologies
            .push(crate::persistence::economy::FREE_TECH_ANY);
        let mut game = decode(&save, &rules).unwrap();
        game.execute(&rules, plan(2)).unwrap();
        assert_eq!(
            crate::derived::grants::owed(game.view().auth, PlayerId(0))
                .technologies
                .len(),
            1
        );
        game.execute(
            &rules,
            Command::ChooseFreeTechnology {
                player: PlayerId(0),
                tech: TechId(0),
            },
        )
        .unwrap();
        assert_eq!(game.view().auth.players().science_of(PlayerId(0)), 100);
        assert!(crate::derived::grants::owed(game.view().auth, PlayerId(0))
            .technologies
            .is_empty());
        let before = authoritative_hash(&game);
        assert!(game.execute(&rules, plan(0)).is_err());
        assert_eq!(before, authoritative_hash(&game));
        game.execute(
            &rules,
            Command::EndTurn {
                player: PlayerId(0),
            },
        )
        .unwrap();
        assert_eq!(
            game.view().auth.players().techs_of(PlayerId(0)),
            [TechId(0), TechId(1), TechId(2)]
        );
        assert!(game
            .view()
            .auth
            .players()
            .research_queue_of(PlayerId(0))
            .is_empty());
        // Two paid technologies of cost 2 are 3 each with the city-count
        // penalty (rules.md): 100 − 6.
        assert!(game.view().auth.players().science_of(PlayerId(0)) >= 94);
    }
    #[test]
    fn malformed_restored_order_cannot_bypass_prerequisites() {
        let rules = chain(3);
        let mut save = encode(&game(&rules), &rules);
        save.world.player_research[0] = vec![2, 0, 1];
        save.world.player_science[0] = 100;
        let mut game = decode(&save, &rules).unwrap();
        crate::fixtures::play_round(&mut game, &rules);
        assert!(game.view().auth.players().techs_of(PlayerId(0)).is_empty());
        assert!(game.view().auth.players().science_of(PlayerId(0)) >= 100);
        game.execute(&rules, plan(2)).unwrap();
        crate::fixtures::play_round(&mut game, &rules);
        assert_eq!(
            game.view().auth.players().techs_of(PlayerId(0)),
            [TechId(0), TechId(1), TechId(2)]
        );
    }
}

mod cost_tests {
    use crate::ids::{BuildingKindId, PlayerId, TechId, TileId};
    use crate::{Command, GameConfig, GameState};
    #[test]
    fn public_cost_matches_actual_modified_research_spending() {
        let rules = crate::fixtures::compile_ron(
            r#"(
            technologies:[(id:"test:tech/target",cost:100)],
            buildings:[(id:"test:building/academy",cost:1,effects:[
                (scope:Player,selector:Owner,modifier:ModifyCost(target:Technologies,percent:-25))
            ])]
        )"#,
        );
        let mut game = GameState::new(
            &rules,
            GameConfig {
                players: 2,
                width: 16,
                height: 16,
                rng_seed: 123,
            },
        );
        let city = game.debug_found_city(&rules, PlayerId(0), TileId(85));
        game.debug_found_city(&rules, PlayerId(1), TileId(204));
        game.debug_add_building(&rules, city, BuildingKindId(0));
        // 100 × (1 + 0 × 5 %) (one city counts nothing, Unciv: TechManager.costOfTech) = 100, then −25 % = 75.
        assert_eq!(
            crate::technology_cost(game.view().auth, &rules, PlayerId(0), TechId(0)),
            75
        );
        let mut save = crate::persistence::encode::encode(&game, &rules);
        save.world.player_science[0] = 100;
        let mut game = crate::persistence::decode::decode(&save, &rules).unwrap();
        let mut idle = crate::persistence::decode::decode(&save, &rules).unwrap();
        game.execute(
            &rules,
            Command::PlanResearch {
                player: PlayerId(0),
                target: TechId(0),
            },
        )
        .unwrap();
        let end = Command::EndTurn {
            player: PlayerId(0),
        };
        game.execute(&rules, end.clone()).unwrap();
        idle.execute(&rules, end).unwrap();
        assert!(game
            .view()
            .auth
            .players()
            .techs_of(PlayerId(0))
            .contains(&TechId(0)));
        assert_eq!(
            idle.view().auth.players().science_of(PlayerId(0))
                - game.view().auth.players().science_of(PlayerId(0)),
            75
        );
    }
}
