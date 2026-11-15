//! The colour of each owner on the map: units, borders and banners. A nation brings its own colour
//! (`nations.ron`), but the data has near twins (Germany, Aztecs and Ethiopia are all the same red,
//! Sweden and Venice are the same green). A match gives each player a colour that differs from
//! every colour given before it: a nation whose colour is too close takes the first colour of
//! the spare palette that is far enough from all of them.
//!
//! The result depends only on the players in the order of their slots and on the nations they
//! play, so one match always looks the same, and a saved game gets its colours again from
//! its own players: nothing is stored.
use classic_rules::ids::PlayerId;
use classic_rules::world::player::{ClassicKind, PlayerKind};
use classic_rules::{CompiledRuleset, GameState};

pub type Rgb = [u8; 3];

/// Two colours of a match are at least this far apart (CIE76 distance in Lab) when the spare
/// palette has a colour for it. 25 is a clear difference at map size; 2.3 is the least that is seen.
pub const MIN_DISTANCE: f32 = 25.0;

/// The fixed colour of the barbarians (the renderer's outsider colour, as bytes).
const BARBARIAN: Rgb = [26, 26, 31];

/// Spare colours, spread round the hue circle in three lightness bands.
const SPARE: [Rgb; 24] = [
    [226, 54, 54],
    [121, 236, 155],
    [115, 25, 179],
    [226, 205, 54],
    [121, 217, 236],
    [179, 25, 108],
    [97, 226, 54],
    [126, 121, 236],
    [179, 77, 25],
    [54, 226, 162],
    [226, 121, 236],
    [146, 179, 25],
    [54, 140, 226],
    [236, 121, 145],
    [25, 179, 39],
    [119, 54, 226],
    [236, 198, 121],
    [25, 179, 172],
    [226, 54, 183],
    [173, 236, 121],
    [25, 51, 179],
    [226, 76, 54],
    [121, 236, 169],
    [134, 25, 179],
];

/// Who a player slot is, as far as its colour goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// A power that plays a nation and wants the colour of it.
    Nation(Rgb),
    /// A city-state: it has no colour of its own and takes one that is free.
    CityState,
    Barbarian,
}

/// The slots of a match, in order, for `rules`' nations.
pub fn slots_of(game: &GameState, rules: &CompiledRuleset) -> Vec<Slot> {
    let players = game.view().auth.players();
    (0..players.count() as u8)
        .map(|p| match players.kind_of(PlayerId(p)) {
            PlayerKind::Major => match rules.nations.get(usize::from(players.nation_of(PlayerId(p)))) {
                Some(nation) => Slot::Nation([nation.color.0, nation.color.1, nation.color.2]),
                None => Slot::CityState,
            },
            PlayerKind::Module(ClassicKind::CityState(_)) => Slot::CityState,
            PlayerKind::Module(ClassicKind::Barbarian) => Slot::Barbarian,
        })
        .collect()
}

/// The colour of each slot. Powers choose first, in slot order (the human is the first slot, so
/// the player keeps the colour of the nation), then the city-states take what is left.
pub fn colors_of(slots: &[Slot]) -> Vec<Rgb> {
    let mut colors = vec![BARBARIAN; slots.len()];
    let mut taken = vec![BARBARIAN];
    let first = rotation(slots);
    for (index, slot) in slots.iter().enumerate() {
        if let Slot::Nation(own) = slot {
            colors[index] = choose(Some(*own), &taken, first);
            taken.push(colors[index]);
        }
    }
    for (index, slot) in slots.iter().enumerate() {
        if *slot == Slot::CityState {
            colors[index] = choose(None, &taken, first);
            taken.push(colors[index]);
        }
    }
    colors
}

/// Where the walk of the spare palette starts: it follows the players of the match, so that
/// two matches with different nations do not hand out the spare colours in one order.
fn rotation(slots: &[Slot]) -> usize {
    let mut hash: u32 = 0x811c_9dc5;
    for slot in slots {
        let bytes = match slot {
            Slot::Nation([r, g, b]) => [1, *r, *g, *b],
            Slot::CityState => [2, 0, 0, 0],
            Slot::Barbarian => [3, 0, 0, 0],
        };
        for byte in bytes {
            hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
        }
    }
    hash as usize % SPARE.len()
}

/// `own` when it is far enough from every colour in `taken`, else the first spare colour that is.
/// With no such colour (a match with more players than the palette holds), the spare colour that
/// is farthest from all of them.
fn choose(own: Option<Rgb>, taken: &[Rgb], first: usize) -> Rgb {
    if let Some(color) = own {
        if nearest(color, taken) >= MIN_DISTANCE {
            return color;
        }
    }
    let mut best = SPARE[first];
    let mut best_distance = -1.0;
    for step in 0..SPARE.len() {
        let candidate = SPARE[(first + step) % SPARE.len()];
        let distance = nearest(candidate, taken);
        if distance >= MIN_DISTANCE {
            return candidate;
        }
        if distance > best_distance {
            best = candidate;
            best_distance = distance;
        }
    }
    best
}

/// The distance from `color` to the closest colour of `others`.
fn nearest(color: Rgb, others: &[Rgb]) -> f32 {
    others.iter().map(|other| distance(color, *other)).fold(f32::MAX, f32::min)
}

/// CIE76 colour difference: the straight distance in Lab space.
pub fn distance(a: Rgb, b: Rgb) -> f32 {
    let (a, b) = (lab(a), lab(b));
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn lab([r, g, b]: Rgb) -> [f32; 3] {
    let linear = |byte: u8| {
        let c = f32::from(byte) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (linear(r), linear(g), linear(b));
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.950_47;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.088_83;
    let f = |t: f32| if t > 216.0 / 24389.0 { t.cbrt() } else { (24389.0 / 27.0 * t + 16.0) / 116.0 };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// Gives the renderer the colours of this match. Set before the match is drawn.
pub fn paint_owners(game: &GameState, rules: &CompiledRuleset) {
    render::gpu::buffers::set_owner_palette(&colors_of(&slots_of(game, rules)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nations() -> Vec<Rgb> {
        let rules = classic_rules::compile_rules(classic_content::core::core_rules()).expect("the classic rules compile");
        rules.nations.iter().map(|n| [n.color.0, n.color.1, n.color.2]).collect()
    }

    /// `count` nations of the data, picked by `seed` without repeats.
    fn match_of(all: &[Rgb], count: usize, seed: usize) -> Vec<Slot> {
        let step = [1, 3, 7, 9, 11, 13, 17, 19][seed % 8];
        (0..count).map(|i| Slot::Nation(all[(seed * 5 + i * step) % all.len()])).collect()
    }

    fn closest_pair(colors: &[Rgb]) -> f32 {
        let mut least = f32::MAX;
        for (i, a) in colors.iter().enumerate() {
            for b in &colors[i + 1..] {
                least = least.min(distance(*a, *b));
            }
        }
        least
    }

    #[test]
    fn the_data_has_twins_the_match_must_not_show() {
        let all = nations();
        assert!(closest_pair(&all) < 2.0, "the data holds nations with one colour (Sweden, Venice)");
    }

    #[test]
    fn a_match_of_up_to_eight_nations_has_clear_colours() {
        let all = nations();
        for players in 2..=8 {
            for seed in 0..all.len() * 2 {
                let colors = colors_of(&match_of(&all, players, seed));
                let least = closest_pair(&colors);
                assert!(least >= MIN_DISTANCE, "{players} players, seed {seed}: closest pair {least}");
            }
        }
    }

    #[test]
    fn every_pair_of_the_data_gets_two_colours_in_a_two_player_match() {
        let all = nations();
        for (i, a) in all.iter().enumerate() {
            for b in &all[i..] {
                let colors = colors_of(&[Slot::Nation(*a), Slot::Nation(*b)]);
                assert!(distance(colors[0], colors[1]) >= MIN_DISTANCE, "{a:?} and {b:?}");
                assert!(distance(colors[0], BARBARIAN) >= MIN_DISTANCE, "{a:?} stays away from the barbarians");
            }
        }
    }

    /// 25 is out of reach here: 25 owners (16 powers, 8 city-states, the barbarians) are more than
    /// the 24 spare colours plus the nations' own colours can keep 25 apart in Lab. The measured
    /// closest pair over all 43 rotations is 11.99 (seed 25), so the floor is 11: still two
    /// different colours on the map, unlike the twins of the data (distance 0 to 2).
    #[test]
    fn the_biggest_match_with_city_states_stays_readable() {
        let all = nations();
        for seed in 0..all.len() {
            let mut slots = match_of(&all, 16, seed);
            slots.extend([Slot::CityState; 8]);
            slots.push(Slot::Barbarian);
            let colors = colors_of(&slots);
            let least = closest_pair(&colors);
            assert!(least >= 11.0, "seed {seed}: closest pair {least}");
        }
    }

    #[test]
    fn a_nation_with_a_clear_colour_keeps_it_and_the_choice_repeats() {
        let all = nations();
        let slots = match_of(&all, 6, 3);
        let colors = colors_of(&slots);
        assert_eq!(colors, colors_of(&slots), "the same match, the same colours");
        let Slot::Nation(first) = slots[0] else { unreachable!("the match holds nations only") };
        assert_eq!(colors[0], first, "the first player keeps the colour of the nation");
    }

    #[test]
    fn the_barbarians_and_the_city_states_have_their_places() {
        let slots = [Slot::Nation([199, 90, 93]), Slot::CityState, Slot::Barbarian, Slot::Nation([199, 91, 90])];
        let colors = colors_of(&slots);
        assert_eq!(colors[2], BARBARIAN);
        assert!(closest_pair(&[colors[0], colors[1], colors[3]]) >= MIN_DISTANCE);
    }

    /// The slots of a real match, from the new-game screen's own numbers.
    fn slots_of_match(size: crate::ui::screens::WorldSize, players: u8, city_states: u8) -> usize {
        use crate::ui::screens::{prepare_game, GameOptions, MapSize, MatchRequest, NewGame};
        let request = MatchRequest { nation: "core:nation/rome".into(), seed: 4, size: MapSize::World(size), players, water: 25 };
        let options = GameOptions { city_states, barbarians: true, ..GameOptions::default() };
        let prepared = prepare_game(NewGame { request, options }).expect("the world is generated");
        slots_of(&prepared.game, &crate::ui::screens::game_rules()).len()
    }

    /// The renderer paints `MAX_PAINTED_OWNERS` slots; a slot past it falls back to the old palette
    /// of eight colours, which another owner already wears. Every size of the setup, and the most
    /// the setup lets through (16 powers, 24 city-states, the barbarians), must fit.
    #[test]
    fn every_player_slot_of_a_real_match_is_painted_by_the_renderer() {
        use crate::ui::screens::WorldSize;
        let cap = render::gpu::buffers::MAX_PAINTED_OWNERS;
        for size in WorldSize::ALL {
            let slots = slots_of_match(size, size.nations(), size.city_states());
            assert!(slots <= cap, "{size:?}: {slots} player slots, the renderer paints {cap}");
        }
        let most = slots_of_match(WorldSize::Huge, 16, 24);
        assert_eq!(most, 41, "16 powers, 24 city-states, the barbarians");
        assert!(most <= cap, "the biggest match: {most} player slots, the renderer paints {cap}");
    }
}
