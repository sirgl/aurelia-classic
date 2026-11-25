//! The army net's DAgger probe (`aurelia_qa::army_net::seat`, the army-net design note): when the net
//! plays exactly as the expert, the probe's labels are the expert's own. The staged-assault expert keeps its
//! commitments between turns (`Scene::track`: engaged, waited, resting); a probe that loses them answers every
//! turn as its first ("wait for the stage"), which is what DAgger round 1 recorded before the fix.

use aurelia_qa::army_net::{parity, record};
use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::trainers::families::{self, Fam};
use aurelia_qa::trainers::{lab, play};

/// (round, pointer, action, candidates) of every unit decision of a recorded play.
fn labels(rules: &classic_rules::CompiledRuleset, fam: Fam, era: u8, instance: usize, player: &str) -> Vec<(u16, i16, i16, usize)> {
    let (game, scene) = families::build(rules, fam, era, instance, 0).expect("the scene builds");
    record::want(true);
    let row = play(rules, game, scene, player, false).expect("the scene plays");
    record::want(false);
    let (bytes, _) = record::take_last().expect("the play was recorded");
    assert!(row.rounds >= 2, "{player}: the play lasts beyond its first turn");
    parity::read(&bytes, usize::MAX).expect("the record reads").iter().filter(|d| d.pointer >= 0).map(|d| (d.round, d.pointer, d.action, d.candidates.len())).collect()
}

#[test]
fn a_probe_of_the_staged_assault_labels_what_the_reference_plays() {
    let rules = ClassicBench::rules();
    let era = lab::era_index(&rules, "classical").expect("the classical era");
    for expert in ["ref-planned", "ref-fire"] {
        let fam = if expert == "ref-planned" { Fam::PlannedAssault } else { Fam::CityFire };
        let taught = labels(&rules, fam, era, 1, expert);
        let probed = labels(&rules, fam, era, 1, &format!("probe:{expert}"));
        let rounds = taught.iter().map(|d| d.0).max().expect("decisions");
        assert!(rounds >= 2, "{expert}: the reference acts after its second turn");
        for r in 0..=rounds {
            let t: Vec<_> = taught.iter().filter(|d| d.0 == r).collect();
            let p: Vec<_> = probed.iter().filter(|d| d.0 == r).collect();
            // The turn of the reference comes first; the seat's mind (`standard`) may add a command or two after
            // it (a set-up, a fortify), which is not the reference's and not the probe's.
            assert!(!p.is_empty() || t.is_empty(), "{expert}: round {r} has a probe");
            assert_eq!(p[..], t[..p.len().min(t.len())], "{expert}: round {r}: the probe labels what the reference itself plays");
        }
        assert!(probed.len() * 4 >= taught.len() * 3, "{expert}: the probe holds {} of the {} decisions (the rest are the seat mind's)", probed.len(), taught.len());
    }
}

/// Variant M's weights, random (a fixed LCG), in the export format (`army/export.py`): the test needs a net whose
/// memory moves its play, not a good one.
fn random_m_weights(seed: u64) -> Vec<u8> {
    let (d, heads, layers, ff, f_tok, f_act, promos, dmax) = (64usize, 4usize, 3usize, 128usize, 48usize, 40usize, 400usize, 15usize);
    let mut shapes: Vec<(String, Vec<usize>, &str)> = Vec::new();
    let dense = |name: &str, i: usize, o: usize, v: &mut Vec<(String, Vec<usize>, &str)>| {
        v.push((format!("{name}.weight"), vec![i, o], "w"));
        v.push((format!("{name}.bias"), vec![o], "b"));
    };
    for (n, i, o) in [("e1", f_tok, d), ("e2", d, d), ("pointer", d, 1), ("end", d, 1), ("c1", f_act, d), ("c2", d, d), ("s1", 4 * d, d), ("s2", d, 1), ("v1", d, d), ("v2", d, 2), ("um", 16, d), ("pq", d, d), ("pk", d, d)] {
        dense(n, i, o, &mut shapes);
    }
    for l in 0..layers {
        for (n, i, o) in [("q", d, d), ("k", d, d), ("v", d, d), ("o", d, d), ("f1", d, ff), ("f2", ff, d)] {
            dense(&format!("layers.{l}.{n}"), i, o, &mut shapes);
        }
        for n in ["norm1", "norm2"] {
            shapes.push((format!("layers.{l}.{n}.weight"), vec![d], "one"));
            shapes.push((format!("layers.{l}.{n}.bias"), vec![d], "b"));
        }
        shapes.push((format!("layers.{l}.dist_bias"), vec![heads, dmax + 1], "b"));
    }
    shapes.push(("norm.weight".into(), vec![d], "one"));
    shapes.push(("norm.bias".into(), vec![d], "b"));
    shapes.push(("promo".into(), vec![promos + 1, d], "w"));
    shapes.push(("plan0".into(), vec![4, d], "w"));
    shapes.push(("plan_bias".into(), vec![layers, heads], "b"));
    for (n, i, o) in [("gu", d + f_tok, 16), ("gp", d + f_tok, d)] {
        shapes.push((format!("{n}_wx"), vec![i, 3 * o], "w"));
        shapes.push((format!("{n}_bx"), vec![3 * o], "b"));
        shapes.push((format!("{n}_wh"), vec![o, 3 * o], "w"));
        shapes.push((format!("{n}_bh"), vec![3 * o], "b"));
    }
    shapes.push(("hyst".into(), vec![4, 3], "hyst"));
    let mut state = seed;
    let mut next = || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((state >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0
    };
    let mut data: Vec<f32> = Vec::new();
    let mut tensors = Vec::new();
    for (name, shape, kind) in &shapes {
        let n: usize = shape.iter().product();
        tensors.push(serde_json::json!({"name": name, "shape": shape, "offset": data.len()}));
        // The memory's way into the tokens strong, so that it moves the play.
        let scale = if name.starts_with("um.") || name.starts_with("gu_") { 4.0 } else { 1.0 } / (shape[0] as f32).sqrt();
        for k in 0..n {
            data.push(match *kind {
                "w" => next() * scale,
                "one" => 1.0,
                "hyst" => [0.15, 20.0, 10.0][k % 3],
                _ => next() * 0.1,
            });
        }
    }
    let head = serde_json::to_vec(&serde_json::json!({"config": {"d": d, "heads": heads, "layers": layers, "ff": ff, "f_tok": f_tok, "f_act": f_act, "promos": promos, "dmax": dmax}, "tensors": tensors})).expect("json");
    let mut out = Vec::new();
    out.extend_from_slice(&u32::from_le_bytes(*b"ARMW").to_le_bytes());
    out.extend_from_slice(&(head.len() as u32).to_le_bytes());
    out.extend_from_slice(&head);
    for x in data {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

/// Variant M's memory survives a save and load in the middle of a siege: the game through the save format, the
/// memory through its sidecar beside the save (keyed by the game's id). The play is then the same as without the
/// save; without the sidecar it is not (the memory moves the play), and the reference's play does not change.
#[test]
fn the_army_nets_memory_survives_a_save_and_load_mid_siege() {
    use aurelia_qa::army_net::{model::Net, seat};
    let rules = ClassicBench::rules();
    let era = lab::era_index(&rules, "classical").expect("the classical era");
    let net = Net::from_bytes(&random_m_weights(7)).expect("the random M net loads");
    assert!(net.has_memory());
    seat::install_net(net);
    let dir = std::env::temp_dir().join(format!("army-net-save-{}", std::process::id()));
    let mut differs_without = 0;
    for instance in 0..4 {
        let (game, scene) = families::build(&rules, Fam::PlannedAssault, era, instance, 0).expect("the scene builds");
        let run = |player: &str, at: Option<seat::SaveAt>| {
            seat::set_save_at(at);
            let row = play(&rules, game.clone(), scene.clone(), player, true).expect("the scene plays");
            seat::set_save_at(None);
            (row.win, row.rounds, row.trace)
        };
        let at = |sidecar| Some(seat::SaveAt { round: 3, dir: dir.clone(), sidecar, ..Default::default() });
        assert_eq!(run("ref-planned", at(true)), run("ref-planned", None), "#{instance}: the save format keeps the scene (the reference plays the same)");
        let plain = run("army-net", None);
        assert!(plain.1 > 3, "#{instance}: the play lasts past the save");
        assert_eq!(run("army-net", at(true)), plain, "#{instance}: with the sidecar the net plays as without the save");
        if run("army-net", at(false)) != plain {
            differs_without += 1;
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(differs_without > 0, "without the sidecar the memory starts fresh and the play changes (in some scene)");
}

/// The stake of a trainer scene (M-HANDOFF.md, addition 8): a careful family is worth its loss cap and pressed by its
/// round limit; an "at any cost" family is worth the whole army and pressed by its deadline. The careful pressure was
/// 0 before (no deadline), which the addition does not say.
#[test]
fn a_scenes_price_is_its_loss_cap_or_its_army_and_one_over_the_round_that_matters() {
    use aurelia_qa::army_net::Orders;
    let rules = ClassicBench::rules();
    let era = lab::era_index(&rules, "classical").expect("the classical era");
    let (game, scene) = families::build(&rules, Fam::PlannedAssault, era, 1, 0).expect("the scene builds");
    assert!(scene.deadline.is_none());
    let p = Orders::of_scene(&rules, &game, &scene).price;
    assert_eq!(p, [1.0 / 8.0, 1.0 / scene.rounds as f32, 1.0], "planned-assault: worth one unit, pressed by the round limit {}", scene.rounds);
    let (game, scene) = families::build(&rules, Fam::AssaultPeace, era, 1, 0).expect("the scene builds");
    let d = scene.deadline.expect("the peace has a deadline");
    let p = Orders::of_scene(&rules, &game, &scene).price;
    assert_eq!(p, [scene.mine.len() as f32 / 8.0, 1.0 / d as f32, 1.0], "assault-peace: worth the army, pressed by the deadline {d}");
}

/// The stake set from outside (`ARMY_PRICE`, the paired data teach7p and P3): "careful" is the loss cap and the round
/// limit (one unit where the rule sets no cap), "any cost" the whole army and the deadline given (else the scene's own).
/// A careful scene's own price equals its "careful" form, so the careful half of the paired data is the scene's play.
#[test]
fn a_price_set_from_outside_is_careful_or_any_cost_per_scene() {
    use aurelia_qa::army_net::{price_of, price_set};
    let r = 1.0 / 30.0;
    assert_eq!(price_set("careful", "planned-assault", 9, 30, None), Some([1.0 / 8.0, r, 1.0]));
    assert_eq!(price_set("careful", "planned-assault", 9, 30, None), Some(price_of("planned-assault", 9, Some(30))), "careful = the scene's own");
    assert_eq!(price_set("careful", "assault-heal", 8, 30, None), Some([4.0 / 8.0, r, 1.0]), "half the army");
    assert_eq!(price_set("careful", "siege", 7, 20, None), Some([1.0 / 8.0, 1.0 / 20.0, 1.0]), "no cap: one unit");
    assert_eq!(price_set("anycost:7", "planned-assault", 9, 30, None), Some([9.0 / 8.0, 1.0 / 7.0, 1.0]));
    assert_eq!(price_set("anycost", "assault-peace", 10, 8, Some(8)), Some(price_of("assault-peace", 10, Some(8))), "any cost = an any-cost scene's own");
    assert_eq!(price_set("anycost", "planned-assault", 9, 30, None), Some([9.0 / 8.0, r, 1.0]), "no deadline: the limit");
    assert_eq!(price_set("0.125,0.0833,1", "duel", 3, 12, None), Some([0.125, 0.0833, 1.0]));
    assert_eq!(price_set("anycost:x", "duel", 3, 12, None), None);
    assert_eq!(price_set("anycostly", "duel", 3, 12, None), None);
    assert_eq!(price_set("cheap", "duel", 3, 12, None), None);
}

/// The export format as tensors: (config, [(name, shape, data)]).
fn read_weights(bytes: &[u8]) -> (serde_json::Value, Vec<(String, Vec<usize>, Vec<f32>)>) {
    let len = u32::from_le_bytes(bytes[4..8].try_into().expect("len")) as usize;
    let head: serde_json::Value = serde_json::from_slice(&bytes[8..8 + len]).expect("json");
    let floats: Vec<f32> = bytes[8 + len..].chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
    let tensors = head["tensors"]
        .as_array()
        .expect("tensors")
        .iter()
        .map(|e| {
            let shape: Vec<usize> = e["shape"].as_array().expect("shape").iter().map(|v| v.as_u64().expect("dim") as usize).collect();
            let at = e["offset"].as_u64().expect("offset") as usize;
            (e["name"].as_str().expect("name").to_owned(), shape.clone(), floats[at..at + shape.iter().product::<usize>()].to_vec())
        })
        .collect();
    (head["config"].clone(), tensors)
}

fn write_weights(config: &serde_json::Value, tensors: &[(String, Vec<usize>, Vec<f32>)]) -> Vec<u8> {
    let mut data: Vec<f32> = Vec::new();
    let mut entries = Vec::new();
    for (name, shape, v) in tensors {
        entries.push(serde_json::json!({"name": name, "shape": shape, "offset": data.len()}));
        data.extend_from_slice(v);
    }
    let head = serde_json::to_vec(&serde_json::json!({"config": config, "tensors": entries})).expect("json");
    let mut out = Vec::new();
    out.extend_from_slice(&u32::from_le_bytes(*b"ARMW").to_le_bytes());
    out.extend_from_slice(&(head.len() as u32).to_le_bytes());
    out.extend_from_slice(&head);
    for x in data {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

/// The base weights plus one modern layer at the end (kinds "cccm"): random, except `o` and `w2` which are zero when `zero`.
fn with_modern_layer(base: &[u8], zero: bool) -> Vec<u8> {
    let (mut config, mut tensors) = read_weights(base);
    let d = config["d"].as_u64().expect("d") as usize;
    let heads = config["heads"].as_u64().expect("heads") as usize;
    let dmax = config["dmax"].as_u64().expect("dmax") as usize;
    let l = config["layers"].as_u64().expect("layers") as usize;
    let (dh, ff) = (d / heads, 86usize);
    config["layers"] = serde_json::json!(l + 1);
    config["kinds"] = serde_json::json!(format!("{}m", "c".repeat(l)));
    for (name, shape, v) in tensors.iter_mut() {
        if name == "plan_bias" {
            shape[0] += 1;
            v.extend((0..heads).map(|h| 0.05 * h as f32 - 0.1));
        }
    }
    let mut state = 99u64;
    let mut next = move |n: usize, scale: f32, shift: f32| -> Vec<f32> {
        (0..n)
            .map(|_| {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                shift + ((state >> 33) as f32 / (1u64 << 31) as f32 * 2.0 - 1.0) * scale
            })
            .collect()
    };
    let p = format!("layers.{l}");
    let mut add = |n: &str, shape: Vec<usize>, v: Vec<f32>| tensors.push((format!("{p}.{n}"), shape, v));
    add("rms1.weight", vec![d], next(d, 0.1, 1.0));
    for n in ["q", "k", "v", "g", "o"] {
        let w = if n == "o" && zero { vec![0.0; d * d] } else { next(d * d, 0.2, 0.0) };
        add(&format!("{n}.weight"), vec![d, d], w);
        add(&format!("{n}.bias"), vec![d], if n == "o" && zero { vec![0.0; d] } else { next(d, 0.1, 0.0) });
    }
    add("qn.weight", vec![dh], next(dh, 0.1, 1.0));
    add("kn.weight", vec![dh], next(dh, 0.1, 1.0));
    add("dist_bias", vec![heads, dmax + 1], next(heads * (dmax + 1), 0.1, 0.0));
    add("rms2.weight", vec![d], next(d, 0.1, 1.0));
    add("w1.weight", vec![d, ff], next(d * ff, 0.2, 0.0));
    add("w3.weight", vec![d, ff], next(d * ff, 0.2, 0.0));
    add("w2.weight", vec![ff, d], if zero { vec![0.0; ff * d] } else { next(ff * d, 0.2, 0.0) });
    add("w2.bias", vec![d], if zero { vec![0.0; d] } else { next(d, 0.1, 0.0) });
    write_weights(&config, &tensors)
}

/// A modern layer with zero `o` and `w2` adds nothing: the net's body is the base net's, bit for bit; with them
/// random it moves.
#[test]
fn a_modern_layer_with_zero_outputs_is_the_identity_of_the_base_forward() {
    use aurelia_qa::army_net::model::{MemIn, Net};
    let rules = ClassicBench::rules();
    let era = lab::era_index(&rules, "classical").expect("the classical era");
    let (game, scene) = families::build(&rules, Fam::PlannedAssault, era, 1, 0).expect("the scene builds");
    record::want(true);
    play(&rules, game, scene, "ref-planned", false).expect("the scene plays");
    record::want(false);
    let (bytes, _) = record::take_last().expect("the play was recorded");
    let decisions = parity::read(&bytes, 12).expect("the record reads");
    assert!(!decisions.is_empty());

    let base = random_m_weights(11);
    let plain = Net::from_bytes(&base).expect("base loads");
    let identity = Net::from_bytes(&with_modern_layer(&base, true)).expect("the zero modern layer loads");
    let moved = Net::from_bytes(&with_modern_layer(&base, false)).expect("the random modern layer loads");
    let state = plain.initial_memory().expect("memory");
    let mut moves = 0.0f32;
    for dec in &decisions {
        let mi = MemIn { slots: &dec.slots, state: &state };
        let a = plain.body(&dec.tokens, None, Some(&mi));
        let b = identity.body(&dec.tokens, None, Some(&mi));
        let c = moved.body(&dec.tokens, None, Some(&mi));
        assert_eq!(a.h, b.h, "zero o and w2: the modern layer is the identity");
        moves = moves.max(a.h.iter().zip(&c.h).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max));
    }
    assert!(moves > 1e-3, "random o and w2 move the body ({moves})");
}
