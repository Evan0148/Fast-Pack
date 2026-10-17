use super::*;
use std::fs as disk;

fn json_file(root: &Path, path: &str, value: &Value) {
    let path = root.join(path);
    disk::create_dir_all(path.parent().unwrap()).unwrap();
    disk::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}
fn definition() -> Value {
    json!({"hand_animation_on_swap":false,"model":{
        "type":"minecraft:select","property":"minecraft:display_context","cases":[{"when":"gui","model":{
            "type":"minecraft:composite","models":[{"type":"minecraft:model","model":"minecraft:item/diamond_sword"},{
                "type":"minecraft:condition","property":"minecraft:component","predicate":"minecraft:enchantments","value":[{"enchantments":"minecraft:sharpness","levels":{"min":3}}],
                "on_true":{"type":"minecraft:model","model":"another_namespace:item/number"},"on_false":{"type":"minecraft:empty"}
            }]
        }}],"fallback":{"type":"minecraft:model","model":"minecraft:item/diamond_sword"}
    }})
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
