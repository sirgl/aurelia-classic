//! The army net's DAgger probe (`aurelia_qa::army_net::seat`, work/nets/army-net/DESIGN.md): when the net
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
