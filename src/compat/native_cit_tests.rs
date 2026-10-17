use super::*;
use std::fs as disk;

fn json_file(root: &Path, path: &str, value: &Value) {
    let path = root.join(path);
    disk::create_dir_all(path.parent().unwrap()).unwrap();
    disk::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}
fn definition() -> Value {
    json!({"hand_animation_on_swap":false,"model":{
            "type":"minecraft:composite","models":[{"type":"minecraft:model","model":"minecraft:item/diamond_sword"},{
                "type":"minecraft:condition","property":"minecraft:component","predicate":"minecraft:enchantments","value":[{"enchantments":"minecraft:sharpness","levels":{"min":3}}],
                "on_true":{"type":"minecraft:model","model":"another_namespace:item/number"},"on_false":{"type":"minecraft:empty"}
            }]
    }})
}

fn scoped_definition() -> Value {
    json!({"model":{"type":"minecraft:select","property":"minecraft:display_context","cases":[{"when":"gui","model":definition()["model"]}],"fallback":{"type":"minecraft:model","model":"minecraft:item/diamond_sword"}}})
}
fn fixture(root: &Path) -> Value {
    let definition = definition();
    json_file(
        root,
        "assets/minecraft/items/diamond_sword.json",
        &definition,
    );
    json_file(
        root,
        "assets/another_namespace/models/item/number.json",
        &json!({"parent":"minecraft:item/generated","textures":{"layer0":"another_namespace:item/number"}}),
    );
    let path = root.join("assets/another_namespace/textures/item/number.png");
    disk::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbaImage::from_pixel(16, 16, image::Rgba([201, 31, 41, 150]))
        .save(path)
        .unwrap();
    definition
}
fn down(root: &Path) -> AssetReport {
    prepare(root, 69, 15).unwrap();
    let mut report = AssetReport::default();
    migrate(root, 15, "1.20.1", &mut report).unwrap();
    finalize(root).unwrap();
    report
}

#[test]
fn native_enchantment_composites_produce_real_optifine_rules_and_restore_exactly() {
    let work = tempfile::tempdir().unwrap();
    let root = work.path();
    let original = fixture(root);
    let report = down(root);
    assert!(!report.converted.is_empty());
    assert!(
        !root
            .join("assets/minecraft/items/diamond_sword.json")
            .exists()
    );
    let rules = generated_rules(root).unwrap();
    assert_eq!(rules.len(), 2);
    let rule = rules
        .iter()
        .map(|path| properties::parse(&disk::read(path).unwrap()).unwrap())
        .find(|rule| rule.contains_key("enchantments"))
        .unwrap();
    assert_eq!(rule["enchantments"], "minecraft:sharpness");
    assert_eq!(rule["enchantmentLevels"], "3-");
    let model = root.join(format!(
        "assets/minecraft/models/{}.json",
        rule["model"].strip_prefix("minecraft:").unwrap()
    ));
    let model: Value = serde_json::from_slice(&disk::read(model).unwrap()).unwrap();
    assert_eq!(model["textures"]["layer1"], "another_namespace:item/number");
    prepare(root, 15, 69).unwrap();
    let restored: Value = serde_json::from_slice(
        &disk::read(root.join("assets/minecraft/items/diamond_sword.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(restored, original);
    assert!(rules.iter().all(|path| !path.exists()));
    // Other compatibility adapters must not insert unrelated mappings into an exact restoration.
    json_file(
        root,
        "assets/minecraft/items/diamond_sword.json",
        &json!({"model":{"type":"minecraft:empty"}}),
    );
    preserve_restored_definitions(root, 69).unwrap();
    assert_eq!(
        crate::model_json::parse(
            &disk::read(root.join("assets/minecraft/items/diamond_sword.json")).unwrap()
        )
        .unwrap(),
        original
    );
    let edit = json!({"model":{"type":"minecraft:empty"}});
    json_file(root, "assets/minecraft/items/diamond_sword.json", &edit);
    preserve_restored_definitions(root, 69).unwrap();
    assert_eq!(
        crate::model_json::parse(
            &disk::read(root.join("assets/minecraft/items/diamond_sword.json")).unwrap()
        )
        .unwrap(),
        edit
    );
}

#[test]
fn edited_or_deleted_generated_rules_are_not_overwritten_and_older_native_versions_keep_cit() {
    for deleted in [false, true] {
        let work = tempfile::tempdir().unwrap();
        let root = work.path();
        fixture(root);
        down(root);
        let path = generated_rules(root).unwrap().pop_first().unwrap();
        if deleted {
            disk::remove_file(&path).unwrap();
        } else {
            disk::write(&path, b"items=stick\ntexture=mine\n").unwrap();
        }
        let notes = prepare(root, 15, 69).unwrap();
        assert!(!notes.is_empty());
        assert!(
            !root
                .join("assets/minecraft/items/diamond_sword.json")
                .exists()
        );
    }
    let work = tempfile::tempdir().unwrap();
    let root = work.path();
    fixture(root);
    down(root);
    let notes = prepare(root, 15, 46).unwrap();
    assert!(!notes.is_empty());
    assert!(!generated_rules(root).unwrap().is_empty());
    assert!(
        !root
            .join("assets/minecraft/items/diamond_sword.json")
            .exists()
    );
}

#[test]
fn invalid_provenance_paths_unknown_nodes_and_unsafe_conjunctions_are_detected() {
    let work = tempfile::tempdir().unwrap();
    let root = work.path();
    json_file(
        root,
        CACHE,
        &json!({"ready":true,"definitions":{"../outside.json":{"model":{}}}}),
    );
    assert!(prepare(root, 15, 69).is_err());
    assert!(!root.parent().unwrap().join("outside.json").exists());
    assert!(tree::compile(&json!({"type":"minecraft:condition","property":"modded:custom_property","on_true":{"type":"minecraft:empty"},"on_false":{"type":"minecraft:empty"}})).is_err());
    assert!(
        tree::properties(&[
            tree::Atom::Trim("minecraft:gold".into()),
            tree::Atom::Trim("minecraft:diamond".into())
        ])
        .is_err()
    );
    let value = json!([{"enchantments":"minecraft:power","levels":{"min":2}}]);
    assert!(
        tree::properties(&[
            tree::Atom::Component("minecraft:enchantments".into(), value.clone()),
            tree::Atom::Component(
                "minecraft:enchantments".into(),
                json!([{"enchantments":"minecraft:flame"}])
            )
        ])
        .is_err()
    );
}

#[test]
fn damage_boundaries_and_runtime_bow_states_use_their_own_version_semantics() {
    let model = |reference: &str| json!({"type":"minecraft:model","model":reference});
    let tree = json!({"type":"minecraft:condition","property":"minecraft:using_item",
        "on_false":model("minecraft:item/bow"),"on_true":{"type":"minecraft:range_dispatch","property":"minecraft:use_duration","scale":0.05,
        "entries":[{"threshold":0.65,"model":model("minecraft:item/bow_pulling_1")},{"threshold":0.9,"model":model("minecraft:item/bow_pulling_2")}],"fallback":model("minecraft:item/bow_pulling_0")}});
    let cases = tree::compile(&tree).unwrap();
    let thresholds: Vec<_> = cases
        .iter()
        .filter_map(|case| case.runtime.get("pull"))
        .copied()
        .collect();
    assert_eq!(thresholds, vec![0.9, 0.65, 0.0]);
    let tree = json!({"type":"minecraft:range_dispatch","property":"minecraft:damage","normalize":false,"entries":[{"threshold":5,"model":model("minecraft:item/bow")}],"fallback":{"type":"minecraft:empty"}});
    let cases = tree::compile(&tree).unwrap();
    assert_eq!(tree::properties(&cases[0].tests).unwrap()["damage"], "5-");
    assert_eq!(tree::properties(&cases[1].tests).unwrap()["damage"], "0-4");
}

#[test]
fn missing_textures_and_unsupported_tints_never_create_missing_texture_rules() {
    let work = tempfile::tempdir().unwrap();
    let root = work.path();
    fixture(root);
    disk::remove_file(root.join("assets/another_namespace/textures/item/number.png")).unwrap();
    let report = down(root);
    assert!(report.warnings.iter().any(|text| text.contains("缺少贴图")));
    let rules = generated_rules(root).unwrap();
    assert_eq!(rules.len(), 1); // The valid base appearance remains available.
    assert!(
        !properties::parse(&disk::read(rules.first().unwrap()).unwrap())
            .unwrap()
            .contains_key("enchantments")
    );

    let leaf = json!({"type":"minecraft:model","model":"minecraft:block/stone","tints":[{"type":"modded:unsupported"}]});
    assert!(models::compose(root, 15, "minecraft:stone", &[leaf], &mut BTreeMap::new()).is_err());
    let armor = models::compose(
        root,
        15,
        "minecraft:diamond_boots",
        &[json!({"type":"minecraft:model","model":"minecraft:item/diamond_boots_diamond_trim"})],
        &mut BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(
        armor["textures"]["layer1"],
        "minecraft:trims/items/boots_trim_diamond_darker"
    );
}

#[test]
fn protocol_carriers_only_capture_the_authored_conditional_fallback() {
    let work = tempfile::tempdir().unwrap();
    let root = work.path();
    for (name, base) in [
        ("diamond_sword", definition()),
        (
            "stone",
            json!({"model":{"type":"minecraft:model","model":"minecraft:block/stone"}}),
        ),
    ] {
        json_file(
            root,
            &format!("assets/minecraft/items/{name}.json"),
            &json!({"fastpack_via_bridge":1,"model":{"type":"minecraft:select","property":"minecraft:custom_model_data","cases":[],"fallback":base["model"]}}),
        );
        json_file(
            root,
            &format!("fastpack/via-bridge/definitions/{name}.json"),
            &base,
        );
    }
    prepare(root, 69, 15).unwrap();
    let state: State = serde_json::from_slice(&disk::read(root.join(CACHE)).unwrap()).unwrap();
    assert_eq!(state.definitions.len(), 1);
    assert_eq!(
        state.definitions["assets/minecraft/items/diamond_sword.json"],
        definition()
    );
}

#[test]
fn future_or_non_equivalent_predicate_fields_are_preserved_instead_of_ignored() {
    let leaf = json!({"type":"minecraft:empty"});
    for (key, value) in [("index", json!(1)), ("remaining", json!(true))] {
        let mut node = json!({"type":"minecraft:range_dispatch","property":"minecraft:use_duration","entries":[],"fallback":leaf});
        node[key] = value;
        assert!(tree::compile(&node).is_err());
    }
    for value in [
        json!([{"enchantments":"#minecraft:damage"}]),
        json!([{"enchantments":"minecraft:sharpness","levels":{"min":-1}}]),
        json!([{"enchantments":"minecraft:sharpness","future":true}]),
    ] {
        assert!(
            tree::properties(&[tree::Atom::Component(
                "minecraft:enchantments".into(),
                value
            )])
            .is_err()
        );
    }
}

#[test]
fn gui_only_overlays_never_enter_legacy_hand_ground_or_frame_models() {
    let work = tempfile::tempdir().unwrap();
    let root = work.path();
    fixture(root);
    let original = scoped_definition();
    json_file(root, "assets/minecraft/items/diamond_sword.json", &original);
    let report = down(root);
    assert!(
        report
            .warnings
            .iter()
            .any(|text| text.contains("背包专用数字或标记未应用"))
    );
    assert!(generated_rules(root).unwrap().is_empty());
    assert!(tree::compile(&original["model"]).is_err());
    let (node, removed) = tree::legacy_contexts(&original["model"]).unwrap();
    assert_eq!(removed, 1);
    assert_eq!(node["model"], "minecraft:item/diamond_sword");
    prepare(root, 15, 69).unwrap();
    assert_eq!(
        crate::model_json::parse(
            &disk::read(root.join("assets/minecraft/items/diamond_sword.json")).unwrap()
        )
        .unwrap(),
        original
    );
}

#[test]
fn old_generated_gui_models_are_repaired_but_author_edits_are_preserved() {
    for edited in [false, true] {
        let work = tempfile::tempdir().unwrap();
        let root = work.path();
        let original = fixture(root);
        down(root);
        let path = generated_rules(root).unwrap().pop_first().unwrap();
        let mut state: State =
            serde_json::from_slice(&disk::read(root.join(CACHE)).unwrap()).unwrap();
        state.generator = 0;
        state.definitions.insert(
            "assets/minecraft/items/diamond_sword.json".into(),
            scoped_definition(),
        );
        disk::write(root.join(CACHE), serde_json::to_vec(&state).unwrap()).unwrap();
        if edited {
            disk::write(&path, b"items=diamond_sword\ntexture=mine\n").unwrap();
        }
        let mut report = AssetReport::default();
        migrate(root, 15, "1.20.1", &mut report).unwrap();
        if edited {
            assert!(path.exists());
            assert!(
                report
                    .warnings
                    .iter()
                    .any(|text| text.contains("保留用户改动"))
            );
        } else {
            assert!(generated_rules(root).unwrap().is_empty());
            assert!(!path.exists());
        }
        assert_ne!(original, scoped_definition());
    }
}

#[test]
fn instant_potion_custom_effects_match_exact_type_and_amplifier_without_color_guesses() {
    let base = properties::parse(b"items=minecraft:splash_potion\nnbt.Potion=minecraft:healing\nmodel=minecraft:item/healing\nweight=99979\n").unwrap();
    let variants = potions::variants(&base, 15, "minecraft:splash_potion");
    assert_eq!(variants.len(), 1);
    let variant = &variants[0].1;
    assert_eq!(variant["nbt.CustomPotionEffects.count"], "1");
    assert_eq!(variant["nbt.CustomPotionEffects.0.Id"], "6");
    assert_eq!(variant["nbt.CustomPotionEffects.0.Amplifier"], "0");
    assert_eq!(variant["weight"], "199979");
    assert!(!variant.contains_key("nbt.display.Name"));
    assert!(!variant.contains_key("nbt.CustomPotionColor"));
    let mut strong = base.clone();
    strong.insert("nbt.Potion".into(), "minecraft:strong_healing".into());
    assert_eq!(
        potions::variants(&strong, 15, "minecraft:splash_potion")[0].1["nbt.CustomPotionEffects.0.Amplifier"],
        "1"
    );
    let mut other = base.clone();
    other.insert("nbt.Potion".into(), "minecraft:swiftness".into());
    assert!(potions::variants(&other, 15, "minecraft:splash_potion").is_empty());
    assert!(potions::variants(&base, 32, "minecraft:splash_potion").is_empty());
}

#[test]
fn transparent_potion_sprites_do_not_reduce_the_shared_atlas_mip_level() {
    let work = tempfile::tempdir().unwrap();
    let root = work.path();
    json_file(
        root,
        "assets/test/models/item/potion.json",
        &json!({"parent":"minecraft:item/generated","textures":{"layer0":"minecraft:item/diamond_sword"}}),
    );
    let mut outputs = BTreeMap::new();
    let model = models::compose(
        root,
        15,
        "minecraft:potion",
        &[json!({"type":"minecraft:model","model":"test:item/potion"})],
        &mut outputs,
    )
    .unwrap();
    assert_eq!(model["textures"]["layer1"], "minecraft:item/diamond_sword");
    let image = image::load_from_memory(outputs.values().next().unwrap())
        .unwrap()
        .to_rgba8();
    assert_eq!(image.dimensions(), (16, 16));
    assert!(image.pixels().all(|pixel| pixel.0[3] == 0));
}

#[test]
fn potion_type_nbt_and_component_paths_follow_the_actual_version_boundary() {
    let body = "items=minecraft:splash_potion\nnbt.Potion=minecraft:strong_healing\n";
    let original = properties::parse(body.as_bytes()).unwrap();
    let mut current = original.clone();
    super::super::cit_rules::conditions(&mut current, Some(15), 32, Some("1.20.6"));
    assert_eq!(
        current
            .get("components.minecraft:potion_contents.potion")
            .map(String::as_str),
        Some("minecraft:strong_healing")
    );
    assert!(!current.contains_key("nbt.Potion"));
    super::super::cit_rules::conditions(&mut current, Some(32), 15, Some("1.20.1"));
    assert_eq!(current, original);
}

#[test]
fn malformed_context_definition_does_not_abort_other_items_or_delete_old_generated_files() {
    let work = tempfile::tempdir().unwrap();
    let root = work.path();
    fixture(root);
    let mut broken = scoped_definition();
    broken["model"].as_object_mut().unwrap().remove("fallback");
    json_file(root, "assets/minecraft/items/iron_sword.json", &broken);
    let report = down(root);
    assert!(
        report
            .warnings
            .iter()
            .any(|text| text.contains("缺少 fallback"))
    );
    let rules = generated_rules(root).unwrap();
    assert_eq!(rules.len(), 2);
    let mut state: State = serde_json::from_slice(&disk::read(root.join(CACHE)).unwrap()).unwrap();
    state.generator = 0;
    disk::write(root.join(CACHE), serde_json::to_vec(&state).unwrap()).unwrap();
    migrate(root, 1, "1.8.9", &mut AssetReport::default()).unwrap();
    assert!(rules.iter().all(|path| path.exists()));
}
