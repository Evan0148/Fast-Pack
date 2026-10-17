use super::*;

fn props(body: &str) -> Properties {
    super::super::properties::parse(body.as_bytes()).unwrap()
}

fn matches(node: &Value, enchantment: &str, level: u32, damage: f64) -> bool {
    match node["type"].as_str().unwrap() {
        "minecraft:model" => node["model"] == "yes",
        "minecraft:condition" => {
            let value = &node["value"][0];
            let levels = &value["levels"];
            let within = if let Some(exact) = levels.as_u64() {
                u64::from(level) == exact
            } else {
                u64::from(level) >= levels["min"].as_u64().unwrap_or(0)
                    && levels["max"]
                        .as_u64()
                        .is_none_or(|max| u64::from(level) <= max)
            };
            let id = value["enchantments"].as_str() == Some(enchantment);
            matches(
                &node[if within && id { "on_true" } else { "on_false" }],
                enchantment,
                level,
                damage,
            )
        }
        "minecraft:range_dispatch" => {
            let entry = node["entries"]
                .as_array()
                .unwrap()
                .iter()
                .rev()
                .find(|entry| entry["threshold"].as_f64().unwrap() <= damage);
            matches(
                entry.map_or(&node["fallback"], |entry| &entry["model"]),
                enchantment,
                level,
                damage,
            )
        }
        other => panic!("unexpected test node {other}"),
    }
}

#[test]
fn enchantment_intervals_damage_and_disjoint_levels_match_the_original_conditions() {
    let original = props(
        "items=diamond_sword\nenchantments=sharpness\nenchantmentLevels=2-4 7\ndamage=0-25%\ntexture=x\n",
    );
    let tests = compile(&original).unwrap();
    let tree = wrap_all(
        &tests,
        json!({"type":"minecraft:model","model":"yes"}),
        &json!({"type":"minecraft:model","model":"no"}),
    );
    for level in 0..10 {
        for damage in [0.0, 0.1, 0.25, 0.259, 0.26, 0.9] {
            for enchantment in ["minecraft:sharpness", "minecraft:unbreaking"] {
                let expected = enchantment == "minecraft:sharpness"
                    && ((2..=4).contains(&level) || level == 7)
                    && damage < 0.26;
                assert_eq!(
                    matches(&tree, enchantment, level, damage),
                    expected,
                    "{enchantment}/{level}/{damage}"
                );
            }
        }
    }
    assert_eq!(
        integer_ranges("1-4 3-7 8 12-").unwrap(),
        vec![(1, Some(8)), (12, None)]
    );
}

#[test]
fn ambiguous_total_levels_unknown_nbt_and_masks_never_become_unconditional_models() {
    for body in [
        "enchantmentLevels=8",
        "nbt.display.Name=regex:.*Sword.*",
        "damage=3\ndamageMask=15",
        "enchantments=sharpness\nhand=off",
        "damage=4294967295",
    ] {
        assert!(compile(&props(body)).is_err(), "{body}");
    }
    let tests=compile(&props("items=firework_rocket\nnbt.Fireworks.Flight=2\nnbt.Fireworks.Explosions.0.Type=regex:[0-9]+\n")).unwrap();
    let value = tests[0].wrap(json!("yes"), json!("no"));
    assert_eq!(value["predicate"], "minecraft:fireworks");
    assert_eq!(
        value["value"],
        json!({"flight_duration":2,"explosions":{"contains":[{}]}})
    );
}
