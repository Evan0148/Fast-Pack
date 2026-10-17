use super::super::{assets::AssetReport, cit, properties};
use super::*;
use crate::packfs as fs;

fn convert(root: &Path, source: u32, target: &str) -> Properties {
    let paths = cit::migrate_from(
        root,
        Some(source),
        crate::pack_format_of(target).unwrap(),
        Some(target),
        &mut AssetReport::default(),
    )
    .unwrap();
    assert_eq!(paths.len(), 1);
    properties::parse(&fs::read(paths.keys().next().unwrap()).unwrap()).unwrap()
}
fn fixture(root: &Path, text: &str) {
    let path = root.join("assets/minecraft/mcpatcher/cit/test.properties");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

#[test]
fn block_dye_food_variants_round_trip_without_reinterpreting_item_wear() {
    let work = tempfile::tempdir().unwrap();
    for (id, damage, modern) in [
        (35, 14, "red_wool"),
        (351, 1, "red_dye"),
        (263, 1, "charcoal"),
        (322, 1, "enchanted_golden_apple"),
        (31, 1, "short_grass"),
    ] {
        let root = work.path().join(id.to_string());
        let _mount = fs::Mount::new(&root);
        fixture(
            &root,
            &format!("type=item\nitems={id}\ndamage={damage}\nnbt.custom_flag=pattern:Ruby*\n"),
        );
        let new = convert(&root, 1, "1.21.10");
        assert_eq!(new["items"], format!("minecraft:{modern}"));
        assert!(!new.contains_key("damage"));
        assert_eq!(
            new["components.minecraft:custom_data.custom_flag"],
            "pattern:Ruby*"
        );
        let old = convert(&root, 69, "1.8.9");
        assert_eq!(old["items"], id.to_string());
        assert_eq!(old["damage"], damage.to_string());
        assert_eq!(old["nbt.custom_flag"], "pattern:Ruby*");
    }
    let root = work.path().join("wear");
    let _mount = fs::Mount::new(&root);
    fixture(
        &root,
        "type=armor\nitems=310\ndamage=25-75%\ndamageMask=65535\n",
    );
    let value = convert(&root, 1, "1.21.10");
    assert_eq!(value["items"], "minecraft:diamond_helmet");
    assert_eq!(value["damage"], "25-75%");
    assert_eq!(value["damageMask"], "65535");
}

#[test]
fn grass_block_and_short_grass_are_disambiguated_using_source_version() {
    let work = tempfile::tempdir().unwrap();
    let root = work.path().join("block");
    let _mount = fs::Mount::new(&root);
    fixture(&root, "items=2\n");
    let block = convert(&root, 1, "1.20.1");
    assert_eq!(block["items"], "minecraft:grass_block");
    let old = convert(&root, 15, "1.8.9");
    assert_eq!(old["items"], "2");
    assert!(!old.contains_key("damage"));
    let root = work.path().join("plant");
    let _mount = fs::Mount::new(&root);
    fixture(&root, "items=grass\n");
    let old = convert(&root, 15, "1.8.9");
    assert_eq!(old["items"], "31");
    assert_eq!(old["damage"], "1");
    let new = convert(&root, 1, "1.20.1");
    assert_eq!(new["items"], "minecraft:grass");
    assert!(!new.contains_key("damage"));
}

#[test]
fn patch_versions_components_and_enchantment_identifiers_are_reversible() {
    for (version, expected) in [
        ("1.15.2", "minecraft:zombie_pigman_spawn_egg"),
        ("1.16.1", "minecraft:zombified_piglin_spawn_egg"),
    ] {
        let mut values =
            Properties::from([("items".into(), "minecraft:zombie_pigman_spawn_egg".into())]);
        conditions(&mut values, Some(5), 5, Some(version));
        assert_eq!(values["items"], expected);
    }
    let mut values = Properties::from([
        ("items".into(), "minecraft:diamond_sword".into()),
        ("enchantmentIDs".into(), "16 34".into()),
        ("nbt.CustomModelData".into(), "7".into()),
        ("nbt.display.Name".into(), "pattern:Ruby*".into()),
    ]);
    conditions(&mut values, Some(1), 69, Some("1.21.10"));
    assert_eq!(
        values["enchantments"],
        "minecraft:sharpness minecraft:unbreaking"
    );
    assert_eq!(
        values["components.minecraft:custom_model_data.floats.0"],
        "7"
    );
    conditions(&mut values, Some(69), 1, Some("1.8.9"));
    assert_eq!(values["enchantmentIDs"], "16 34");
    assert_eq!(values["nbt.CustomModelData"], "7");
    assert_eq!(values["nbt.display.Name"], "pattern:Ruby*");
}

#[test]
fn cit_model_hand_view_keys_follow_the_same_conversion_as_regular_item_models() {
    let work = tempfile::tempdir().unwrap();
    let root = work.path();
    let _mount = fs::Mount::new(root);
    fixture(root, "items=diamond_sword\nmodel=./model\n");
    let path = root.join("assets/minecraft/mcpatcher/cit/model.json");
    fs::write(&path,br#"{"parent":"minecraft:item/handheld","display":{"firstperson_righthand":{"rotation":[0,90,25],"translation":[1,2,3],"scale":[0.7,0.7,0.7]}}}"#).unwrap();
    let paths = cit::migrate_from(
        root,
        Some(15),
        1,
        Some("1.8.9"),
        &mut AssetReport::default(),
    )
    .unwrap();
    let rule = properties::parse(&fs::read(paths.keys().next().unwrap()).unwrap()).unwrap();
    let (namespace, name) = rule["model"].split_once(':').unwrap();
    let path = root.join(format!("assets/{namespace}/{name}.json"));
    let old = crate::model_json::parse(&fs::read(path).unwrap()).unwrap();
    assert!(old["display"].get("firstperson").is_some());
    assert!(old["display"].get("firstperson_righthand").is_none());
    let paths = cit::migrate_from(
        root,
        Some(1),
        69,
        Some("1.21.10"),
        &mut AssetReport::default(),
    )
    .unwrap();
    let rule = properties::parse(&fs::read(paths.keys().next().unwrap()).unwrap()).unwrap();
    let (namespace, name) = rule["model"].split_once(':').unwrap();
    let new = crate::model_json::parse(
        &fs::read(root.join(format!("assets/{namespace}/{name}.json"))).unwrap(),
    )
    .unwrap();
    assert!(new["display"].get("firstperson_righthand").is_some());
    assert!(new["display"].get("firstperson").is_none());
}
