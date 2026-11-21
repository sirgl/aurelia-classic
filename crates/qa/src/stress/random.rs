//! The random mode: a random combination of primitives at random strengths on a position, one nation
//! of it the seat whose outcome is judged. The primitives are the situations' own (`edit::Change`,
//! `scene`'s builders); the worst cases by damage to the seat become named situations.

use classic_rules::ids::PlayerId;
use classic_rules::CompiledRuleset;

use super::edit::{Change, Gift};
use super::position::{mix, Position};
use super::scene::{self, Edit, Plan, Roles};
use crate::bench::modules::majors;

/// One primitive with its strength, as the sampler drew it.
#[derive(Debug, Clone, Copy)]
enum Primitive {
    /// `n`% of the military of a nation removed.
    Disarm { nation: PlayerId, percent: u64 },
    /// `units` units of `enemy`'s kinds inside the seat's land, in a war.
    Invade { enemy: PlayerId, units: usize },
    /// A nation's science raised by `percent`.
    Boost { nation: PlayerId, percent: u32 },
    /// `cities` cities of the seat go to `taker`, in a war.
    Seize { taker: PlayerId, cities: usize },
    /// `enemy` declares war on the seat.
    War { enemy: PlayerId },
}

/// The plan and edit of sample `seed` on the position, with a one-line description.
pub fn sample(rules: &CompiledRuleset, position: &Position, seed: u64) -> Result<(Plan, Edit, String), String> {
    let game = &position.game;
    let all = majors(game);
    let draw = |salt: u64| mix(seed ^ salt.wrapping_mul(0x9e37_79b9));
    let pick = |salt: u64| all[(draw(salt) % all.len() as u64) as usize];
    let seat = pick(1);
    let other = |salt: u64| {
        let rivals: Vec<PlayerId> = all.iter().copied().filter(|p| *p != seat).collect();
        rivals[(draw(salt) % rivals.len().max(1) as u64) as usize % rivals.len().max(1)]
    };
    if all.len() < 2 {
        return Err("fewer than two nations".to_owned());
    }
    let count = 1 + (draw(2) % 2) as usize;
    let primitives: Vec<Primitive> = (0..count as u64)
        .map(|i| {
            let salt = 10 + i * 10;
            let strength = draw(salt + 1);
            match draw(salt) % 5 {
                0 => Primitive::Disarm { nation: if draw(salt + 2) % 2 == 0 { seat } else { other(salt + 3) }, percent: 25 + strength % 76 },
                1 => Primitive::Invade { enemy: other(salt + 3), units: 3 + (strength % 10) as usize },
                2 => Primitive::Boost { nation: pick(salt + 3), percent: 25 + (strength % 76) as u32 },
                3 => Primitive::Seize { taker: other(salt + 3), cities: 1 + (strength % 3) as usize },
                _ => Primitive::War { enemy: other(salt + 3) },
            }
        })
        .collect();
    let mut changes = Vec::new();
    let mut gift: Option<Gift> = None;
    let mut text = Vec::new();
    for primitive in primitives {
        text.push(format!("{primitive:?}"));
        match primitive {
            Primitive::Disarm { nation, percent } => {
                let ids = super::edit::military_units(game, rules, nation).into_iter().filter(|id| mix(*id ^ seed) % 100 < percent).collect();
                changes.push(Change::RemoveUnits { ids });
            }
            Primitive::Invade { enemy, units } => {
                changes.push(Change::Meet { a: enemy.0, b: seat.0 });
                changes.push(Change::War { a: enemy.0, b: seat.0 });
                changes.extend(scene::invaders(game, rules, seat, enemy, units, seed)?);
            }
            Primitive::Boost { nation, percent } => {
                gift.get_or_insert(Gift { player: nation.0, percent });
            }
            Primitive::Seize { taker, cities } => {
                let capital = classic_rules::combat::city::capital_of(game.view().auth, rules, seat).and_then(|c| game.view().auth.cities().get(c)).map(|c| c.id.0).ok_or("the seat has no capital")?;
                changes.push(Change::Meet { a: taker.0, b: seat.0 });
                changes.push(Change::War { a: taker.0, b: seat.0 });
                changes.extend(scene::lost_cities(game, seat, taker, capital, cities));
            }
            Primitive::War { enemy } => {
                changes.push(Change::Meet { a: enemy.0, b: seat.0 });
                changes.push(Change::War { a: enemy.0, b: seat.0 });
            }
        }
    }
    let plan = Plan { roles: Roles::Seat { seat: seat.0 }, base: Vec::new(), info: Default::default() };
    Ok((plan, Edit { changes, gift }, format!("seat {} / {}", seat.0, text.join(" + "))))
}
