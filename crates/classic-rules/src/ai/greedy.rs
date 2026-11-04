//! The pre-M6 greedy planner, kept only as a fixed command script of the
//! pinned regression tests (baseline_hashes, classic_game, save_continuation,
//! corrupt_saves): their hashes pin the rules, not the AI. The game's AI is
//! `ai::driver` with `ai::planner` (M6).
//!
//! Defects that made it unfit as the game AI (R-07 AI review, all fixed in
//! the M6 planner): no memory between rounds (a new planner every round),
//! the first unit founds the capital whatever its kind, no scouting,
//! settling, production or workers, one march step per unit and turn,
//! O(own × visible) target scans, war on any contact and peace on any lost
//! contact.

use crate::command::Command;
use crate::ids::{PlayerId, TileId};
use crate::knowledge::observation::{AIObservation, ObservedUnit, OwnUnit};
use crate::rules::CompiledRuleset;

pub trait Planner {
    fn plan(&mut self, observation: AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Command>;
}

/// A v0 stub: does nothing.
pub struct NullPlanner;

impl Planner for NullPlanner {
    fn plan(&mut self, _observation: AIObservation<'_>, _rules: &CompiledRuleset) -> Vec<Command> {
        vec![]
    }
}

/// Demo planner: war on contact, a step towards a visible enemy, an attack nearby,
/// a city when the empire is empty. ALL from the observation: hidden units are
/// invisible to it (the test ai_ignores_hidden below proves it at the behaviour level).
pub struct GreedyPlanner;

impl GreedyPlanner {
    /// Hex distance on the public map size (the map topology of crate::math).
    fn distance(obs: &AIObservation<'_>, a: TileId, b: TileId) -> u32 {
        let dims = crate::math::MapDimensions {
            width: obs.map_width,
            height: obs.map_height,
        };
        dims.distance(a, b).unwrap_or(i32::MAX) as u32
    }

    fn nearest_enemy(obs: &AIObservation<'_>, own: &OwnUnit) -> Option<ObservedUnit> {
        obs.visible_foreign_units
            .iter()
            .copied()
            .min_by_key(|f| Self::distance(obs, own.tile, f.tile))
    }
}

impl Planner for GreedyPlanner {
    fn plan(&mut self, obs: AIObservation<'_>, rules: &CompiledRuleset) -> Vec<Command> {
        let mut out = Vec::new();
        // Research: with an empty queue, the cheapest available tech
        // (prerequisites respected, duplicates excluded, as validate does).
        if obs.economy.research_queue.is_empty() {
            let mut best: Option<(u32, crate::ids::TechId)> = None;
            for (i, def) in rules.technologies.iter().enumerate() {
                let id = crate::ids::TechId(i as u16);
                if obs.economy.techs.contains(&id) {
                    continue;
                }
                if !def.requires.iter().all(|r| obs.economy.techs.contains(r)) {
                    continue;
                }
                if best.map(|(c, _)| def.cost < c).unwrap_or(true) {
                    best = Some((def.cost, id));
                }
            }
            if let Some((_, tech)) = best {
                out.push(Command::ChooseTechnology {
                    player: obs.player,
                    technology: tech,
                });
            }
        }
        // Expansion by the first unit (it drops out; the rest fight on).
        let mut settler_used = None;
        if obs.own_cities.is_empty() {
            if let Some(u) = obs.own_units.first() {
                out.push(Command::FoundCity { settler: u.handle });
                settler_used = Some(u.handle);
            }
        }
        for u in &obs.own_units {
            if Some(u.handle) == settler_used {
                continue;
            }
            // Siege: an adjacent foreign visible city is hit before units
            // (the city will not run away; units come after).
            let mut sieged = false;
            for c in obs.visible_foreign_cities.iter() {
                if Self::distance(&obs, u.tile, c.tile) == 1 {
                    out.push(Command::AttackCity {
                        attacker: u.handle,
                        city: c.handle,
                    });
                    sieged = true;
                    break;
                }
            }
            if sieged {
                continue;
            }
            if let Some(foe) = Self::nearest_enemy(&obs, u) {
                if foe.tile == u.tile {
                    continue;
                }
                // Adjacent: a strike (validate will almost always pass: war is already
                // declared by the driver before planning).
                if Self::distance(&obs, u.tile, foe.tile) == 1 {
                    out.push(Command::AttackUnit {
                        attacker: u.handle,
                        defender: foe.handle,
                    });
                    continue;
                }
                // Otherwise a march: driver/execute will unfold it into steps, one per turn.
                out.push(Command::MarchTo {
                    unit: u.handle,
                    destination: foe.tile,
                });
            }
        }
        out
    }
}

/// War on contact: the AI declares war on players whose units it sees.
/// A separate pass (a command outside plan; the driver decides by observation).
pub fn war_declarations(obs: &AIObservation<'_>) -> Vec<Command> {
    use std::collections::BTreeSet;
    let seen: BTreeSet<PlayerId> = obs.visible_foreign_units.iter().map(|u| u.owner).collect();
    seen.into_iter()
        .map(|enemy| Command::DeclareWar {
            player: obs.player,
            enemy,
        })
        .collect()
}

/// Rounds without sight of an enemy after which the AI makes peace.
pub const PEACE_AFTER_UNSEEN_ROUNDS: u32 = 5;

/// AI diplomacy for one turn, from the contact memory of the observation
/// (never from the truth): a war with a player not seen for
/// PEACE_AFTER_UNSEEN_ROUNDS rounds (or never seen) ends in peace with open
/// borders; a met player at peace gets open borders. The memory keeps one
/// lost sight from flipping a war into peace and back every turn.
pub fn diplomacy_commands(obs: &AIObservation<'_>) -> Vec<Command> {
    let player = obs.player;
    let mut out = Vec::new();
    for relation in &obs.relations {
        let other = relation.other;
        if relation.at_war {
            let unseen_since = match relation.contact.last_seen_round {
                Some(round) => obs.round.saturating_sub(round),
                None => u32::MAX,
            };
            if unseen_since >= PEACE_AFTER_UNSEEN_ROUNDS {
                out.push(Command::MakePeace {
                    player,
                    enemy: other,
                });
                out.push(Command::OpenBorders {
                    host: player,
                    guest: other,
                });
            }
        } else if !relation.we_grant_entry {
            out.push(Command::OpenBorders {
                host: player,
                guest: other,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
use crate::game::ClassicGame;
    use super::*;
    use crate::ids::{TechId, UnitHandle, UnitKindId};
    use crate::rules::SourceRules;
    use crate::world::state::GameConfig;
    use crate::{compile_rules, GameState};

    fn arena() -> (GameState, CompiledRuleset) {
        let rules = compile_rules(SourceRules::default()).unwrap();
        let game = GameState::new(
            &rules,
            GameConfig {
                players: 2,
                width: 16,
                height: 16,
                rng_seed: 0,
            },
        );
        (game, rules)
    }

    /// Unit handles by owner (for the test's attacking commands).
    fn units_of(game: &GameState, p: PlayerId) -> Vec<UnitHandle> {
        game.view().auth.indexes().units_of(p).to_vec()
    }

    #[test]
    fn ai_ignores_hidden() {
        // Hidden enemy: no command refers to it (neither handle nor target).
        let (mut game, rules) = arena();
        let dims = game.view().auth.map().dimensions;
        let t0 = dims.to_tile(crate::math::HexCoord { q: 1, r: 1 }).unwrap();
        let far = dims
            .to_tile(crate::math::HexCoord { q: 15, r: 15 })
            .unwrap();
        game.debug_spawn_unit(&rules, PlayerId(0), t0, UnitKindId(0));
        let hidden = game.debug_spawn_unit(&rules, PlayerId(1), far, UnitKindId(0));
        let obs = game.ai_observation(&rules, PlayerId(0));
        let mut ai = GreedyPlanner;
        let cmds = ai.plan(obs, &rules);
        let dump = format!("{:?}", cmds);
        assert!(
            !dump.contains(&format!("{:?}", hidden.index)),
            "AI must not reference hidden unit: {}",
            dump
        );
        // And untouched cities: expansion via FoundCity is ok, but only its own.
        for cmd in &cmds {
            if let Command::FoundCity { settler } = cmd {
                assert!(units_of(&game, PlayerId(0)).contains(settler));
            }
        }
    }

    #[test]
    fn ai_attacks_visible_enemy() {
        // A visible enemy nearby + a second unit (the first is a settler): the plan attacks.
        let (mut game, rules) = arena();
        let dims = game.view().auth.map().dimensions;
        let t0 = dims.to_tile(crate::math::HexCoord { q: 4, r: 4 }).unwrap();
        let t0b = dims.to_tile(crate::math::HexCoord { q: 4, r: 5 }).unwrap();
        let t1 = dims.to_tile(crate::math::HexCoord { q: 5, r: 4 }).unwrap();
        let foe = game.debug_spawn_unit(&rules, PlayerId(1), t1, UnitKindId(0));
        game.debug_spawn_unit(&rules, PlayerId(0), t0, UnitKindId(0));
        game.debug_spawn_unit(&rules, PlayerId(0), t0b, UnitKindId(0));
        let obs = game.ai_observation(&rules, PlayerId(0));
        assert_eq!(obs.visible_foreign_units.len(), 1);
        let mut ai = GreedyPlanner;
        let cmds = ai.plan(obs, &rules);
        assert!(
            cmds.iter().any(|c| matches!(
                c,
                Command::AttackUnit { defender, .. } if *defender == foe
            )),
            "expected attack on visible foe: {:?}",
            cmds
        );
    }

    #[test]
    fn ai_founds_first_city_when_cityless() {
        let (mut game, rules) = arena();
        let dims = game.view().auth.map().dimensions;
        let t0 = dims.to_tile(crate::math::HexCoord { q: 4, r: 4 }).unwrap();
        game.debug_spawn_unit(&rules, PlayerId(0), t0, UnitKindId(0));
        let obs = game.ai_observation(&rules, PlayerId(0));
        let mut ai = GreedyPlanner;
        let cmds = ai.plan(obs, &rules);
        assert!(cmds.iter().any(|c| matches!(c, Command::FoundCity { .. })));
    }

    #[test]
    fn war_declarations_cover_visible_owners() {
        let (mut game, rules) = arena();
        let dims = game.view().auth.map().dimensions;
        let t0 = dims.to_tile(crate::math::HexCoord { q: 4, r: 4 }).unwrap();
        let t1 = dims.to_tile(crate::math::HexCoord { q: 5, r: 4 }).unwrap();
        game.debug_spawn_unit(&rules, PlayerId(0), t0, UnitKindId(0));
        game.debug_spawn_unit(&rules, PlayerId(1), t1, UnitKindId(0));
        let obs = game.ai_observation(&rules, PlayerId(0));
        let decls = war_declarations(&obs);
        assert_eq!(decls.len(), 1);
        assert!(matches!(
            decls[0],
            Command::DeclareWar { player, enemy }
            if player == PlayerId(0) && enemy == PlayerId(1)
        ));
        let _ = TechId(0);
    }

    #[test]
    fn planner_queues_cheapest_available_tech() {
        // An AI with an empty queue orders the cheapest available tech
        // (from the observation only; no hidden info, cheats excluded).
        // A fixture from RON: agri(0) is flat, pottery(1) comes after agri.
        let rules = crate::fixtures::chained_content();
        let mut game = GameState::new(
            &rules,
            GameConfig {
                players: 2,
                width: 16,
                height: 16,
                rng_seed: 3,
            },
        );
        // A city is needed: without it there are no beakers and agriculture will not complete.
        let dims = game.view().auth.map().dimensions;
        let t0 = dims.to_tile(crate::math::HexCoord { q: 8, r: 8 }).unwrap();
        game.debug_found_city(&rules, PlayerId(0), t0);
        let obs = game.ai_observation(&rules, PlayerId(0));
        let mut ai = GreedyPlanner;
        let cmds = ai.plan(obs, &rules);
        assert!(
            cmds.iter().any(|c| matches!(
                c,
                Command::ChooseTechnology { player, technology }
                if *player == PlayerId(0) && *technology == TechId(0)
            )),
            "planner must queue agriculture, got {:?}",
            cmds
        );
        // Executed + learned with beakers -> the next plan takes pottery.
        game.execute(
            &rules,
            Command::ChooseTechnology {
                player: PlayerId(0),
                technology: TechId(0),
            },
        )
        .unwrap();
        for _ in 0..30 {
            game.execute(
                &rules,
                Command::EndTurn {
                    player: PlayerId(0),
                },
            )
            .unwrap();
            if game
                .view()
                .auth
                .players()
                .techs_of(PlayerId(0))
                .contains(&TechId(0))
            {
                break;
            }
        }
        let obs = game.ai_observation(&rules, PlayerId(0));
        let cmds = ai.plan(obs, &rules);
        assert!(
            cmds.iter().any(|c| matches!(
                c,
                Command::ChooseTechnology { player, technology }
                if *player == PlayerId(0) && *technology == TechId(1)
            )),
            "planner must advance to pottery, got {:?}",
            cmds
        );
    }
}
