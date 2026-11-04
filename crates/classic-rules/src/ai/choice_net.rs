//! The choice net: two small learned models, one for what a city builds
//! and one for what a player researches, that estimate per candidate of a
//! choice set (`ai::choice`) the outcome the choice leads to. Trained
//! offline by `ai-train-choice` (aurelia-qa, its `nn` module) on the
//! explored choices of AI-only games (`ai-choice`): there the candidate
//! taken was a random one of the allocator's top, so the outcome compares
//! candidates and not the states the allocator meets them in. The weights
//! are the generated `choice_weights`; `Variant::ChoiceNet` takes the
//! candidate of the allocator's top with the best answer
//! (classic/docs/ai-choice.md).
//!
//! Integer only (`ai::net`, the AI's one inference of its learned nets),
//! so the game gets the same answer on every platform.

#[cfg(feature = "experimental-ai")]
use super::choice_weights;
use super::net::{self, Net};

/// The two choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// What a city builds (`ai::allocator`).
    Production,
    /// What the player researches (`planner_research`).
    Research,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Production => "production",
            Kind::Research => "research",
        }
    }

    /// The net of the choice; None until one is trained.
    pub fn net(self) -> Option<&'static Net<'static>> {
        // The tables are `Variant::ChoiceNet`'s alone: compiled only with `experimental-ai`.
        #[cfg(feature = "experimental-ai")]
        return match self {
            Kind::Production => choice_weights::PRODUCTION,
            Kind::Research => choice_weights::RESEARCH,
        };
        #[cfg(not(feature = "experimental-ai"))]
        None
    }

    /// The least lead (Q16, the net's units) the net's best needs over the
    /// first row (the allocator's best) to be taken instead.
    pub fn margin(self) -> i64 {
        #[cfg(feature = "experimental-ai")]
        return match self {
            Kind::Production => choice_weights::PRODUCTION_MARGIN,
            Kind::Research => choice_weights::RESEARCH_MARGIN,
        };
        #[cfg(not(feature = "experimental-ai"))]
        0
    }
}

/// The net's answer for a row (`choice::PRODUCTION_INPUTS` or
/// `RESEARCH_INPUTS` values), its first head in Q16; None without a net.
pub fn score(kind: Kind, row: &[i32]) -> Option<i64> {
    let net = kind.net()?;
    let mut out = [0i64; net::MAX_WIDTH];
    let outputs = net.outputs();
    net::forward(net, row, &mut out[..outputs]);
    Some(out[0])
}

/// The row with the best answer (ties: the earlier, the allocator's
/// better) when it leads the first row (the allocator's best) by more than
/// the kind's margin, else the first; None without a net or with fewer than
/// two rows.
pub fn best<'r>(kind: Kind, rows: impl Iterator<Item = &'r [i32]>) -> Option<usize> {
    let scores: Vec<i64> = rows.map(|r| score(kind, r)).collect::<Option<Vec<i64>>>()?;
    if scores.len() < 2 {
        return None;
    }
    let mut at = 0;
    for (i, s) in scores.iter().enumerate() {
        if *s > scores[at] {
            at = i;
        }
    }
    Some(if scores[at] - scores[0] > kind.margin() { at } else { 0 })
}

/// A row of the test games and the trainer's integer answer for it, kept
/// beside the weights: the game's inference is checked against it.
#[derive(Debug, Clone, Copy)]
pub struct Reference {
    pub kind: Kind,
    pub row: &'static [i32],
    pub output: i64,
}

/// The trainer's answers on a few test rows (`Reference`).
#[cfg(feature = "experimental-ai")]
pub fn references() -> &'static [Reference] {
    choice_weights::REFERENCE
}

#[cfg(all(test, feature = "experimental-ai"))]
mod tests {
    use super::*;

    /// The game's inference gives the trainer's answers.
    #[test]
    fn the_inference_reproduces_the_trainer() {
        for reference in references() {
            assert_eq!(score(reference.kind, reference.row), Some(reference.output), "{:?}", reference.kind);
        }
    }
}
