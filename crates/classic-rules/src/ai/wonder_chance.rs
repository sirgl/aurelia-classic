//! The chance an order of a world wonder is won, percent: GENERATED, do not edit.
//!
//! Command: python3 -I rival/runs/rival-fit.py ../classic/crates/classic-rules/src/ai/wonder_chance.rs calib/sharp-200.jsonl calib/sharp-201.jsonl calib/sharp-202.jsonl
//! Source: 18970 orders of `sharp` in the pace bench (probe lines, `bench::rival`), 51.3 % won;
//! `AGE_ROUNDS_PER_CITIZEN` 30 maximised the likelihood among [1000000, 60, 30, 15].
//! Shrinkage 15 pseudo-orders toward the parent cell; non-increasing in turns, place and the rival's size.

/// Upper bound (inclusive) of each row, in turns at Standard speed.
pub const TURNS: [u32; 8] = [6, 10, 14, 19, 25, 32, 42, 1000000];

/// Upper bound (inclusive) of each column, in citizens of the largest sighted rival city (0: none sighted).
pub const POPULATION: [u32; 4] = [0, 3, 7, 1000000];

/// Age widening, rounds per citizen.
pub const FITTED_AGE: u32 = 30;

/// By turns row, then the seat's place (1, 2, 3 or worse), then the rival's size column.
pub const CHANCE: [[[u8; 4]; 3]; 8] = [
    [[81, 78, 78, 78], [74, 74, 64, 64], [74, 74, 64, 64]],
    [[66, 66, 66, 66], [66, 59, 51, 51], [65, 58, 49, 47]],
    [[64, 63, 56, 56], [64, 56, 47, 44], [62, 53, 47, 36]],
    [[60, 52, 49, 49], [59, 45, 41, 37], [59, 45, 41, 31]],
    [[51, 45, 44, 44], [43, 43, 39, 29], [35, 34, 34, 21]],
    [[44, 25, 25, 25], [43, 25, 25, 25], [30, 17, 17, 17]],
    [[37, 23, 23, 23], [29, 11, 11, 11], [29, 11, 11, 11]],
    [[16, 11, 11, 11], [16, 9, 9, 9], [12, 9, 9, 9]],
];
