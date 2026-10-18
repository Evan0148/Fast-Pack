use super::*;
fn settings(features: &[Feature]) -> Settings {
    Settings {
        features: features.to_vec(),
    }
}

#[test]
fn reconversion_unwraps_only_owned_layers_and_preserves_edited_graphs() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let mut report = AssetReport::default();
    let settings = settings(&[Feature::Sharpness]);
    apply(root, "1.21.10", &settings, &mut report).unwrap();
    let original = read(root, "assets/minecraft/items/diamond_sword.json");
    assert_eq!(effective(root, &Settings::default()).unwrap(), settings);
    prepare(root).unwrap();
    assert_eq!(
        read(root, "assets/minecraft/items/diamond_sword.json"),
        original["fastpack_builtin_hud"]
    );
    apply(root, "1.21.10", &settings, &mut report).unwrap();
    assert_eq!(
        read(root, "assets/minecraft/items/diamond_sword.json"),
        original
    );
    let mut edited = original.clone();
    edited["hand_animation_on_swap"] = json!(false);
    write(
        root,
        "assets/minecraft/items/diamond_sword.json",
        &serde_json::to_vec(&edited).unwrap(),
    )
    .unwrap();
    assert!(!prepare(root).unwrap().is_empty());
    apply(root, "1.21.10", &settings, &mut report).unwrap();
    assert_eq!(
        read(root, "assets/minecraft/items/diamond_sword.json"),
        edited
    );
}
fn read(root: &Path, name: &str) -> Value {
    serde_json::from_slice(&std::fs::read(root.join(name)).unwrap()).unwrap()
}

#[test]
fn disabled_features_leave_resources_and_settings_untouched() {
    let temp = tempfile::tempdir().unwrap();
    apply(
        temp.path(),
        "1.20.1",
        &Settings::default(),
        &mut AssetReport::default(),
    )
    .unwrap();
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
    let mut s = Settings::default();
    s.set(Feature::Sharpness, true);
    s.set(Feature::Sharpness, true);
    assert_eq!(s.count(), 1);
    s.set(Feature::Sharpness, false);
    assert!(!s.active());
}

#[test]
fn modern_gui_layers_preserve_custom_base_bow_states_and_do_not_accumulate() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let path = "assets/minecraft/items/diamond_sword.json";
    let authored = json!({"hand_animation_on_swap":false,"model":{"type":"minecraft:model","model":"author:item/short_sword"}});
    write(root, path, &serde_json::to_vec(&authored).unwrap()).unwrap();
    let s = settings(&[
        Feature::Sharpness,
        Feature::GoldenDurability,
        Feature::BowEnchantments,
        Feature::ArmorProtection,
    ]);
    let mut report = AssetReport::default();
    apply(root, "1.21.10", &s, &mut report).unwrap();
    let first = read(root, path);
    assert_eq!(first["fastpack_builtin_hud"], authored);
    assert_eq!(first["model"]["fallback"], authored["model"]);
    assert_eq!(
        first["model"]["cases"][0]["model"]["models"][0],
        authored["model"]
    );
    assert_eq!(first["hand_animation_on_swap"], false);
    apply(root, "1.21.10", &s, &mut report).unwrap();
    assert_eq!(read(root, path), first);
    let bow = read(root, "assets/minecraft/items/bow.json");
    let text = bow.to_string();
    assert!(
        text.contains("using_item")
            && text.contains("use_duration")
            && text.contains("bowhud:item/pro_5")
    );
    let gold = read(root, "assets/minecraft/items/golden_sword.json").to_string();
    assert!(gold.contains("swordhud:item/dur/32") && gold.contains("swordhud:item/sharp/5"));
    let leather = read(root, "assets/minecraft/items/leather_chestplate.json").to_string();
    assert!(
        leather.contains("minecraft:dye")
            && leather.contains("trim")
            && leather.contains("armorhud:item/pro_4")
    );
}

#[test]
fn numeric_features_are_independent_and_skip_items_absent_from_the_target() {
    for feature in [
        Feature::GoldenDurability,
        Feature::Sharpness,
        Feature::BowEnchantments,
        Feature::ArmorProtection,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        apply(
            root,
            "1.21.5",
            &settings(&[feature]),
            &mut AssetReport::default(),
        )
        .unwrap();
        let sword = root
            .join("assets/minecraft/items/diamond_sword.json")
            .is_file();
        let bow = root.join("assets/minecraft/items/bow.json").is_file();
        let armor = root
            .join("assets/minecraft/items/diamond_helmet.json")
            .is_file();
        assert_eq!(sword, feature == Feature::Sharpness);
        assert_eq!(bow, feature == Feature::BowEnchantments);
        assert_eq!(armor, feature == Feature::ArmorProtection);
        assert!(
            !root
                .join("assets/minecraft/items/copper_sword.json")
                .exists()
        );
        assert!(
            !root
                .join("assets/minecraft/items/copper_helmet.json")
                .exists()
        );
    }
}

#[test]
fn every_supported_shader_profile_keeps_its_native_interface_and_glyph_discard() {
    for (version, format) in &crate::tables().pack_formats {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let mut report = AssetReport::default();
        shaders(
            root,
            version,
            &settings(&[Feature::HighlightNames, Feature::ClearTooltips]),
            &mut report,
        )
        .unwrap();
        if *format < 7 {
            assert!(!root.join("assets").exists());
            assert!(!report.warnings.is_empty());
            continue;
        }
        let profile = &SHADERS.profiles[&SHADERS.versions[version]];
        let mut text_found = false;
        for (name, native) in profile {
            let output =
                std::fs::read_to_string(root.join(format!("assets/minecraft/shaders/core/{name}")))
                    .unwrap();
            if name.contains("text") && name.ends_with(".fsh") {
                assert!(
                    output.contains("color.a < 0.1")
                        && output.contains("discard;")
                        && output.contains("vertexColor.a < 0.5"),
                    "{version}"
                );
                text_found = true;
                if name == "text.fsh" {
                    assert!(output.contains("#ifdef IS_SEE_THROUGH"));
                }
            }
            for line in native.lines().filter(|line| {
                line.starts_with("#version")
                    || line.contains(" in ")
                    || line.contains(" out ")
                    || line.starts_with("uniform ")
                    || line.starts_with("#include")
                    || line.starts_with("#moj_import")
            }) {
                assert!(output.contains(line), "{version}: {line}");
            }
        }
        assert!(text_found, "{version}");
    }
}

#[test]
fn legacy_gui_numbers_do_not_replace_native_cit_cache_or_leak_into_handheld_base() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(
        root,
        "fastpack/native-cit.json",
        br#"{"author":"preserve me"}"#,
    )
    .unwrap();
    let mut report = AssetReport::default();
    apply(
        root,
        "1.20.1",
        &settings(&[Feature::Sharpness]),
        &mut report,
    )
    .unwrap();
    assert_eq!(
        std::fs::read(root.join("fastpack/native-cit.json")).unwrap(),
        br#"{"author":"preserve me"}"#
    );
    let models = crate::packfs::WalkDir::new(
        root.join("assets/minecraft/models/item/fastpack_builtin_hud_cit"),
    )
    .into_iter()
    .filter_map(Result::ok)
    .filter(|e| e.file_type().is_file())
    .collect::<Vec<_>>();
    assert!(!models.is_empty());
    let mut scoped = false;
    for entry in models {
        let model: Value = serde_json::from_slice(&std::fs::read(entry.path()).unwrap()).unwrap();
        if model["loader"] == "forge:separate_transforms" {
            assert!(!model["base"].to_string().contains("swordhud:item/sharp"));
            let gui = model["perspectives"]["gui"].to_string();
            if gui.contains("swordhud:item/sharp") || gui.contains("fastpack_builtin_hud_cit") {
                scoped = true;
            }
        }
    }
    assert!(scoped);
}
