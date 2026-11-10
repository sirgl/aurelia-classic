//! The chance an order of a world wonder is won, percent: GENERATED, do not edit.
//!
//! Command: python3 -I rival/runs/rival-fit.py ../classic/crates/classic-rules/src/ai/wonder_chance.rs calib/sharp-200.jsonl calib/sharp-201.jsonl calib/sharp-202.jsonl calib/sharp-203.jsonl calib/sharp-204.jsonl calib/sharp-205.jsonl calib/sharp-206.jsonl calib/sharp-207.jsonl calib/sharp-208.jsonl calib/sharp-209.jsonl
//! Source: 67864 orders of `sharp` in the pace bench (probe lines, `bench::rival`), 51.5 % won;
//! `AGE_ROUNDS_PER_CITIZEN` 15 maximised the likelihood among [1000000, 60, 30, 15].
//! Shrinkage 15 pseudo-orders toward the parent cell; non-increasing in turns, place and the rival's size.

/// Upper bound (inclusive) of each row, in turns at Standard speed.
pub const TURNS: [u32; 8] = [6, 10, 14, 19, 25, 32, 42, 1000000];

/// Upper bound (inclusive) of each column, in citizens of the largest sighted rival city (0: none sighted).
pub const POPULATION: [u32; 4] = [0, 3, 7, 1000000];

/// Age widening, rounds per citizen.
pub const FITTED_AGE: u32 = 15;

/// By turns row, then the seat's place (1, 2, 3 or worse), then the rival's size column.
pub const CHANCE: [[[u8; 4]; 3]; 8] = [
    [[80, 80, 80, 80], [80, 76, 68, 68], [80, 76, 68, 68]],
    [[66, 66, 66, 66], [66, 66, 57, 53], [66, 63, 55, 48]],
    [[62, 62, 57, 57], [62, 62, 53, 43], [62, 62, 53, 36]],
    [[58, 58, 50, 50], [58, 58, 43, 36], [58, 58, 43, 30]],
    [[50, 50, 43, 43], [50, 50, 36, 31], [50, 48, 36, 27]],
    [[41, 41, 32, 32], [41, 32, 27, 27], [41, 26, 26, 21]],
    [[34, 29, 25, 25], [34, 17, 17, 17], [29, 17, 17, 17]],
    [[15, 8, 8, 8], [15, 8, 8, 8], [15, 8, 8, 8]],
];
