//! The unit kind a city orders by its purpose (`ai::army_fit`, classic/docs/ai-army.md):
//! the rules' combat bonuses decide it, read from the core rules' data.

use classic_content::core::core_rules;
use classic_rules::ai::army_fit::{pick, Foes, Purpose};
use classic_rules::ids::UnitKindId;
use classic_rules::{compile_rules, CompiledRuleset};

fn kind(rules: &CompiledRuleset, name: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.name == name).expect(name) as u16)
}

/// The rules' bonus against mounted units makes the anti-mounted kind the defender against
/// cavalry; against infantry the stronger kind holds; a mounted kind's penalty against
/// cities keeps it from taking them, and a melee kind is no shooter.
#[test]
fn the_rules_bonuses_decide_the_kind() {
    let rules = compile_rules(core_rules()).unwrap();
    let (spear, sword, horse, knight, pike, crossbow) =
        (kind(&rules, "Spearman"), kind(&rules, "Swordsman"), kind(&rules, "Horseman"), kind(&rules, "Knight"), kind(&rules, "Pikeman"), kind(&rules, "Crossbowman"));
    let cavalry = Foes { kinds: vec![(horse, 3)] };
    assert_eq!(pick(&rules, &[spear, sword], Purpose::Hold, &cavalry, None, None, 0), Some(spear), "+100 % against mounted");
    let infantry = Foes { kinds: vec![(sword, 3)] };
    assert_eq!(pick(&rules, &[spear, sword], Purpose::Hold, &infantry, None, None, 0), Some(sword));
    assert_eq!(pick(&rules, &[knight, pike], Purpose::Take, &infantry, Some(2000), None, 0), Some(pike), "−33 % against cities");
    assert_eq!(pick(&rules, &[knight, pike], Purpose::Strike, &infantry, Some(2000), None, 0), None);
    assert_eq!(pick(&rules, &[knight, pike, crossbow], Purpose::Strike, &infantry, Some(2000), None, 0), Some(crossbow));
}
