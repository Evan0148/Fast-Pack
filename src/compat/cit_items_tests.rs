use super::*;
use image::{Rgba, RgbaImage};
use std::fs as disk;

fn write(root: &Path, name: &str, bytes: impl AsRef<[u8]>) {
    let path = root.join(name);
    disk::create_dir_all(path.parent().unwrap()).unwrap();
    disk::write(path, bytes).unwrap();
}
fn json_file(root: &Path, name: &str) -> Value {
    serde_json::from_slice(&disk::read(root.join(name)).unwrap()).unwrap()
}
fn image(root: &Path, name: &str) {
    let path = root.join(name);
    disk::create_dir_all(path.parent().unwrap()).unwrap();
    RgbaImage::from_pixel(16, 16, Rgba([123, 45, 67, 180]))
        .save(path)
        .unwrap();
}
fn run(root: &Path, target: u32, version: &str) -> AssetReport {
    let mut report = AssetReport::default();
    let paths =
        super::super::cit::migrate_versioned(root, target, Some(version), &mut report).unwrap();
    migrate(root, target, version, &paths, &mut report).unwrap();
    report
}
fn selected(value: &Value, cmd: Option<f32>) -> String {
    match value["type"].as_str().unwrap() {
        "minecraft:model" => value["model"].as_str().unwrap().into(),
        "minecraft:condition" => selected(
            &value[if cmd.is_some() { "on_true" } else { "on_false" }],
            cmd,
        ),
        "minecraft:range_dispatch" => {
            let current = cmd.unwrap_or(0.0);
            let chosen = value["entries"]
                .as_array()
                .unwrap()
                .iter()
                .rev()
                .find(|entry| entry["threshold"].as_f64().unwrap() as f32 <= current);
            selected(
                chosen.map_or(&value["fallback"], |entry| &entry["model"]),
                cmd,
            )
        }
        other => panic!("unexpected node: {other}"),
    }
}
fn model_path(root: &Path, reference: &str) -> PathBuf {
    let (namespace, name) = id(reference).unwrap();
    root.join(format!("assets/{namespace}/models/{name}.json"))
}
fn color_path(root: &Path, model: &Value) -> PathBuf {
    let (namespace, name) = id(model["textures"]["layer0"].as_str().unwrap()).unwrap();
    root.join(format!("assets/{namespace}/textures/{name}.png"))
}
fn authored_fallback(root: &Path) {
    write(root,"assets/minecraft/items/diamond_sword.json",br#"{"hand_animation_on_swap":false,"model":{"type":"minecraft:model","model":"demo:item/base","tints":[{"type":"minecraft:constant","value":16777215}]}}"#);
}
fn cit(root: &Path, folder: &str, file: &str, body: &str) {
    write(
        root,
        &format!("assets/minecraft/{folder}/cit/{file}.properties"),
        body,
    );
    image(root, &format!("assets/minecraft/{folder}/cit/{file}.png"));
}

#[test]
fn upstream_cmd_case_uses_numeric_exact_dispatch_and_preserves_default_tree() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    authored_fallback(root);
    let before = json_file(root, "assets/minecraft/items/diamond_sword.json");
    cit(
        root,
        "mcpatcher",
        "Ruby Sword/ruby",
        "type=item\nitems=diamond_sword\ntexture=./ruby\nnbt.CustomModelData=7\n",
    );
    let report = run(root, 69, "1.21.10");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let value = json_file(root, "assets/minecraft/items/diamond_sword.json");
    assert_eq!(value["hand_animation_on_swap"], false);
    let replacement = selected(&value["model"], Some(7.0));
    for cmd in [
        None,
        Some(0.0),
        Some(6.0),
        Some(7.5),
        Some(8.0),
        Some(100.0),
    ] {
        assert_eq!(selected(&value["model"], cmd), "demo:item/base");
    }
    assert_eq!(value["model"]["on_false"], before["model"]);
    let model: Value =
        serde_json::from_slice(&disk::read(model_path(root, &replacement)).unwrap()).unwrap();
    assert_eq!(model["parent"], "minecraft:item/handheld");
    assert!(color_path(root, &model).is_file());
    assert!(!value.to_string().contains("\"when\":7"));
    let first = disk::read(root.join("assets/minecraft/items/diamond_sword.json")).unwrap();
    run(root, 69, "1.21.10");
    assert_eq!(
        disk::read(root.join("assets/minecraft/items/diamond_sword.json")).unwrap(),
        first
    );
}

#[test]
fn animations_relative_models_namespaces_and_geometry_are_preserved() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    authored_fallback(root);
    write(
        root,
        "assets/demo/mcpatcher/cit/armor colors/test.properties",
        "items=minecraft:diamond_sword, minecraft:stick\nnbt.CustomModelData=12\nmodel=./model\n",
    );
    write(root,"assets/demo/mcpatcher/cit/armor colors/model.json",br##"// Model comment
{"parent":"./parent","textures":{"layer0":"./Animated Color"},"display":{"gui":{"scale":[0.5,0.5,0.5]}}}"##);
    write(root,"assets/demo/mcpatcher/cit/armor colors/parent.json",br##"{"parent":"builtin/generated","textures":{"particle":"#layer0"},"elements":[{"from":[0,0,7],"to":[16,16,9],"faces":{"north":{"texture":"#layer0"}}}]}"##);
    image(
        root,
        "assets/demo/mcpatcher/cit/armor colors/Animated Color.png",
    );
    let metadata = br#"{"animation":{"frametime":3,"frames":[0],"interpolate":true}}"#;
    write(
        root,
        "assets/demo/mcpatcher/cit/armor colors/Animated Color.png.mcmeta",
        metadata,
    );
    let report = run(root, 69, "1.21.10");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let value = json_file(root, "assets/minecraft/items/diamond_sword.json");
    let model: Value = serde_json::from_slice(
        &disk::read(model_path(root, &selected(&value["model"], Some(12.0)))).unwrap(),
    )
    .unwrap();
    assert_eq!(model["display"]["gui"]["scale"], json!([0.5, 0.5, 0.5]));
    let parent: Value = serde_json::from_slice(
        &disk::read(model_path(root, model["parent"].as_str().unwrap())).unwrap(),
    )
    .unwrap();
    assert_eq!(parent["elements"][0]["from"], json!([0, 0, 7]));
    let texture = color_path(root, &model);
    assert!(texture.is_file());
    assert_eq!(
        disk::read(texture.with_extension("png.mcmeta")).unwrap(),
        metadata
    );
    assert!(root.join("assets/minecraft/items/stick.json").is_file());
}

#[test]
fn weight_and_unsupported_conditions_never_widen_matching() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    authored_fallback(root);
    cit(
        root,
        "optifine",
        "low",
        "items=diamond_sword\nnbt.CustomModelData=7\nweight=0\n",
    );
    cit(
        root,
        "optifine",
        "high",
        "items=diamond_sword\nnbt.CustomModelData=7\nweight=5\nnbt.display.Name=Secret\n",
    );
    cit(
        root,
        "optifine",
        "other",
        "items=diamond_sword\nnbt.CustomModelData=8\nweight=7\n",
    );
    let report = run(root, 69, "1.21.10");
    assert!(
        report
            .warnings
            .iter()
            .any(|value| value.contains("custom_name"))
    );
    let value = json_file(root, "assets/minecraft/items/diamond_sword.json");
    assert_eq!(selected(&value["model"], Some(7.0)), "demo:item/base");
    assert_ne!(selected(&value["model"], Some(8.0)), "demo:item/base");
}

#[test]
fn cmd_zero_negative_and_adjacent_numbers_do_not_match_absent_or_fractional_values() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    authored_fallback(root);
    for code in [-1, 0, 1, 2] {
        cit(
            root,
            "optifine",
            &format!("code_{code}"),
            &format!("items=diamond_sword\nnbt.CustomModelData={code}\n"),
        );
    }
    run(root, 46, "1.21.4");
    let value = json_file(root, "assets/minecraft/items/diamond_sword.json");
    let models: BTreeSet<_> = [-1.0, 0.0, 1.0, 2.0]
        .iter()
        .map(|code| selected(&value["model"], Some(*code)))
        .collect();
    assert_eq!(models.len(), 4);
    for code in [
        None,
        Some(-2.0),
        Some(-0.5),
        Some(0.5),
        Some(1.5),
        Some(3.0),
    ] {
        assert_eq!(selected(&value["model"], code), "demo:item/base");
    }
}

#[test]
fn missing_models_cycles_and_traversal_do_not_generate_broken_definitions() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    authored_fallback(root);
    let before = disk::read(root.join("assets/minecraft/items/diamond_sword.json")).unwrap();
    cit(
        root,
        "optifine",
        "missing",
        "items=diamond_sword\nnbt.CustomModelData=7\nmodel=./missing_model\n",
    );
    cit(
        root,
        "optifine",
        "cycle",
        "items=diamond_sword\nnbt.CustomModelData=8\nmodel=./a\n",
    );
    write(
        root,
        "assets/minecraft/optifine/cit/a.json",
        br#"{"parent":"./b"}"#,
    );
    write(
        root,
        "assets/minecraft/optifine/cit/b.json",
        br#"{"parent":"./a"}"#,
    );
    cit(
        root,
        "optifine",
        "escape",
        "items=diamond_sword\nnbt.CustomModelData=9\ntexture=../../../../../../outside\n",
    );
    let report = run(root, 69, "1.21.10");
    assert!(report.warnings.len() >= 3);
    assert_eq!(
        disk::read(root.join("assets/minecraft/items/diamond_sword.json")).unwrap(),
        before
    );
    assert!(
        !root
            .join("assets/minecraft/models/item/fastpack_cit")
            .exists()
    );
}

#[test]
fn both_roots_and_duplicate_basenames_keep_distinct_resources_and_defaults() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    authored_fallback(root);
    cit(
        root,
        "mcpatcher",
        "same",
        "items=diamond_sword\nnbt.CustomModelData=7\n",
    );
    cit(
        root,
        "optifine",
        "same",
        "items=diamond_sword\nnbt.CustomModelData=8\n",
    );
    run(root, 69, "1.21.10");
    let value = json_file(root, "assets/minecraft/items/diamond_sword.json");
    assert_ne!(
        selected(&value["model"], Some(7.0)),
        selected(&value["model"], Some(8.0))
    );
    let original = value["model"]["on_false"].clone();
    let paths: Vec<_> = walkdir::WalkDir::new(root.join("assets/minecraft/optifine/cit"))
        .into_iter()
        .map(Result::unwrap)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|ext| ext == "properties")
        })
        .map(|entry| entry.path().to_path_buf())
        .collect();
    for path in paths {
        disk::remove_file(path).unwrap();
    }
    run(root, 69, "1.21.10");
    assert_eq!(
        json_file(root, "assets/minecraft/items/diamond_sword.json")["model"],
        original
    );
}

#[test]
fn unconditional_rules_can_replace_default_without_fabricating_a_cmd_requirement() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    authored_fallback(root);
    cit(
        root,
        "optifine",
        "default",
        "items=diamond_sword\nweight=0\n",
    );
    cit(
        root,
        "optifine",
        "special",
        "items=diamond_sword\nnbt.CustomModelData=7\nweight=2\n",
    );
    run(root, 69, "1.21.10");
    let value = json_file(root, "assets/minecraft/items/diamond_sword.json");
    let default = selected(&value["model"], None);
    assert_ne!(default, "demo:item/base");
    assert_eq!(default, selected(&value["model"], Some(100.0)));
    assert_ne!(default, selected(&value["model"], Some(7.0)));
}

#[test]
fn old_targets_keep_mod_rules_and_do_not_gain_modern_item_definitions() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    cit(
        root,
        "optifine",
        "ruby",
        "items=diamond_sword\nnbt.CustomModelData=7\n",
    );
    run(root, 15, "1.20.1");
    assert!(!root.join("assets/minecraft/items").exists());
    assert!(
        root.join("assets/minecraft/optifine/cit/ruby.properties")
            .is_file()
    );
}

#[test]
fn every_version_pair_preserves_cit_resources_conditions_and_directory_identity() {
    let work = tempfile::tempdir().unwrap();
    let profiles = &crate::tables().pack_formats;
    let mut checked = 0;
    for (source_version, source_format) in profiles {
        for (target_version, target_format) in profiles {
            let root = work.path().join(format!("case_{checked}"));
            let _mount = crate::packfs::Mount::new(&root);
            let base = "assets/minecraft/mcpatcher/cit/Armor Colors";
            fs::create_dir_all(root.join(base)).unwrap();
            for (name, text) in [
                (
                    "weapon",
                    "type=item\nitems=diamond_sword\nnbt.CustomModelData=7\ntexture=./color\n",
                ),
                (
                    "armor",
                    "type=armor\nitems=diamond_chestplate\ndamage=25-75%\ndamageMask=65535\ntexture.diamond_layer_1=./color\n",
                ),
                (
                    "named",
                    "type=item\nitems=diamond_sword\nnbt.display.Name=ipattern:Ruby *\nnbt.display.Lore.*=pattern:Test *\ntexture=./color\n",
                ),
            ] {
                fs::write(root.join(format!("{base}/{name}.properties")), text).unwrap();
            }
            let png = RgbaImage::from_pixel(16, 16, Rgba([12, 34, 56, 170]));
            fs::save_image(&png, root.join(format!("{base}/color.png"))).unwrap();
            let metadata = b"{\"animation\":{\"frametime\":3,\"frames\":[0]}}";
            fs::write(root.join(format!("{base}/color.png.mcmeta")), metadata).unwrap();
            let mut count = None;
            for (version, format) in [
                (source_version, *source_format),
                (target_version, *target_format),
                (source_version, *source_format),
            ] {
                let mut report = AssetReport::default();
                let paths =
                    super::super::cit::migrate_versioned(&root, format, Some(version), &mut report)
                        .unwrap();
                migrate(&root, format, version, &paths, &mut report).unwrap();
                assert_eq!(paths.len(), 3, "{source_version} -> {target_version}");
                let resources = Resources::new(&root, format).unwrap();
                for (path, origin) in &paths {
                    assert!(origin.starts_with("assets/minecraft/mcpatcher/cit/Armor Colors/"));
                    let relative = path
                        .strip_prefix(&root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    assert!(
                        relative.starts_with(if format < 4 {
                            "assets/minecraft/mcpatcher/cit/"
                        } else {
                            "assets/minecraft/optifine/cit/"
                        }),
                        "{version}: {relative}"
                    );
                    let values = properties::parse(&fs::read(path).unwrap()).unwrap();
                    if origin.ends_with("weapon.properties") {
                        assert_eq!(
                            values[if format >= 46 {
                                "components.minecraft:custom_model_data.floats.0"
                            } else if format >= 32 {
                                "components.minecraft:custom_model_data"
                            } else {
                                "nbt.CustomModelData"
                            }],
                            "7"
                        );
                    } else if origin.ends_with("armor.properties") {
                        assert_eq!(values["damage"], "25-75%");
                        assert_eq!(values["damageMask"], "65535");
                    } else {
                        assert_eq!(
                            values[if format >= 32 {
                                "components.minecraft:custom_name"
                            } else {
                                "nbt.display.Name"
                            }],
                            "ipattern:Ruby *"
                        );
                        assert_eq!(
                            values[if format >= 32 {
                                "components.minecraft:lore.*"
                            } else {
                                "nbt.display.Lore.*"
                            }],
                            "pattern:Test *"
                        );
                    }
                    for (key, value) in &values {
                        if key == "texture" || key.starts_with("texture.") {
                            let texture = resources.resolve(path, value, ".png").unwrap();
                            assert_eq!(fs::open_image(&texture).unwrap().to_rgba8(), png);
                            assert_eq!(
                                fs::read(texture.with_extension("png.mcmeta")).unwrap(),
                                metadata
                            );
                        }
                    }
                }
                let cit_files = WalkDir::new(root.join("assets/minecraft"))
                    .into_iter()
                    .map(Result::unwrap)
                    .filter(|entry| {
                        entry.file_type().is_file()
                            && entry
                                .path()
                                .to_string_lossy()
                                .replace('\\', "/")
                                .contains("/cit/")
                    })
                    .count();
                if let Some(previous) = count {
                    assert_eq!(
                        cit_files, previous,
                        "CIT folder growth: {source_version} -> {target_version}"
                    );
                } else {
                    count = Some(cit_files);
                }
            }
            checked += 1;
        }
    }
    assert_eq!(checked, profiles.len() * profiles.len());
}

#[test]
fn inherited_item_pose_and_user_edits_survive_conversion() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    authored_fallback(root);
    write(
        root,
        "assets/minecraft/models/item/diamond_sword.json",
        br#"{"parent":"demo:item/pose","display":{"gui":{"scale":[0.5,0.5,0.5]}}}"#,
    );
    write(root,"assets/demo/models/item/pose.json",br#"{"parent":"minecraft:item/handheld","display":{"thirdperson_righthand":{"scale":[0.7,0.7,0.7]}}}"#);
    cit(
        root,
        "optifine",
        "texture_without_tool_suffix",
        "items=diamond_sword\nnbt.CustomModelData=7\n",
    );
    run(root, 69, "1.21.10");
    let mut value = json_file(root, "assets/minecraft/items/diamond_sword.json");
    let model: Value = serde_json::from_slice(
        &disk::read(model_path(root, &selected(&value["model"], Some(7.0)))).unwrap(),
    )
    .unwrap();
    assert_eq!(model["parent"], "minecraft:item/handheld");
    assert_eq!(model["display"]["gui"]["scale"], json!([0.5, 0.5, 0.5]));
    assert_eq!(
        model["display"]["thirdperson_righthand"]["scale"],
        json!([0.7, 0.7, 0.7])
    );
    value["model"] = json!({"type":"minecraft:model","model":"demo:item/user_edit"});
    let path = root.join("assets/minecraft/items/diamond_sword.json");
    disk::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let report = run(root, 69, "1.21.10");
    assert_eq!(
        json_file(root, "assets/minecraft/items/diamond_sword.json"),
        value
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|value| value.contains("已生成定义被修改"))
    );
}

#[test]
fn legacy_cmd_bindings_preserve_other_overrides_and_survive_repeated_downgrades() {
    for version in ["1.14", "1.16.5", "1.20.1", "1.20.5", "1.21.1", "1.21.3"] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        write(root,"assets/minecraft/models/item/bow.json",br#"{"parent":"minecraft:item/generated","textures":{"layer0":"minecraft:item/bow"},"display":{"gui":{"scale":[0.5,0.5,0.5]}},"overrides":[{"predicate":{"pulling":1},"model":"minecraft:item/bow_pulling_0"},{"predicate":{"pulling":1,"pull":0.65},"model":"minecraft:item/bow_pulling_1"},{"predicate":{"custom_model_data":99},"model":"minecraft:item/other"}]}"#);
        for code in [7, 9] {
            cit(
                root,
                "optifine",
                &format!("bow_{code}"),
                &format!("items=bow\nnbt.CustomModelData={code}\n"),
            );
        }
        let format = crate::pack_format_of(version).unwrap();
        run(root, format, version);
        let model = json_file(root, "assets/minecraft/models/item/bow.json");
        let selected = |cmd: f64| {
            model["overrides"]
                .as_array()
                .unwrap()
                .iter()
                .rev()
                .find(|entry| {
                    entry["predicate"]
                        .as_object()
                        .unwrap()
                        .iter()
                        .all(|(key, value)| {
                            let actual = match key.as_str() {
                                "custom_model_data" => cmd,
                                "pulling" => 1.0,
                                "pull" => 0.9,
                                _ => panic!(),
                            };
                            actual >= value.as_f64().unwrap()
                        })
                })
                .unwrap()["model"]
                .as_str()
                .unwrap()
                .to_string()
        };
        for code in [7.0, 9.0] {
            assert!(
                selected(code).starts_with("minecraft:item/fastpack_cit/"),
                "{version}"
            );
        }
        for code in [0.0, 6.0, 8.0, 10.0] {
            assert_eq!(
                selected(code),
                "minecraft:item/bow_pulling_1",
                "{version} cmd={code}"
            );
        }
        assert_eq!(selected(99.0), "minecraft:item/other");
        assert_eq!(model["display"]["gui"]["scale"], json!([0.5, 0.5, 0.5]));
        let first = disk::read(root.join("assets/minecraft/models/item/bow.json")).unwrap();
        run(root, format, version);
        assert_eq!(
            disk::read(root.join("assets/minecraft/models/item/bow.json")).unwrap(),
            first
        );
    }
}

#[test]
fn modern_components_uppercase_files_and_shared_animations_keep_targeted_items() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    authored_fallback(root);
    write(
        root,
        "assets/minecraft/optifine/cit/Ruby/RULE.PROPERTIES",
        "type=item\nitems=diamond_sword stick\ncomponents.minecraft\\:custom_model_data=42\ntexture=./IMAGE.PNG\n",
    );
    image(root, "assets/minecraft/optifine/cit/Ruby/IMAGE.PNG");
    run(root, 69, "1.21.10");
    for item in ["diamond_sword", "stick"] {
        let value = json_file(root, &format!("assets/minecraft/items/{item}.json"));
        let model: Value = serde_json::from_slice(
            &disk::read(model_path(root, &selected(&value["model"], Some(42.0)))).unwrap(),
        )
        .unwrap();
        assert!(color_path(root, &model).is_file());
        assert_eq!(
            model["parent"],
            if item == "diamond_sword" {
                "minecraft:item/handheld"
            } else {
                "minecraft:item/generated"
            }
        );
    }
}

#[test]
fn complete_1201_conversion_exports_native_cit_without_changing_source_or_other_models() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    write(
        &source,
        "pack.mcmeta",
        br#"{"pack":{"pack_format":15,"description":"CIT conversion"}}"#,
    );
    write(&source,"assets/minecraft/models/item/diamond_sword.json",br#"{"parent":"minecraft:item/handheld","textures":{"layer0":"minecraft:item/diamond_sword"},"display":{"gui":{"scale":[0.8,0.8,0.8]}}}"#);
    image(&source, "assets/minecraft/textures/item/diamond_sword.png");
    cit(
        &source,
        "optifine",
        "ruby",
        "items=diamond_sword\nnbt.CustomModelData=7\n",
    );
    let original =
        disk::read(source.join("assets/minecraft/optifine/cit/ruby.properties")).unwrap();
    for version in ["1.21.4", "1.21.10", "1.21.11", "26.4"] {
        let output = temp.path().join(version);
        let report = crate::convert_pack(
            &source,
            &output,
            &crate::ConversionOptions {
                src_format: Some(15),
                tgt_format: crate::pack_format_of(version).unwrap(),
                target_version: Some(version.into()),
                as_zip: false,
                auto_netherite: false,
                auto_client_setup: false,
                ..Default::default()
            },
            |_, _| {},
        )
        .unwrap();
        assert!(
            report
                .assets
                .converted
                .iter()
                .any(|value| value.contains("CIT 原版物品适配")),
            "{version}"
        );
        let definition = json_file(&output, "assets/minecraft/items/diamond_sword.json");
        let node = selected(&definition["model"], Some(7.0));
        assert!(
            node.starts_with("minecraft:item/fastpack_cit/"),
            "{version}"
        );
        let model: Value =
            serde_json::from_slice(&disk::read(model_path(&output, &node)).unwrap()).unwrap();
        assert_eq!(model["display"]["gui"]["scale"], json!([0.8, 0.8, 0.8]));
        assert!(color_path(&output, &model).is_file());
        assert!(
            output
                .join("assets/minecraft/optifine/cit/ruby.properties")
                .is_file()
        );
        assert_eq!(
            disk::read(source.join("assets/minecraft/optifine/cit/ruby.properties")).unwrap(),
            original
        );
    }
}
