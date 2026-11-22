//! The integer forward pass of `net2`'s neurons (`weights`): a frame of the layers a neuron may read,
//! one neuron, the seat's and a rival's concepts. No float: the same answer on every platform.

use crate::ai::strategy_net::layers::{unit, ONE};
use crate::ai::strategy_net::weights::Act;
use crate::ai::strategy_net::Milli;

use super::plan::{RivalInputs, SeatInputs, TargetInputs};
use super::weights::{self as w, q, r, s, Join, Neuron, Shape, Src, Term};

/// The layers one neuron reads; a layer not yet computed is an empty slice (reading it is a defect of
/// the tables, caught by `tests::every_term_reads_a_layer_of_its_stage`).
#[derive(Debug, Clone, Copy, Default)]
pub struct Frame<'a> {
    pub f: &'a [Milli],
    pub k: &'a [Milli],
    pub g: &'a [Milli],
    pub s: &'a [Milli],
    pub r: &'a [Milli],
    pub q: &'a [Milli],
    pub a: &'a [Milli],
    pub y: &'a [Milli],
    pub c: &'a [Milli],
    pub t: &'a [Milli],
}

impl Frame<'_> {
    pub fn value(&self, src: Src) -> Milli {
        match src {
            Src::F(i) => self.f[i],
            Src::K(i) => self.k[i],
            Src::G(i) => self.g[i],
            Src::S(i) => self.s[i],
            Src::R(i) => self.r[i],
            Src::Q(i) => self.q[i],
            Src::A(i) => self.a[i],
            Src::Y(i) => self.y[i],
            Src::C(i) => self.c[i],
            Src::T(i) => self.t[i],
        }
    }

    /// A term's input after its shape.
    pub fn shaped(&self, term: &Term) -> Milli {
        let x = self.value(term.src);
        match term.shape {
            Shape::Plain => x,
            Shape::Not => ONE - x,
            Shape::Above(c) => (x - c).max(0),
            Shape::Centered(c) => x - c,
            Shape::Times(other) => x * self.value(other) / ONE,
        }
    }
}

/// One neuron: `bias + (Σ w·x) / 1000` and its activation, or the largest `w·x / 1000` clamped.
pub fn fire(neuron: &Neuron, frame: &Frame<'_>) -> Milli {
    match neuron.join {
        Join::Sum(act) => {
            let mut sum = 0;
            for term in neuron.terms {
                sum += term.w * frame.shaped(term);
            }
            let out = neuron.bias + sum / ONE;
            match act {
                Act::Unit => unit(out),
                Act::Linear => out,
            }
        }
        Join::Max => {
            let mut best = 0;
            for term in neuron.terms {
                best = best.max(term.w * frame.shaped(term) / ONE);
            }
            unit(best)
        }
    }
}

impl SeatInputs {
    /// The inputs in `weights::g`'s order.
    pub fn row(&self) -> [Milli; w::g::N] {
        [
            self.capital_lost,
            self.cities_lost,
            self.war_rounds,
            self.war_gain,
            self.unhappy_trend,
            self.embark,
            self.enemy_air,
            self.gold_stock,
            self.cs_allies,
            self.cs_contested,
            self.own_infamy,
            self.ties_at_risk,
            self.space_race,
            self.parts_short,
        ]
    }
}

impl RivalInputs {
    /// The inputs in `weights::r`'s order; `soft` is the core's aggregate of the targets (`levers::soft_of`).
    pub fn row(&self, soft: Milli) -> [Milli; r::N] {
        [
            self.near,
            self.land,
            self.sea,
            self.soldiers,
            self.seen_army,
            self.open_cities,
            self.tree,
            self.lead,
            self.pace,
            self.culture,
            self.stack,
            self.warmonger,
            self.betrayed,
            self.denounced_us,
            self.friend,
            self.pact,
            self.at_war,
            self.war_rounds,
            self.holds_capital,
            self.holds_cities,
            self.grab,
            self.ally_hit,
            self.local,
            self.score,
            self.retake_case,
            self.captured_recent,
            soft,
            self.operating,
            self.alone,
            self.cs_held,
        ]
    }
}

impl TargetInputs {
    /// The inputs in `weights::t`'s order.
    pub fn row(&self) -> [Milli; w::t::N] {
        [self.size, self.garrison, self.near, self.walls]
    }
}

/// One target's ease (`weights::EASE`).
pub fn ease(target: &TargetInputs) -> Milli {
    fire(&w::EASE, &Frame { t: &target.row(), ..Frame::default() })
}

/// The new seat concepts (§4.1) of the features, the carried concepts and the seat inputs.
pub fn seat(features: &[Milli], carried: &[Milli], seat: &[Milli; w::g::N]) -> [Milli; s::N] {
    let mut out = [0; s::N];
    for (i, neuron) in w::SEAT.iter().enumerate() {
        let frame = Frame { f: features, k: carried, g: seat, ..Frame::default() };
        out[i] = fire(neuron, &frame);
    }
    out
}

/// One rival's concepts (§4.2), in order: a concept reads the earlier ones.
pub fn rival(carried: &[Milli], seat: &[Milli; s::N], inputs: &[Milli; r::N]) -> [Milli; q::N] {
    let mut out = [0; q::N];
    for (i, neuron) in w::RIVAL.iter().enumerate() {
        let frame = Frame { k: carried, s: seat, r: inputs, q: &out, ..Frame::default() };
        out[i] = fire(neuron, &frame);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::net2::weights::{Table, TILT};

    /// The layers each stage of the pass offers its neurons.
    fn stage_of(name: &str) -> &'static [char] {
        match name {
            "war_weary" | "capital_fall" | "city_state_room" | "air_need" => &['F', 'K', 'G'],
            "peace" => &['S', 'R', 'Q'],
            "guard" => &['S', 'C'],
            "military_tilt" => &['S', 'A', 'Y'],
            "infamy_cost" | "late" => &['F', 'K', 'G'],
            "ease" => &['T'],
            "cs_gold_share" => &['F', 'K', 'G', 'Y'],
            "part_hold" => &['K', 'G'],
            _ => &['K', 'S', 'R', 'Q'],
        }
    }

    fn layer(src: Src) -> (char, usize, usize) {
        match src {
            Src::F(i) => ('F', i, crate::ai::strategy_net::weights::fe::N),
            Src::K(i) => ('K', i, crate::ai::strategy_net::weights::kc::N),
            Src::G(i) => ('G', i, w::g::N),
            Src::S(i) => ('S', i, s::N),
            Src::R(i) => ('R', i, r::N),
            Src::Q(i) => ('Q', i, q::N),
            Src::A(i) => ('A', i, w::a::N),
            Src::Y(i) => ('Y', i, w::y::N),
            Src::C(i) => ('C', i, w::c::N),
            Src::T(i) => ('T', i, w::t::N),
        }
    }

    #[test]
    fn every_term_reads_a_layer_of_its_stage() {
        for neuron in w::neurons() {
            let allowed = stage_of(neuron.name);
            for term in neuron.terms {
                let mut srcs = vec![term.src];
                if let Shape::Times(other) = term.shape {
                    srcs.push(other);
                }
                for src in srcs {
                    let (l, i, n) = layer(src);
                    assert!(allowed.contains(&l), "{}: {:?} outside its stage", neuron.name, src);
                    assert!(i < n, "{}: {:?} out of its layer", neuron.name, src);
                }
            }
        }
        for (i, neuron) in w::RIVAL.iter().enumerate() {
            for term in neuron.terms {
                if let Src::Q(j) = term.src {
                    assert!(j < i, "{} reads the later concept {j}", neuron.name);
                }
            }
        }
        for (table, at, term) in TILT {
            let n = match table {
                Table::Style => 3,
                Table::Aim => 4,
                Table::Posture => 4,
            };
            assert!(*at < n);
            assert!(matches!(term.src, Src::S(_) | Src::A(_)), "a tilt reads the seat concepts and the aggregates only");
        }
    }

    #[test]
    fn every_weight_has_its_reason() {
        for neuron in w::neurons() {
            assert!(!neuron.why.is_empty(), "{}", neuron.name);
            for term in neuron.terms {
                assert!(!term.why.is_empty(), "{}", neuron.name);
            }
        }
        for (_, _, term) in TILT {
            assert!(!term.why.is_empty());
        }
    }
}
