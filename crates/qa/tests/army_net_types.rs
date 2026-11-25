//! Typed tokens and the third parties in the army net's input (step 1.5, gate T1 of the typed-token plan):
//! a net converted to the typed code (the new encoders copies of the unit's, every new output zero) plays as the old
//! net, also after a game the old net saved in the middle of a third-party scene is loaded: the game through the save
//! format, the memory through its sidecar (format 1, unchanged), the third parties' tokens and their relations rebuilt
//! from the saved diplomacy.

use aurelia_qa::army_net::model::Net;
use aurelia_qa::army_net::{observe, seat, Orders};
use aurelia_qa::bench::modules::{BenchRules, ClassicBench};
use aurelia_qa::trainers::families::{self, Fam};
use aurelia_qa::trainers::{lab, play};

type Tensors = Vec<(String, Vec<usize>, Vec<f32>)>;

const D: usize = 64;
const HEADS: usize = 4;
const LAYERS: usize = 3;

/// A fixed LCG in [-1, 1).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0
    }
}

/// Variant M's weights, random (`tests/army_net.rs` alike): a net whose memory moves its play, not a good one.
fn random_m(seed: u64) -> Tensors {
    let (ff, f_tok, f_act, promos, dmax) = (128usize, 48usize, 40usize, 400usize, 15usize);
    let mut shapes: Vec<(String, Vec<usize>, &str)> = Vec::new();
    let dense = |name: &str, i: usize, o: usize, v: &mut Vec<(String, Vec<usize>, &str)>| {
        v.push((format!("{name}.weight"), vec![i, o], "w"));
        v.push((format!("{name}.bias"), vec![o], "b"));
    };
    for (n, i, o) in [("e1", f_tok, D), ("e2", D, D), ("pointer", D, 1), ("end", D, 1), ("c1", f_act, D), ("c2", D, D), ("s1", 4 * D, D), ("s2", D, 1), ("v1", D, D), ("v2", D, 2), ("um", 16, D), ("pq", D, D), ("pk", D, D)] {
        dense(n, i, o, &mut shapes);
    }
    for l in 0..LAYERS {
        for (n, i, o) in [("q", D, D), ("k", D, D), ("v", D, D), ("o", D, D), ("f1", D, ff), ("f2", ff, D)] {
            dense(&format!("layers.{l}.{n}"), i, o, &mut shapes);
        }
        for n in ["norm1", "norm2"] {
            shapes.push((format!("layers.{l}.{n}.weight"), vec![D], "one"));
            shapes.push((format!("layers.{l}.{n}.bias"), vec![D], "b"));
        }
        shapes.push((format!("layers.{l}.dist_bias"), vec![HEADS, dmax + 1], "b"));
    }
    shapes.push(("norm.weight".into(), vec![D], "one"));
    shapes.push(("norm.bias".into(), vec![D], "b"));
    shapes.push(("promo".into(), vec![promos + 1, D], "w"));
    shapes.push(("plan0".into(), vec![4, D], "w"));
    shapes.push(("plan_bias".into(), vec![LAYERS, HEADS], "b"));
    for (n, i, o) in [("gu", D + f_tok, 16), ("gp", D + f_tok, D)] {
        shapes.push((format!("{n}_wx"), vec![i, 3 * o], "w"));
        shapes.push((format!("{n}_bx"), vec![3 * o], "b"));
        shapes.push((format!("{n}_wh"), vec![o, 3 * o], "w"));
        shapes.push((format!("{n}_bh"), vec![3 * o], "b"));
    }
    shapes.push(("hyst".into(), vec![4, 3], "hyst"));
    let mut rng = Lcg(seed);
    shapes
        .into_iter()
        .map(|(name, shape, kind)| {
            let n: usize = shape.iter().product();
            let scale = if name.starts_with("um.") || name.starts_with("gu_") { 4.0 } else { 1.0 } / (shape[0] as f32).sqrt();
            let v = (0..n)
                .map(|k| match kind {
                    "w" => rng.next() * scale,
                    "one" => 1.0,
                    "hyst" => [0.15, 20.0, 10.0][k % 3],
                    _ => rng.next() * 0.1,
                })
                .collect();
            (name, shape, v)
        })
        .collect()
}

/// The typed conversion of `base` (`army/init_mbt.py`): with `seed` None the start (copies, zeros, -10), else the new
/// parts drawn (the third path's output among them), so that the new input moves the play.
fn typed(base: &Tensors, seed: Option<u64>) -> Tensors {
    let get = |n: &str| base.iter().find(|t| t.0 == n).unwrap_or_else(|| panic!("no {n}")).clone();
    let mut out = base.clone();
    let mut rng = Lcg(seed.unwrap_or(0));
    let mut draw = |n: usize, s: f32| -> Vec<f32> { (0..n).map(|_| rng.next() * s).collect() };
    for (a, b) in [("ty_obj1", "ty_obj2"), ("ty_city1", "ty_city2"), ("tp_u1", "tp_u2"), ("tp_c1", "tp_c2")] {
        for (name, src) in [(a, "e1"), (b, "e2")] {
            let (_, ws, w) = get(&format!("{src}.weight"));
            let (_, bs, bv) = get(&format!("{src}.bias"));
            let (w, bv) = if seed.is_some() { (draw(w.len(), 1.0 / (ws[0] as f32).sqrt()), draw(bv.len(), 0.1)) } else { (w, bv) };
            out.push((format!("{name}.weight"), ws, w));
            out.push((format!("{name}.bias"), bs, bv));
        }
    }
    let x = observe::X_TOK;
    for (name, rows) in [("ty_emb", aurelia_qa::army_net::model::TYPES), ("ty_x", x), ("tp_x", x), ("cx", observe::X_ACT)] {
        let v = if seed.is_some() { draw(rows * D, 0.3) } else { vec![0.0; rows * D] };
        out.push((name.into(), vec![rows, D], v));
    }
    out.push(("tp_null".into(), vec![LAYERS, HEADS], vec![0.0; LAYERS * HEADS]));
    let dist: Vec<f32> = (0..LAYERS).flat_map(|l| get(&format!("layers.{l}.dist_bias")).2).collect();
    out.push(("tp_dist".into(), vec![LAYERS, HEADS, 16], dist));
    for l in 0..LAYERS {
        let w = if seed.is_some() { draw(D * D, 2.0 / (D as f32).sqrt()) } else { vec![0.0; D * D] };
        out.push((format!("tp_o.{l}.weight"), vec![D, D], w));
        out.push((format!("tp_o.{l}.bias"), vec![D], vec![0.0; D]));
    }
    out.push(("c_new".into(), vec![1], vec![-10.0]));
    out
}

/// The export format (`army/export.py`).
fn bytes(t: &Tensors) -> Vec<u8> {
    let mut data: Vec<f32> = Vec::new();
    let mut tensors = Vec::new();
    for (name, shape, v) in t {
        tensors.push(serde_json::json!({"name": name, "shape": shape, "offset": data.len()}));
        data.extend_from_slice(v);
    }
    let head = serde_json::to_vec(&serde_json::json!({"config": {"d": D, "heads": HEADS, "layers": LAYERS, "ff": 128, "f_tok": 48, "f_act": 40, "promos": 400, "dmax": 15}, "tensors": tensors})).expect("json");
    let mut out = Vec::new();
    out.extend_from_slice(&u32::from_le_bytes(*b"ARMW").to_le_bytes());
    out.extend_from_slice(&(head.len() as u32).to_le_bytes());
    out.extend_from_slice(&head);
    for x in data {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

fn leak(t: &Tensors) -> &'static Net {
    Box::leak(Box::new(Net::from_bytes(&bytes(t)).expect("the net loads")))
}

/// A game the OLD net saved in the middle of a third-party scene (the game and the memory's sidecar) is played on under
/// the typed code by the old net's CONVERSION exactly as the old net plays it without a save; the tokens after the load
/// (the main stream, the extras, the third parties with their relation) equal those before the save; a conversion whose
/// new parts are drawn plays on differently (the third tokens are live after the load).
#[test]
fn an_old_nets_save_plays_on_under_its_typed_conversion() {
    let rules = ClassicBench::rules();
    let era = lab::era_index(&rules, "classical").expect("the classical era");
    let base = random_m(7);
    let old = leak(&base);
    let conv = leak(&typed(&base, None));
    let drawn = leak(&typed(&base, Some(11)));
    assert!(!old.has_typed() && conv.has_typed() && drawn.has_typed());
    let dir = std::env::temp_dir().join(format!("army-net-types-save-{}", std::process::id()));
    let (mut played_on, mut moved) = (0, 0);
    // neutral-joins-war saved at round 4: after the neutral's declaration (round 3), its units are at war.
    for (fam, round) in [(Fam::NeutralPath, 3), (Fam::NeutralJoinsWar, 4), (Fam::CapitalRace, 3), (Fam::CampRace, 3), (Fam::ClosedBorders, 3)] {
        for instance in 0..2 {
            let (game, scene) = families::build(&rules, fam, era, instance, 0).expect("the scene builds");
            let orders = Orders::of_scene(&rules, &game, &scene);
            observe::reset_history();
            assert!(!observe::tokens(&game, &rules, &orders).third.is_empty(), "{fam:?} #{instance}: the third party is in the input");
            let run = |first: &'static Net, at: Option<seat::SaveAt>| {
                seat::set_thread_net(Some(first));
                seat::set_save_at(at);
                let row = play(&rules, game.clone(), scene.clone(), "army-net", true).expect("the scene plays");
                seat::set_save_at(None);
                seat::set_thread_net(None);
                (row.win, row.rounds, row.trace)
            };
            let at = |then: &'static Net, compare: bool| Some(seat::SaveAt { round, dir: dir.clone(), sidecar: true, then: Some(then), compare });
            let plain = run(old, None);
            assert_eq!(run(conv, None), plain, "{fam:?} #{instance}: the conversion plays as the old net");
            if plain.1 <= round {
                continue;
            }
            played_on += 1;
            assert_eq!(run(old, at(conv, false)), plain, "{fam:?} #{instance}: the old net's save played on by its conversion");
            let _ = seat::take_load_same();
            run(old, at(old, true));
            assert_eq!(seat::take_load_same(), Some(true), "{fam:?} #{instance}: the tokens after the load are those before the save");
            if run(old, at(drawn, false)) != plain {
                moved += 1;
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(played_on >= 6, "the plays last past the save in most scenes ({played_on})");
    assert!(moved > 0, "a drawn third path moves the play after the load");
}

/// The tokens of a third-party scene: the main stream is the old one (the objective, ours, the foe's units, cities:
/// the third nation is not in it), the third tokens carry their relation (an ally at war with our foe, a neutral at
/// peace; after its declaration the neutral of neutral-joins-war is at war), and the field stays within its budget.
#[test]
fn the_third_parties_are_tokens_of_their_own_with_their_relation() {
    let rules = ClassicBench::rules();
    let era = lab::era_index(&rules, "classical").expect("the classical era");
    let rel_of = |fam: Fam| -> Vec<usize> {
        let (game, scene) = families::build(&rules, fam, era, 0, 0).expect("the scene builds");
        let orders = Orders::of_scene(&rules, &game, &scene);
        observe::reset_history();
        let t = observe::tokens(&game, &rules, &orders);
        assert!(t.len() + t.third.len() <= observe::FIELD_BUDGET.max(t.len()));
        assert_eq!(t.xfeat.len(), t.len());
        assert_eq!(t.third.dist.len(), t.len() * t.third.len());
        t.third.x.iter().map(|x| x[..observe::REL].iter().position(|v| *v > 0.5).expect("a relation")).collect()
    };
    let ally = rel_of(Fam::AllySiege);
    assert!(!ally.is_empty() && ally.iter().all(|r| *r == usize::from(observe::R_ALLY)), "ally-siege: the ally {ally:?}");
    let neutral = rel_of(Fam::NeutralPath);
    assert!(!neutral.is_empty() && neutral.iter().all(|r| *r == usize::from(observe::R_PEACE)), "neutral-path: at peace {neutral:?}");
    // The camp's neutral is at war with the barbarians, as every nation: no ally for that.
    let camp = rel_of(Fam::CampRace);
    assert!(!camp.is_empty() && camp.iter().all(|r| *r == usize::from(observe::R_PEACE)), "camp-race: the neutral at peace {camp:?}");
}
