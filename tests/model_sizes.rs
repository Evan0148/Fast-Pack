use fast_pack::{ConversionOptions, convert_pack};
use image::{Rgba, RgbaImage};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

fn write(root: &Path, name: &str, bytes: &[u8]) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

#[test]
fn edited_size_models_with_bom_keep_custom_textures_and_pose() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let output = temp.path().join("output");
    write(
        &source,
        "pack.mcmeta",
        br#"{"pack":{"pack_format":1,"description":"Sized custom model"}}"#,
    );
    let model = json!({"parent":"builtin/generated","textures":{"layer0":"demo:items/rod"},"display":{"firstperson":{"scale":[0.55,0.55,0.55]},"gui":{"scale":[0.7,0.7,0.7]}}});
    let mut bytes = vec![0xef, 0xbb, 0xbf];
    bytes.extend(serde_json::to_vec(&model).unwrap());
    write(&source, "assets/demo/models/item/small_rod.json", &bytes);
    write(&source,"assets/minecraft/models/item/fishing_rod.json",br#"{"parent":"item/handheld","textures":{"layer0":"demo:items/rod"},"overrides":[{"predicate":{"cast":1},"model":"demo:item/small_rod"}]}"#);
    let png = source.join("assets/demo/textures/items/rod.png");
    fs::create_dir_all(png.parent().unwrap()).unwrap();
    RgbaImage::from_pixel(16, 16, Rgba([23, 87, 125, 255]))
        .save(png)
        .unwrap();
    convert_pack(
        &source,
        &output,
        &ConversionOptions {
            src_format: Some(1),
            tgt_format: 15,
            target_version: Some("1.20.1".into()),
            as_zip: false,
            auto_netherite: false,
            auto_client_setup: false,
            ..Default::default()
        },
        |_, _| {},
    )
    .unwrap();
    let converted: Value = serde_json::from_slice(
        &fs::read(output.join("assets/demo/models/item/small_rod.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(converted["textures"]["layer0"], "demo:item/rod");
    assert_eq!(converted["display"]["gui"]["scale"], json!([0.7, 0.7, 0.7]));
    assert!(converted["display"].get("firstperson_righthand").is_some());
    assert!(output.join("assets/demo/textures/item/rod.png").is_file());
}

#[test]
fn modern_block_items_retain_authored_item_models_without_overwriting_world_models() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let output = temp.path().join("output");
    write(
        &source,
        "pack.mcmeta",
        br#"{"pack":{"pack_format":15,"description":"Small block items"}}"#,
    );
    let model = json!({"parent":"minecraft:block/stone","display":{"gui":{"scale":[0.4,0.4,0.4]}}});
    write(
        &source,
        "assets/minecraft/models/item/stone.json",
        &serde_json::to_vec(&model).unwrap(),
    );
    convert_pack(
        &source,
        &output,
        &ConversionOptions {
            src_format: Some(15),
            tgt_format: 69,
            target_version: Some("1.21.10".into()),
            as_zip: false,
            auto_netherite: false,
            auto_client_setup: false,
            ..Default::default()
        },
        |_, _| {},
    )
    .unwrap();
    let definition: Value = serde_json::from_slice(
        &fs::read(output.join("assets/minecraft/items/stone.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(definition["model"]["model"], "minecraft:item/stone");
    let actual: Value = serde_json::from_slice(
        &fs::read(output.join("assets/minecraft/models/item/stone.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(actual["parent"], "minecraft:block/stone");
    assert_eq!(actual["display"]["gui"]["scale"], json!([0.4, 0.4, 0.4]));
    assert!(
        !output
            .join("assets/minecraft/models/block/stone.json")
            .exists()
    );
}
