//! The chance an order of a world wonder is won, percent: GENERATED, do not edit.
//!
//! Command: python3 -I rival/runs/rival-fit.py ../classic/crates/classic-rules/src/ai/wonder_chance.rs p/smoke2.jsonl
//! Source: 2263 orders of `sharp` in the pace bench (probe lines, `bench::rival`), 46.1 % won.
//! Shrinkage 12 pseudo-orders toward the row, the row toward the whole; non-increasing in the turns.

/// Upper bound (inclusive) of each row, in turns at Standard speed.
pub const TURNS: [u32; 8] = [6, 10, 14, 19, 25, 32, 42, 1000000];

/// Columns: met rivals able to build the wonder, 0, 1, 2, 3 or more.
pub const CHANCE: [[u8; 4]; 8] = [
    [92, 85, 91, 90],
    [68, 67, 58, 53],
    [61, 47, 54, 41],
    [54, 40, 37, 35],
    [48, 35, 37, 35],
    [39, 29, 32, 26],
    [20, 27, 32, 26],
    [17, 10, 9, 14],
];
