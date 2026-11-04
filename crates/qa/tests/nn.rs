//! The trainer (`aurelia_qa::nn`): the same samples give the same net bit
//! for bit; it learns an easy function well past the constant guess, a
//! ranking head learns an order; its integer form (`ai::net::forward`, the
//! game's one inference, and `danger_net::infer` on it) answers as the f32
//! net does.

use aurelia_qa::nn::{quantise, sigmoid, train, Config, Head, Loss, Net, Rng, Samples};
use classic_rules::ai::danger_net::infer;
use classic_rules::ai::net::{forward, ACT};

/// Rows of (hp, threat, noise): the damage is 6 hp a point of threat,
/// at most the hp; the unit dies when the threat is 8 or more and the hp
/// at most 60.
fn synthetic(rows: usize, seed: u64) -> Samples {
    let mut rng = Rng::new(seed);
    let mut out = Samples::new(3, 2);
    for _ in 0..rows {
        let hp = 1 + rng.below(100) as i32;
        let threat = rng.below(11) as i32;
        let noise = rng.below(50) as i32;
        let dead = threat >= 8 && hp <= 60;
        let damage = if dead { hp } else { (threat * 6).min(hp) };
        out.push(&[hp as f32, threat as f32, noise as f32], &[damage as f32 / 100.0, if dead { 1.0 } else { 0.0 }], 0);
    }
    out
}

fn config() -> Config {
    let heads = vec![Head { loss: Loss::Huber(1.0), weight: 1.0 }, Head { loss: Loss::Logistic, weight: 1.0 }];
    Config { hidden: vec![16, 8], heads, max_epochs: 40, patience: 8, learning_rate: 3e-3, batch: 64, ..Config::default() }
}

fn damage_hp(net: &Net, x: &[f32]) -> f32 {
    (net.forward(x)[0] * 100.0).clamp(0.0, x[0].max(0.0))
}

fn mae_hp(net: &Net, samples: &Samples) -> f32 {
    (0..samples.len()).map(|i| (damage_hp(net, samples.row(i)) - samples.target(i, 0) * 100.0).abs()).sum::<f32>() / samples.len() as f32
}

fn bits(net: &Net) -> Vec<u32> {
    net.layers.iter().flat_map(|l| l.w.iter().chain(&l.b)).map(|v| v.to_bits()).collect()
}

#[test]
fn training_is_deterministic() {
    let (a, b) = (synthetic(2000, 1), synthetic(500, 2));
    let small = Config { max_epochs: 5, ..config() };
    let (first, log_first) = train(&a, &b, &small);
    let (second, log_second) = train(&a, &b, &small);
    assert_eq!(log_first, log_second);
    assert_eq!(first, second);
    assert_eq!(bits(&first), bits(&second));
    let (other, _) = train(&a, &b, &Config { seed: 2, ..small });
    assert_ne!(bits(&first), bits(&other), "the seed matters");
}

#[test]
fn learns_an_easy_function() {
    let (a, b, test) = (synthetic(6000, 3), synthetic(1500, 4), synthetic(1500, 5));
    let (net, _) = train(&a, &b, &config());
    let mean = (0..a.len()).map(|i| a.target(i, 0)).sum::<f32>() / a.len() as f32 * 100.0;
    let constant = (0..test.len()).map(|i| (mean - test.target(i, 0) * 100.0).abs()).sum::<f32>() / test.len() as f32;
    let learned = mae_hp(&net, &test);
    assert!(learned < constant / 4.0, "MAE {learned:.2} hp against the constant guess's {constant:.2}");
    let right = (0..test.len()).filter(|i| (sigmoid(net.forward(test.row(*i))[1]) >= 0.5) == (test.target(*i, 1) > 0.5)).count();
    assert!(right as f32 > 0.97 * test.len() as f32, "death right on {right} of {}", test.len());
}

#[test]
fn integer_inference_follows_the_f32_net() {
    let (a, b, test) = (synthetic(4000, 6), synthetic(1000, 7), synthetic(1000, 8));
    let (net, _) = train(&a, &b, &config());
    let quantised = quantise(&net);
    let layers = quantised.layers();
    let int_net = quantised.net(&layers);
    for i in 0..test.len() {
        let x = test.row(i);
        let features: Vec<i32> = x.iter().map(|v| *v as i32).collect();
        let answer = infer(&int_net, &features);
        let damage = damage_hp(&net, x);
        assert!((f32::from(answer.damage) - damage).abs() <= 1.0, "row {i}: damage {} against {damage}", answer.damage);
        let death = sigmoid(net.forward(x)[1]) * 1000.0;
        assert!((f32::from(answer.death_permille) - death).abs() <= 2.0, "row {i}: death {} against {death}", answer.death_permille);
    }
}

/// Groups of 2..6 rows of (a, b, noise) whose value is `a·b − 3·a`; a
/// ranking head sees only the order within a group.
fn ranked(groups: usize, seed: u64) -> Samples {
    let mut rng = Rng::new(seed);
    let mut out = Samples::new(3, 1);
    for g in 0..groups {
        for _ in 0..2 + rng.below(5) {
            let (a, b, noise) = (rng.below(10) as f32, rng.below(10) as f32, rng.below(10) as f32);
            out.push(&[a, b, noise], &[a * b - 3.0 * a], g as u64);
        }
    }
    out
}

/// The share of the pairs within groups with different targets that the
/// net's first output orders right.
fn ordered_right(net: &Net, samples: &Samples) -> f64 {
    let (mut pairs, mut right) = (0, 0);
    for rows in samples.groups() {
        for (k, &i) in rows.iter().enumerate() {
            for &j in &rows[k + 1..] {
                let (ti, tj) = (samples.target(i, 0), samples.target(j, 0));
                if ti != tj {
                    pairs += 1;
                    right += usize::from((net.forward(samples.row(i))[0] > net.forward(samples.row(j))[0]) == (ti > tj));
                }
            }
        }
    }
    right as f64 / pairs as f64
}

#[test]
fn a_ranking_head_learns_the_order() {
    let (a, b, test) = (ranked(1500, 9), ranked(300, 10), ranked(300, 11));
    let config = Config { hidden: vec![16, 8], heads: vec![Head { loss: Loss::Rank, weight: 1.0 }], max_epochs: 60, patience: 10, learning_rate: 3e-3, batch: 64, ..Config::default() };
    let (net, _) = train(&a, &b, &config);
    let share = ordered_right(&net, &test);
    assert!(share > 0.93, "{:.3} of the pairs ordered right", share);
    let (again, _) = train(&a, &b, &config);
    assert_eq!(bits(&net), bits(&again), "ranking is deterministic too");
}

#[test]
fn integer_inference_takes_any_depth() {
    let (a, b, test) = (synthetic(3000, 12), synthetic(800, 13), synthetic(500, 14));
    for hidden in [vec![12], vec![16, 8, 8]] {
        let (net, _) = train(&a, &b, &Config { hidden, max_epochs: 15, ..config() });
        let quantised = quantise(&net);
        let layers = quantised.layers();
        let int_net = quantised.net(&layers);
        for i in 0..test.len() {
            let x = test.row(i);
            let features: Vec<i32> = x.iter().map(|v| *v as i32).collect();
            let mut out = [0i64; 2];
            forward(&int_net, &features, &mut out);
            let f32_out = net.forward(x);
            for k in 0..2 {
                let int_value = out[k] as f64 / f64::from(1u32 << ACT);
                assert!((int_value - f64::from(f32_out[k])).abs() < 2e-3, "row {i}, output {k}: {int_value} against {}", f32_out[k]);
            }
        }
    }
}
