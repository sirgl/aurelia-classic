//! The random mode: a random combination of primitives at random strengths on a position, one nation
//! of it the seat whose outcome is judged. The primitives are the situations' own (`edit::Change`,
//! the builders of `scene` and `tricks`); the worst cases by damage to the seat become named
//! situations (`R1`-`R5`, `wave3`).

use classic_rules::ids::{PlayerId, TileId};
use classic_rules::CompiledRuleset;
use kernel::rules::content::ResourceClass;

use super::edit::{Accord, Change, Gift, Yield};
use super::position::{mix, Position};
use super::scene::{self, Edit, Plan, Roles};
use super::tricks;
use crate::bench::modules::majors;

/// One primitive with its strength, as the sampler drew it.
#[derive(Debug, Clone, Copy)]
pub enum Primitive {
    /// `percent`% of the military of a nation removed.
    Disarm { nation: PlayerId, percent: u64 },
    /// `units` units of `enemy`'s kinds inside the seat's land, in a war.
    Invade { enemy: PlayerId, units: usize },
    /// A nation's science raised by `percent`.
    Boost { nation: PlayerId, percent: u32 },
    /// `cities` cities of the seat go to `taker`, in a war.
    Seize { taker: PlayerId, cities: usize },
    /// `enemy` declares war on the seat.
    War { enemy: PlayerId },
    /// The seat's treasury emptied and its luxuries gone.
    Bankrupt,
    /// The seat's strategic resources gone.
    Deplete,
    /// `percent`% of a rival's land army copied on its land.
    Arm { rival: PlayerId, percent: usize },
    /// A rival's gold raised by `percent` every round.
    Riches { rival: PlayerId, percent: u32 },
    /// A rival founds a city at the seat's border with two citadels next to it.
    Grab { rival: PlayerId },
    /// `units` of `enemy` inside the seat's land at peace (open borders), the war three rounds later.
    Sneak { enemy: PlayerId, units: usize },
}

impl Primitive {
    /// The kind of the primitive, without its parties and strength: what makes two samples the
    /// same combination.
    pub fn kind(&self) -> &'static str {
        match self {
            Primitive::Disarm { .. } => "disarm",
            Primitive::Invade { .. } => "invade",
            Primitive::Boost { .. } => "boost",
            Primitive::Seize { .. } => "seize",
            Primitive::War { .. } => "war",
            Primitive::Bankrupt => "bankrupt",
            Primitive::Deplete => "deplete",
            Primitive::Arm { .. } => "arm",
            Primitive::Riches { .. } => "riches",
            Primitive::Grab { .. } => "grab",
            Primitive::Sneak { .. } => "sneak",
        }
    }
}

/// Kinds of primitive the sampler draws from.
const KINDS: u64 = 11;

/// The rounds of the window the sneak stack waits before its war.
const SNEAK_WAR_AFTER: u32 = 3;

/// The plan and edit of sample `seed` on the position, with a one-line description and the
/// combination (the sorted kinds of its primitives).
pub fn sample(rules: &CompiledRuleset, position: &Position, seed: u64) -> Result<(Plan, Edit, String, String), String> {
    let game = &position.game;
    let all = majors(game);
    if all.len() < 2 {
        return Err("fewer than two nations".to_owned());
    }
    let draw = |salt: u64| mix(seed ^ salt.wrapping_mul(0x9e37_79b9));
    let seat = all[(draw(1) % all.len() as u64) as usize];
    let rivals: Vec<PlayerId> = all.iter().copied().filter(|p| *p != seat).collect();
    let rival = |salt: u64| rivals[(draw(salt) % rivals.len() as u64) as usize];
    let count = 1 + (draw(2) % 3) as usize;
    let primitives: Vec<Primitive> = (0..count as u64).map(|i| draw_primitive(&draw, seat, &rival, 10 + i * 10)).collect();
    let mut edit = Edit::default();
    let mut text = Vec::new();
    for primitive in &primitives {
        text.push(format!("{primitive:?}"));
        add(rules, position, seat, *primitive, seed, &mut edit)?;
    }
    let mut kinds: Vec<&str> = primitives.iter().map(Primitive::kind).collect();
    kinds.sort_unstable();
    let plan = Plan { roles: Roles::Seat { seat: seat.0 }, base: Vec::new(), info: Default::default() };
    Ok((plan, edit, format!("seat {} / {}", seat.0, text.join(" + ")), kinds.join("+")))
}

fn draw_primitive(draw: &dyn Fn(u64) -> u64, seat: PlayerId, rival: &dyn Fn(u64) -> PlayerId, salt: u64) -> Primitive {
    let strength = draw(salt + 1);
    match draw(salt) % KINDS {
        0 => Primitive::Disarm { nation: if draw(salt + 2) % 2 == 0 { seat } else { rival(salt + 3) }, percent: 25 + strength % 76 },
        1 => Primitive::Invade { enemy: rival(salt + 3), units: 3 + (strength % 10) as usize },
        2 => Primitive::Boost { nation: rival(salt + 3), percent: 25 + (strength % 76) as u32 },
        3 => Primitive::Seize { taker: rival(salt + 3), cities: 1 + (strength % 3) as usize },
        4 => Primitive::War { enemy: rival(salt + 3) },
        5 => Primitive::Bankrupt,
        6 => Primitive::Deplete,
        7 => Primitive::Arm { rival: rival(salt + 3), percent: 50 + (strength % 151) as usize },
        8 => Primitive::Riches { rival: rival(salt + 3), percent: 50 + (strength % 151) as u32 },
        9 => Primitive::Grab { rival: rival(salt + 3) },
        _ => Primitive::Sneak { enemy: rival(salt + 3), units: 3 + (strength % 8) as usize },
    }
}

/// The changes of one primitive, added to the edit.
pub(super) fn add(rules: &CompiledRuleset, position: &Position, seat: PlayerId, primitive: Primitive, seed: u64, edit: &mut Edit) -> Result<(), String> {
    let game = &position.game;
    let war_on_seat = |by: PlayerId| [Change::Meet { a: by.0, b: seat.0 }, Change::War { a: by.0, b: seat.0 }];
    let gift = |player: PlayerId, percent: u32, yield_kind: Yield| Gift { player: player.0, percent, yield_kind };
    match primitive {
        Primitive::Disarm { nation, percent } => edit.changes.push(tricks::disarm(game, rules, nation, percent, seed)),
        Primitive::Invade { enemy, units } => {
            edit.changes.extend(war_on_seat(enemy));
            edit.changes.extend(scene::invaders(game, rules, seat, enemy, units, seed)?);
        }
        Primitive::Boost { nation, percent } => {
            edit.gift.get_or_insert(gift(nation, percent, Yield::Science));
        }
        Primitive::Seize { taker, cities } => {
            let auth = game.view().auth;
            let capital = classic_rules::combat::city::capital_of(auth, rules, seat).and_then(|c| auth.cities().get(c)).map(|c| c.id.0).ok_or("the seat has no capital")?;
            edit.changes.extend(war_on_seat(taker));
            edit.changes.extend(scene::lost_cities(game, seat, taker, capital, cities));
        }
        Primitive::War { enemy } => edit.changes.extend(war_on_seat(enemy)),
        Primitive::Bankrupt => {
            edit.changes.push(Change::Treasury { player: seat.0, gold: i32::MIN });
            edit.changes.push(tricks::deplete(game, rules, seat, ResourceClass::Luxury, usize::MAX));
        }
        Primitive::Deplete => edit.changes.push(tricks::deplete(game, rules, seat, ResourceClass::Strategic, usize::MAX)),
        Primitive::Arm { rival, percent } => edit.changes.extend(tricks::army_copies(game, rules, rival, percent, seed)),
        Primitive::Riches { rival, percent } => {
            edit.gift.get_or_insert(gift(rival, percent, Yield::Gold));
        }
        Primitive::Grab { rival } => {
            let tile: TileId = tricks::forward_tile(game, rules, seat, rival, position.round)?;
            edit.changes.push(Change::Meet { a: rival.0, b: seat.0 });
            edit.changes.extend(tricks::grab(game, rules, seat, rival, tile, 1));
        }
        Primitive::Sneak { enemy, units } => {
            edit.changes.push(Change::Meet { a: enemy.0, b: seat.0 });
            if !game.view().auth.diplomacy().at_war(enemy, seat) {
                edit.changes.push(Change::Accord { giver: seat.0, receiver: enemy.0, accord: Accord::OpenBorders });
                edit.changes.push(Change::Later { after: SNEAK_WAR_AFTER, then: Box::new(Change::War { a: enemy.0, b: seat.0 }) });
            }
            edit.changes.extend(scene::invaders(game, rules, seat, enemy, units, mix(seed ^ 0x5eac))?);
        }
    }
    Ok(())
}
