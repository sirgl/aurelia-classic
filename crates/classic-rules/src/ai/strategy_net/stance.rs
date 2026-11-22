//! The stance toward each known city-state (`strategist-x`, `weights::stance`): prey (its cities are
//! the war machine's targets) and client (courted by a pledge), from the city-state screen, the
//! distance of its capital to our nearest city and the intent.

use crate::knowledge::observation::{AIObservation, CityStateInfo};

use super::layers::ONE;
use super::weights::{kc, stance as w};
use super::wishes::Stance;
use super::{Intent, Milli};

/// `near`: 1 at `NEAR_FULL` tiles from our nearest city, 0 at `NEAR_NONE` and beyond (0: capital unknown).
fn near(obs: &AIObservation<'_>, info: &CityStateInfo) -> Milli {
    let Some(capital) = info.capital else { return 0 };
    let dims = obs.view.dimensions();
    let Some(distance) = obs.own_cities.iter().filter_map(|c| dims.distance(c.tile, capital)).min() else { return 0 };
    let span = i64::from(w::NEAR_NONE - w::NEAR_FULL).max(1);
    (i64::from(w::NEAR_NONE - distance) * ONE / span).clamp(0, ONE)
}

/// The value of a named input of the stance neurons.
fn input(name: &str, intent: &Intent, near: Milli, ours: Milli) -> Milli {
    match name {
        "bias" => ONE,
        "style_conquest" => intent.style[2],
        "warlike" => intent.concepts[kc::WARLIKE],
        "near" => near,
        "city_state_play" => intent.concepts[kc::CITY_STATE_PLAY],
        "aim_diplomacy" => intent.aim[super::weights::AIM_DIPLOMACY],
        "ours" => ours,
        _ => 0,
    }
}

fn fire(terms: &[(&str, Milli, &str)], intent: &Intent, near: Milli, ours: Milli) -> Milli {
    let sum: Milli = terms.iter().map(|(name, weight, _)| weight * input(name, intent, near, ours) / ONE).sum();
    sum.clamp(0, ONE)
}

/// The stance toward every city-state on the screen.
pub fn of(obs: &AIObservation<'_>, intent: &Intent) -> Vec<Stance> {
    let mut out = Vec::new();
    for info in &obs.city_states {
        let near = near(obs, info);
        let ours = if info.ally == Some(obs.player) || info.friends || info.pledged { ONE } else { 0 };
        let parts = super::wishes::Parts::of_env();
        let prey = if parts.prey { fire(w::PREY, intent, near, ours) } else { 0 };
        let client = if parts.client { fire(w::CLIENT, intent, near, ours) } else { 0 };
        out.push(Stance { city_state: info.player, prey, client });
    }
    out
}
