//! Followers and majority and the faith price tables (rules.md).

use super::*;
use crate::world::religion::{CityFaith, Pressure};

fn city(entries: &[(Creed, u32)]) -> CityFaith {
    let mut pressure: Vec<Pressure> = entries
        .iter()
        .map(|(creed, amount)| Pressure {
            creed: *creed,
            amount: *amount,
        })
        .collect();
    pressure.sort_by_key(|p| p.creed);
    CityFaith {
        pressure,
        population: 0,
        ..CityFaith::default()
    }
}

const R0: Creed = Creed::Religion(ReligionId(0));
const R1: Creed = Creed::Religion(ReligionId(1));

#[test]
fn an_untouched_city_is_all_atheists() {
    let faith = CityFaith::default();
    assert_eq!(followers(&faith, 4), vec![(Creed::Atheism, 4)]);
    assert_eq!(majority(&faith, 4), None);
}

#[test]
fn followers_split_by_pressure_and_remainders() {
    // 6 citizens, total 6000 + 3500 + 2500 = 12 000, 2000 per citizen:
    // atheists 3 (0 left), R0 1 (1500 left), R1 1 (500 left); the sixth
    // citizen goes to the largest remainder (R0).
    let faith = city(&[(Creed::Atheism, 6000), (R0, 3500), (R1, 2500)]);
    assert_eq!(
        followers(&faith, 6),
        vec![(Creed::Atheism, 3), (R0, 2), (R1, 1)]
    );
    // Atheism leads but is never a majority.
    assert_eq!(majority(&faith, 6), None);
}

#[test]
fn a_majority_needs_half_of_the_citizens() {
    let faith = city(&[(Creed::Atheism, 2000), (R0, 2000), (R1, 1000)]);
    // 5 citizens: 2 / 2 / 1; R0 ties atheism on followers and pressure
    // (atheism first by creed order) — no religion has half.
    assert_eq!(majority(&faith, 5), None);
    let faith = city(&[(Creed::Atheism, 1000), (R0, 10_000)]);
    assert_eq!(followers(&faith, 1), vec![(R0, 1)]);
    assert_eq!(majority(&faith, 1), Some(R0));
}

#[test]
fn ties_on_followers_go_to_more_pressure() {
    let faith = city(&[(R0, 2000), (R1, 2100)]);
    assert_eq!(followers(&faith, 2), vec![(R0, 1), (R1, 1)]);
    assert_eq!(majority(&faith, 2), Some(R1));
}

#[test]
fn faith_prices_follow_the_rules_tables() {
    // Prophets: 200, 300, 500, 800, 1200, 1700, 2300.
    let prophets: Vec<u32> = (1..=7).map(prophet_number).collect();
    assert_eq!(prophets, vec![200, 300, 500, 800, 1200, 1700, 2300]);
    // Great people for faith: 1000, 1500, 2500, 4000, 6000.
    let people: Vec<u32> = (1..=5).map(great_person_number).collect();
    assert_eq!(people, vec![1000, 1500, 2500, 4000, 6000]);
}
