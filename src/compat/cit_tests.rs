use super::*;
use image::{Rgba, RgbaImage};
use std::fs as disk;

fn write(root: &Path, name: &str, bytes: impl AsRef<[u8]>) {
    let path = root.join(name);
    disk::create_dir_all(path.parent().unwrap()).unwrap();
    disk::write(path, bytes).unwrap();
}

fn rules(root: &Path) -> BTreeMap<PathBuf, properties::Properties> {
    walkdir::WalkDir::new(root.join("assets"))
        .into_iter()
        .map(Result::unwrap)
        .filter(|entry| {
            entry.file_type().is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|ext| ext == "properties")
        })
        .map(|entry| {
            (
                entry.path().strip_prefix(root).unwrap().to_path_buf(),
                properties::parse(&disk::read(entry.path()).unwrap()).unwrap(),
            )
        })
        .collect()
}

fn fixture(root: &Path) {
    write(
        root,
        "pack.mcmeta",
        br#"{"pack":{"pack_format":1,"description":"Durability armor"}}"#,
    );
    for (id, item, max, splits, layer) in [
        (310, "helmet", 363, [0, 90, 180, 270, 363], 1),
        (311, "chestplate", 528, [0, 132, 264, 396, 528], 1),
        (312, "leggings", 495, [0, 123, 247, 371, 495], 2),
        (313, "boots", 429, [0, 108, 215, 322, 429], 1),
    ] {
        for stage in 1..=4 {
            let low = splits[4 - stage];
            let high = splits[5 - stage];
            let color = Rgba([stage as u8 * 50, id as u8, 20, 255]);
            let folder = "assets/minecraft/mcpatcher/cit/damaged armor";
            let armor = format!("{folder}/models/{stage}_{item}.properties");
            write(
                root,
                &armor,
                format!(
                    "type=armor\nitems={id}\ntexture.diamond_layer_{layer}=./{stage}_{item}\ndamage={low}-{high}\nweight=7\n"
                ),
            );
            RgbaImage::from_pixel(64, 32, color)
                .save(root.join(armor).with_extension("png"))
                .unwrap();
            let icon = format!("{folder}/items/{stage}_{item}.properties");
            write(
                root,
                &icon,
                format!(
                    "type=item\nmatchItems={id}\ntexture=../models/{stage}_{item}\ndamage={low}-{high}\n"
                ),
            );
        }
        assert_eq!(splits[4], max);
    }
}

fn resolved(root: &Path, value: &str) -> PathBuf {
    let (namespace, name) = value.split_once(':').unwrap();
    root.join(format!("assets/{namespace}/{name}.png"))
}

#[test]
fn all_armor_slots_keep_every_damage_color_and_matching_icons() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fixture(root);
    let original = rules(root);
    let mut report = AssetReport::default();
    migrate(root, 34, &mut report).unwrap();
    let converted = rules(root);
    assert_eq!(converted.len(), 32);
    for (file, before) in &original {
        let (_, after) = converted
            .iter()
            .find(|(name, _)| {
                name.file_name() == file.file_name()
                    && name.parent().unwrap().file_name() == file.parent().unwrap().file_name()
            })
            .unwrap();
        let key = if before.contains_key("items") {
            "items"
        } else {
            "matchItems"
        };
        let id = before[key].parse::<u32>().unwrap();
        assert_eq!(after[key], format!("minecraft:{}", ITEMS[&id]));
        for (key, value) in before {
            if key != "items" && key != "matchItems" && !key.starts_with("texture") {
                assert_eq!(&after[key], value);
            }
        }
        let texture = after
            .iter()
            .find(|(key, _)| key.starts_with("texture"))
            .unwrap()
            .1;
        assert!(resolved(root, texture).is_file());
        assert!(
            texture
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"._-/ :".contains(&c))
        );
        assert!(!texture.contains(' '));
    }
    for (id, max) in [(310, 363), (311, 528), (312, 495), (313, 429)] {
        for damage in 0..=max {
            let selected =
                |entries: &BTreeMap<PathBuf, properties::Properties>, item: &str, kind: &str| {
                    entries
                        .iter()
                        .find(|(_, props)| {
                            let key = if props.contains_key("items") {
                                "items"
                            } else {
                                "matchItems"
                            };
                            let (low, high) = props["damage"].split_once('-').unwrap();
                            props[key] == item
                                && props["type"] == kind
                                && (low.parse::<u32>().unwrap()..=high.parse().unwrap())
                                    .contains(&damage)
                        })
                        .map(|(path, _)| path.file_name().unwrap().to_owned())
                };
            for kind in ["armor", "item"] {
                let before = selected(&original, &id.to_string(), kind);
                assert!(before.is_some());
                assert_eq!(
                    selected(&converted, &format!("minecraft:{}", ITEMS[&id]), kind),
                    before,
                    "{id}: damage={damage}, type={kind}"
                );
            }
        }
    }
    assert_eq!(report.converted.len(), 1);
    assert!(report.converted[0].contains("32 条 CIT"));
    assert!(
        report
            .warnings
            .iter()
            .any(|value| value.contains("需要客户端"))
    );
    let paths: BTreeMap<_, _> = walkdir::WalkDir::new(root)
        .into_iter()
        .map(Result::unwrap)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| {
            (
                entry.path().to_path_buf(),
                disk::read(entry.path()).unwrap(),
            )
        })
        .collect();
    let mut second = AssetReport::default();
    migrate(root, 34, &mut second).unwrap();
    assert!(second.converted.is_empty());
    for (path, bytes) in paths {
        assert_eq!(disk::read(path).unwrap(), bytes);
    }
}

#[test]
fn safe_names_do_not_merge_distinct_textures_and_keep_animation_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for (folder, color) in [("damaged armor", "red"), ("damaged_armor", "green")] {
        let prefix = format!("assets/minecraft/mcpatcher/cit/{folder}");
        write(
            root,
            &format!("{prefix}/armor.properties"),
            "type=armor\nitems=310-313\ntexture.diamond_layer_1=./Armor Color\ndamage=25-50%\ndamageMask=65535\nweight=2\n",
        );
        write(root, &format!("{prefix}/Armor Color.png"), color);
        write(
            root,
            &format!("{prefix}/Armor Color.png.mcmeta"),
            b"{\"animation\":{\"frametime\":2}}",
        );
    }
    let mut report = AssetReport::default();
    migrate(root, 34, &mut report).unwrap();
    let converted = rules(root);
    let mut colors = BTreeSet::new();
    for props in converted.values() {
        assert_eq!(props["items"].split_whitespace().count(), 4);
        assert_eq!(props["damage"], "25-50%");
        assert_eq!(props["damageMask"], "65535");
        let path = resolved(root, &props["texture.diamond_layer_1"]);
        colors.insert(disk::read(&path).unwrap());
        assert!(path.with_extension("png.mcmeta").is_file());
    }
    assert_eq!(colors, BTreeSet::from([b"red".to_vec(), b"green".to_vec()]));
}

#[test]
fn item_model_local_references_and_default_texture_survive_relocation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let prefix = "assets/minecraft/mcpatcher/cit/custom armor";
    write(
        root,
        &format!("{prefix}/icon.properties"),
        "items=310\nmodel=./models/icon\ndamage=1-12\n",
    );
    write(root, &format!("{prefix}/models/icon.json"), br##"{"parent":"builtin/generated","textures":{"layer0":"../Icon Color","particle":"#layer0"}}"##);
    write(root, &format!("{prefix}/Icon Color.png"), b"icon");
    write(
        root,
        &format!("{prefix}/default.properties"),
        "items=311\ndamage=10-20\n",
    );
    write(root, &format!("{prefix}/default.png"), b"default");
    migrate(root, 34, &mut AssetReport::default()).unwrap();
    for (file, props) in rules(root) {
        if let Some(model) = props.get("model") {
            let (namespace, name) = model.split_once(':').unwrap();
            let value: serde_json::Value = serde_json::from_slice(
                &disk::read(root.join(format!("assets/{namespace}/{name}.json"))).unwrap(),
            )
            .unwrap();
            assert_eq!(value["parent"], "builtin/generated");
            assert_eq!(value["textures"]["particle"], "#layer0");
            assert_eq!(
                disk::read(resolved(
                    root,
                    value["textures"]["layer0"].as_str().unwrap()
                ))
                .unwrap(),
                b"icon"
            );
        } else {
            assert_eq!(
                disk::read(root.join(file).with_extension("png")).unwrap(),
                b"default"
            );
        }
    }
}

#[test]
fn legacy_targets_and_existing_modern_rules_are_preserved() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fixture(root);
    migrate(root, 1, &mut AssetReport::default()).unwrap();
    assert!(
        root.join("assets/minecraft/mcpatcher/cit/damaged armor/models/1_helmet.properties")
            .is_file()
    );
    let file = "assets/minecraft/optifine/cit/existing.properties";
    write(
        root,
        file,
        "damage=10-20\nitems=minecraft:diamond_helmet\ntexture=minecraft:optifine/cit/existing\ntype=armor\n",
    );
    write(
        root,
        "assets/minecraft/optifine/cit/existing.png",
        b"existing",
    );
    let before = disk::read(root.join(file)).unwrap();
    migrate(root, 34, &mut AssetReport::default()).unwrap();
    assert_eq!(disk::read(root.join(file)).unwrap(), before);
    assert_eq!(
        disk::read(root.join("assets/minecraft/optifine/cit/existing.png")).unwrap(),
        b"existing"
    );
}

#[test]
fn archive_conversion_runs_cit_migration_without_modifying_input() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fixture(&source);
    let prefix = source.join("assets/minecraft/mcpatcher/cit/damaged armor/models");
    let rule = prefix.join("1_helmet.properties");
    disk::write(
        &rule,
        disk::read_to_string(&rule)
            .unwrap()
            .replace("./1_helmet", "./Durability Color.PNG"),
    )
    .unwrap();
    disk::rename(
        prefix.join("1_helmet.png"),
        prefix.join("Durability Color.PNG"),
    )
    .unwrap();
    let output = temp.path().join("converted.zip");
    let before = rules(&source);
    let report = crate::convert_pack(
        &source,
        &output,
        &crate::ConversionOptions {
            src_format: Some(1),
            tgt_format: 34,
            target_version: Some("1.21.1".into()),
            auto_netherite: false,
            heypixel_compat: false,
            auto_client_setup: false,
            ..Default::default()
        },
        |_, _| {},
    )
    .unwrap();
    assert_eq!(before, rules(&source));
    assert!(
        report
            .assets
            .converted
            .iter()
            .any(|value| value.contains("32 条 CIT")),
        "{:?}",
        report.assets
    );
    let mut archive = zip::ZipArchive::new(disk::File::open(output).unwrap()).unwrap();
    let mut count = 0;
    let mut helmet_texture = None;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).unwrap();
        if entry.name().contains("/optifine/cit/fastpack_legacy/")
            && entry.name().ends_with(".properties")
        {
            assert!(!entry.name().contains(' '));
            assert!(entry.name().contains("/optifine/cit/"));
            if entry.name().ends_with("/models/1_helmet.properties") {
                use std::io::Read;
                let mut entry = entry;
                let mut text = String::new();
                entry.read_to_string(&mut text).unwrap();
                helmet_texture = Some(
                    properties::parse(text.as_bytes()).unwrap()["texture.diamond_layer_1"].clone(),
                );
            }
            count += 1;
        }
    }
    assert_eq!(count, 32);
    let texture = helmet_texture.unwrap();
    let (namespace, path) = texture.split_once(':').unwrap();
    assert!(
        archive
            .by_name(&format!("assets/{namespace}/{path}.png"))
            .is_ok()
    );
}
