use super::*;

#[test]
fn builtin_name_highlight_overlay_needs_no_import_and_does_not_enable_other_effects() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("names.zip");
    let options = Options {
        version: "1.21.10".into(),
        builtin_overlays: crate::builtin_overlays::Settings {
            features: vec![crate::builtin_overlays::Feature::HighlightNames],
        },
        ..Default::default()
    };
    build(&output, &options, |_, _| {}).unwrap();
    let files = unpack(&output);
    assert!(files.contains_key("assets/minecraft/shaders/core/rendertype_text_see_through.fsh"));
    assert!(!files.keys().any(|name| name.contains("swordhud")
        || name.contains("rendertype_lines")
        || name.contains("/gui/")
        || name.contains("glint")));
    assert!(files.contains_key("fastpack/builtin-overlays/NOTICE.txt"));
}
use std::collections::BTreeMap;
use std::io::Read;

fn unpack(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..zip.len())
        .map(|index| {
            let mut entry = zip.by_index(index).unwrap();
            let name = entry.name().to_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            (name, bytes)
        })
        .collect()
}

#[test]
fn colored_description_preserves_unicode_selections_and_repeated_text_edits() {
    let mut description = Description::plain("中文 AAA\n第二行");
    description.paint(0..2, Some([255, 0, 0]));
    description.paint(3..4, Some([0, 255, 0]));
    description
        .edit("中文 AAAA\n第二行".into(), 4, [0, 0, 255])
        .unwrap();
    assert_eq!(description.colors[3], Some([0, 255, 0]));
    assert_eq!(description.colors[4], Some([0, 0, 255]));
    assert_eq!(description.colors[5], None);
    let modern = description.component("1.20.1");
    assert_eq!(
        modern["extra"][0],
        json!({"text":"中文", "color":"#FF0000"})
    );
    let legacy = description.component("1.8.9").as_str().unwrap().to_string();
    assert!(legacy.starts_with("§4中文"));
    assert!(legacy.ends_with("\n第二行"));
    assert_eq!(Description::default().component("1.20.1"), json!(""));
}

#[test]
fn native_formatting_codes_and_styles_export_without_leaking_into_other_runs() {
    let mut text = Description::plain("红色加粗\n普通");
    text.paint(0..4, Some([255, 85, 85]));
    for format in [
        Format::Bold,
        Format::Italic,
        Format::Underlined,
        Format::Strikethrough,
        Format::Obfuscated,
    ] {
        text.format(0..4, format, true);
    }
    let legacy = text.component("1.8.9");
    assert_eq!(legacy, json!("§c§l§o§n§m§k红色加粗§r\n普通"));
    let modern = text.component("1.21.10");
    for key in [
        "bold",
        "italic",
        "underlined",
        "strikethrough",
        "obfuscated",
    ] {
        assert_eq!(modern["extra"][0][key], true);
    }
    assert_eq!(modern["extra"][1], json!({"text":"\n普通"}));
    text.reset(0..4);
    assert_eq!(
        text.component("1.21.10"),
        json!({"text":"","extra":[{"text":"红色加粗\n普通"}]})
    );
    let mut codes = Description::plain("§c红§l粗§a绿§r原");
    codes.materialize_codes();
    assert_eq!(codes.text, "红粗绿原");
    assert_eq!(codes.styles[1].bold, true);
    assert_eq!(
        codes.styles[2].bold, false,
        "a color code resets prior font styles"
    );
    assert_eq!(
        codes.colors,
        [
            Some([255, 85, 85]),
            Some([255, 85, 85]),
            Some([85, 255, 85]),
            None
        ]
    );
    let unknown = Description::plain("§q普通");
    assert_eq!(unknown.formatted_runs("1.20.1")[0].text, "§q普通");
}

#[test]
fn formatted_pack_names_are_real_filename_colors_and_stay_within_windows_limits() {
    let mut name = Description::plain("我的覆盖包");
    name.paint(0..5, Some([255, 85, 85]));
    name.format(0..5, Format::Bold, true);
    let filename = formatted_filename(&name, "1.20.1", true);
    assert_eq!(filename, "[1.20.1] §c§l我的覆盖包§r.zip");
    let decoded = Description::plain(filename).formatted_runs("1.8.9");
    assert!(
        decoded.iter().any(|run| run.text == "我的覆盖包"
            && run.style.bold
            && run.color == Some([255, 85, 85]))
    );
    let mut long = Description::plain("😀".repeat(80));
    for index in 0..80 {
        long.paint(index..index + 1, Some(LEGACY_COLORS[index % 16]));
        long.format(index..index + 1, Format::Bold, index % 2 == 0);
    }
    let filename = formatted_filename(&long, "1.21.10", true);
    assert!(filename.encode_utf16().count() <= 240);
    assert!(filename.ends_with("§r.zip"));
    assert!(!formatted_filename(&Description::plain("../*?:"), "1.8.9", true).contains('/'));
}

#[test]
fn metadata_only_overlays_never_add_game_textures_or_overwrite_outputs() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("overlay.zip");
    let options = Options::default();
    let report = build(&output, &options, |_, _| {}).unwrap();
    assert_eq!(report.files, 2);
    let files = unpack(&output);
    assert_eq!(
        files.keys().map(String::as_str).collect::<Vec<_>>(),
        ["pack.mcmeta", "pack.png"]
    );
    let original = std::fs::read(&output).unwrap();
    assert!(build(&output, &options, |_, _| {}).is_err());
    assert_eq!(std::fs::read(&output).unwrap(), original);
    let folder = directory.path().join("folder");
    build(
        &folder,
        &Options {
            as_zip: false,
            ..options
        },
        |_, _| {},
    )
    .unwrap();
    assert!(folder.join("pack.mcmeta").is_file());
    assert_eq!(std::fs::read_dir(folder).unwrap().count(), 2);
}

#[test]
fn every_supported_version_generates_native_overlay_resources() {
    let directory = tempfile::tempdir().unwrap();
    let crosshair = crate::crosshair::builtin_presets()
        .unwrap()
        .remove(0)
        .crosshair;
    for (index, (version, format)) in crate::tables().pack_formats.iter().enumerate() {
        let output = directory.path().join(format!("{index}.zip"));
        let mut description = Description::plain("彩色简介\n第二行");
        description.paint(0..4, Some([19, 182, 231]));
        let options = Options {
            version: version.clone(),
            description,
            glint_color: Some([22, 44, 88]),
            crosshair: Some(crosshair.clone()),
            fishing_line_color: (*format >= 7).then_some([12, 34, 56]),
            block_outline_color: (*format >= 7).then_some([56, 34, 12]),
            custom_items: vec![crate::compat::custom_items::Rule {
                version: version.clone(),
                item: "minecraft:diamond_sword".into(),
                name: Some("自定义剑".into()),
                inventory_scale: if version == "1.7.10" { 100 } else { 85 },
                ..Default::default()
            }],
            ..Default::default()
        };
        let report = build(&output, &options, |_, _| {})
            .unwrap_or_else(|error| panic!("{version}: {error:#}"));
        assert!(
            report.assets.warnings.is_empty(),
            "{version}: {:?}",
            report.assets.warnings
        );
        let files = unpack(&output);
        let meta: Value = serde_json::from_slice(&files["pack.mcmeta"]).unwrap();
        assert_eq!(meta["pack"]["pack_format"], json!(format), "{version}");
        assert_eq!(
            meta["pack"]["description"],
            options.description.component(version),
            "{version}"
        );
        if *format >= crate::NEW_META_MIN_FORMAT {
            assert_eq!(
                meta["pack"]["min_format"],
                json!(crate::tables::format_components(Some(version), *format))
            );
            assert_eq!(meta["pack"]["max_format"], meta["pack"]["min_format"]);
        }
        let gui = if *format >= 18 {
            "assets/minecraft/textures/gui/sprites/hud/crosshair.png"
        } else {
            "assets/minecraft/textures/gui/icons.png"
        };
        assert!(
            files.contains_key(gui),
            "{version}: missing native crosshair"
        );
        let glint = if *format >= 13 {
            "assets/minecraft/textures/misc/enchanted_glint_item.png"
        } else {
            "assets/minecraft/textures/misc/enchanted_item_glint.png"
        };
        assert!(files.contains_key(glint), "{version}: missing native glint");
        if version != "1.7.10" {
            let model = if *format >= 46 {
                "assets/minecraft/items/diamond_sword.json"
            } else {
                "assets/minecraft/models/item/diamond_sword.json"
            };
            assert!(
                files.contains_key(model),
                "{version}: missing native item model"
            );
        }
        for (name, bytes) in &files {
            if name.ends_with(".json") || name.ends_with(".mcmeta") {
                serde_json::from_slice::<Value>(bytes)
                    .unwrap_or_else(|error| panic!("{version}/{name}: {error}"));
            }
        }
        assert!(!files.keys().any(|name| name.contains("netherite")
            || name.contains("fastpack_via")
            || name.contains("/sky/")));
    }
}

#[test]
fn individual_features_do_not_install_unselected_hud_or_glint_resources() {
    let directory = tempfile::tempdir().unwrap();
    for (index, options) in [
        Options {
            glint_color: Some([1, 2, 3]),
            ..Default::default()
        },
        Options {
            fishing_line_color: Some([1, 2, 3]),
            ..Default::default()
        },
        Options {
            line_widths: crate::compat::line_colors::Widths {
                fishing: Some(2.0),
                outline: Some(3.0),
            },
            ..Default::default()
        },
    ]
    .into_iter()
    .enumerate()
    {
        let output = directory.path().join(format!("{index}.zip"));
        build(&output, &options, |_, _| {}).unwrap();
        let files = unpack(&output);
        assert!(!files.keys().any(|name| name.contains("/gui/")
            || name.contains("/models/")
            || name.contains("/items/")));
        if index >= 1 {
            assert!(!files.keys().any(|name| name.contains("glint")));
        }
        if index == 2 {
            let shader =
                std::str::from_utf8(&files["assets/minecraft/shaders/core/rendertype_lines.vsh"])
                    .unwrap();
            assert!(shader.contains("return 2.000;") && shader.contains("return 3.000;"));
            let cache: Value = serde_json::from_slice(&files["fastpack/line-colors.json"]).unwrap();
            assert!(cache["fishing"].is_null() && cache["outline"].is_null());
        }
    }
}

#[test]
fn icon_transparency_and_shape_survive_and_invalid_input_leaves_no_output() {
    let image = RgbaImage::from_pixel(40, 20, Rgba([12, 34, 56, 170]));
    let mut png = Cursor::new(Vec::new());
    image.write_to(&mut png, ImageFormat::Png).unwrap();
    let icon = prepare_icon(png.get_ref()).unwrap();
    let image = image::load_from_memory(&icon).unwrap().to_rgba8();
    assert_eq!(image.dimensions(), (128, 128));
    assert_eq!(image.get_pixel(0, 0)[3], 0);
    assert_eq!(image.get_pixel(64, 64).0, [12, 34, 56, 170]);
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("invalid.zip");
    assert!(
        build(
            &output,
            &Options {
                icon: Some(vec![0, 1, 2]),
                ..Default::default()
            },
            |_, _| {}
        )
        .is_err()
    );
    assert!(!output.exists());
    assert!(!filename("../CON:*?", "1.8.9", true).contains('/'));
}
