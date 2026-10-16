use super::*;

fn rule(version: &str, id: &str) -> Rule {
    Rule {
        version: version.into(),
        item: format!("minecraft:{id}"),
        ..Default::default()
    }
}

fn write(root: &Path, relative: &str, value: Value) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
}

#[test]
fn catalogs_cover_every_supported_version_and_version_specific_items() {
    for (version, _) in &crate::tables().pack_formats {
        let entries = items(version);
        assert!(!entries.is_empty(), "{version}");
        assert_eq!(
            entries
                .iter()
                .map(|item| &item.id)
                .collect::<BTreeSet<_>>()
                .len(),
            entries.len()
        );
        assert!(
            entries.iter().any(|item| item.matches("钻石剑")
                && item.matches("DIAMOND SWORD")
                && item.matches("minecraft:diamond_sword")),
            "{version}"
        );
    }
    assert!(
        !items("1.8.9")
            .iter()
            .any(|item| item.id == "minecraft:netherite_sword")
    );
    assert!(
        items("1.20.1")
            .iter()
            .any(|item| item.id == "minecraft:netherite_sword")
    );
    assert!(
        !items("1.20.1")
            .iter()
            .any(|item| item.id == "minecraft:mace")
    );
    assert!(
        items("1.21.1")
            .iter()
            .any(|item| item.id == "minecraft:mace")
    );
    assert!(items("26.4").iter().any(|item| item.id == "minecraft:mace"));
}

#[test]
fn invalid_settings_and_paths_are_rejected() {
    let mut value = rule("1.20.1", "diamond_sword");
    assert!(validate(&value).is_ok());
    value.inventory_scale = 0;
    assert!(validate(&value).is_err());
    value.inventory_scale = 100;
    value.name = Some("a\nb".into());
    assert!(validate(&value).is_err());
    assert!(resource("minecraft:../outside").is_err());
    assert!(resource("minecraft:/absolute").is_err());
}

#[test]
fn outlines_preserve_original_pixels_and_never_bleed_into_adjacent_frames() {
    let mut image = RgbaImage::new(4, 8);
    image.put_pixel(1, 3, Rgba([255, 255, 255, 255]));
    let out = outlined(
        &image,
        &Outline {
            color: [12, 34, 56],
            width: 1,
        },
    );
    assert_eq!(*out.get_pixel(1, 3), *image.get_pixel(1, 3));
    assert_eq!(*out.get_pixel(0, 3), Rgba([12, 34, 56, 255]));
    assert_eq!(*out.get_pixel(1, 4), Rgba([0; 4]));
}

#[test]
fn modern_scale_preserves_pose_and_applies_left_hand_only_once() {
    let temp = tempfile::tempdir().unwrap();
    let mut model = json!({"display":{"firstperson_righthand":{"rotation":[1,2,3],"translation":[4,5,6],"scale":[0.5,0.5,0.5]}}});
    let mut value = rule("1.20.1", "diamond_sword");
    value.held_scale = 200;
    scale_model(temp.path(), 15, &mut model, &value);
    for hand in ["firstperson_righthand", "firstperson_lefthand"] {
        assert_eq!(model["display"][hand]["scale"], json!([1.0, 1.0, 1.0]));
        assert_eq!(model["display"][hand]["rotation"], json!([1, 2, 3]));
        assert_eq!(model["display"][hand]["translation"], json!([4, 5, 6]));
    }
}

#[test]
fn item_outline_is_isolated_from_world_blocks_and_language_entries_survive() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(
        root,
        "assets/minecraft/models/item/stone.json",
        json!({"parent":"minecraft:block/stone"}),
    );
    write(
        root,
        "assets/minecraft/models/block/stone.json",
        json!({"parent":"minecraft:block/cube_all","textures":{"all":"minecraft:block/stone"}}),
    );
    write(
        root,
        "assets/minecraft/lang/zh_cn.json",
        json!({"existing":"保留"}),
    );
    let texture = root.join("assets/minecraft/textures/block/stone.png");
    std::fs::create_dir_all(texture.parent().unwrap()).unwrap();
    let mut image = RgbaImage::new(16, 16);
    image.put_pixel(8, 8, Rgba([100, 100, 100, 255]));
    image.save(&texture).unwrap();
    let before = std::fs::read(&texture).unwrap();
    let mut value = rule("1.20.1", "stone");
    value.name = Some("我的石头".into());
    value.inventory_scale = 150;
    value.outline = Some(Outline {
        color: [1, 2, 3],
        width: 1,
    });
    let mut report = AssetReport::default();
    apply(root, "1.20.1", &[value], &mut report).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(std::fs::read(&texture).unwrap(), before);
    let language: Value = serde_json::from_slice(
        &std::fs::read(root.join("assets/minecraft/lang/zh_cn.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(language["existing"], "保留");
    assert_eq!(language["block.minecraft.stone"], "我的石头");
    let model = read_model(root, 15, "item/stone").unwrap();
    assert!(
        model["textures"]["all"]
            .as_str()
            .unwrap()
            .contains("fastpack_custom")
    );
}

#[test]
fn legacy_language_and_display_keys_remain_legacy() {
    let temp = tempfile::tempdir().unwrap();
    let mut value = rule("1.8.9", "diamond_sword");
    value.name = Some("旧版钻石剑".into());
    value.held_scale = 150;
    let mut report = AssetReport::default();
    apply(temp.path(), "1.8.9", &[value], &mut report).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let language =
        std::fs::read_to_string(temp.path().join("assets/minecraft/lang/zh_CN.lang")).unwrap();
    assert!(language.contains("item.swordDiamond.name=旧版钻石剑"));
    let model = read_model(temp.path(), 1, "item/diamond_sword").unwrap();
    assert!(model["display"].get("firstperson").is_some());
    assert!(model["display"].get("firstperson_righthand").is_none());
}

#[test]
fn new_item_dispatch_keeps_stateful_branches() {
    let temp = tempfile::tempdir().unwrap();
    write(
        temp.path(),
        "assets/minecraft/items/bow.json",
        json!({"model":{"type":"minecraft:condition","property":"minecraft:using_item","on_true":{"type":"minecraft:model","model":"minecraft:item/bow_pulling_0"},"on_false":{"type":"minecraft:model","model":"minecraft:item/bow"}}}),
    );
    let mut value = rule("1.21.10", "bow");
    value.held_scale = 150;
    let mut report = AssetReport::default();
    apply(temp.path(), "1.21.10", &[value], &mut report).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let value: Value = serde_json::from_slice(
        &std::fs::read(temp.path().join("assets/minecraft/items/bow.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(value["model"]["property"], "minecraft:using_item");
    assert_ne!(
        value["model"]["on_true"]["model"],
        value["model"]["on_false"]["model"]
    );
    for branch in ["on_true", "on_false"] {
        assert!(
            model_path(
                temp.path(),
                value["model"][branch]["model"].as_str().unwrap()
            )
            .unwrap()
            .is_file()
        );
    }
}

#[test]
fn repeated_customization_never_multiplies_scale() {
    for version in ["1.8.9", "1.20.1", "1.21.10", "26.4"] {
        let temp = tempfile::tempdir().unwrap();
        let mut value = rule(version, "diamond_sword");
        value.inventory_scale = 150;
        let mut first = AssetReport::default();
        apply(temp.path(), version, &[value.clone()], &mut first).unwrap();
        assert!(first.warnings.is_empty(), "{version}: {:?}", first.warnings);
        let model_path = temp
            .path()
            .join(if crate::pack_format_of(version).unwrap() >= 46 {
                "assets/minecraft/models/item/fastpack_custom/diamond_sword/m0.json"
            } else {
                "assets/minecraft/models/item/diamond_sword.json"
            });
        let before = std::fs::read(&model_path).unwrap();
        let mut second = AssetReport::default();
        apply(temp.path(), version, &[value], &mut second).unwrap();
        assert!(
            second.warnings.is_empty(),
            "{version}: {:?}",
            second.warnings
        );
        assert_eq!(before, std::fs::read(&model_path).unwrap(), "{version}");
    }
}

#[test]
fn every_catalog_profile_can_scale_every_item_without_missing_models() {
    let temp = tempfile::tempdir().unwrap();
    let mut seen = BTreeSet::new();
    let mut failures = Vec::new();
    for (version, _) in &crate::tables().pack_formats {
        let entries = items(version);
        if version == "1.7.10" || !seen.insert(Arc::as_ptr(&entries) as usize) {
            continue;
        }
        let _mount = crate::packfs::Mount::new(temp.path());
        let rules: Vec<_> = entries
            .iter()
            .map(|item| Rule {
                version: version.into(),
                item: item.id.clone(),
                inventory_scale: 110,
                ..Default::default()
            })
            .collect();
        let mut report = AssetReport::default();
        apply(temp.path(), version, &rules, &mut report).unwrap();
        failures.extend(
            report
                .warnings
                .iter()
                .map(|warning| format!("{version}: {warning}")),
        );
    }
    assert!(
        failures.is_empty(),
        "{} missing item models: {:?}",
        failures.len(),
        &failures[..failures.len().min(30)]
    );
}
