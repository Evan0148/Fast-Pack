use super::*;
use image::Rgba;
use serde_json::json;

fn pixels(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        Rgba([
            (x % 251) as u8,
            (y % 251) as u8,
            ((x + y) % 251) as u8,
            ((x * 3 + y) % 256) as u8,
        ])
    })
}
fn put(root: &Path, name: &str, image: &RgbaImage) {
    let path = root.join(format!("assets/minecraft/textures/entity/{name}.png"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    image.save(path).unwrap();
}
fn get(root: &Path, name: &str) -> RgbaImage {
    image::open(root.join(format!("assets/minecraft/textures/entity/{name}.png")))
        .unwrap()
        .to_rgba8()
}

#[test]
fn expansion_preserves_every_source_pixel_and_alpha_at_hd_resolutions() {
    for scale in [1, 2, 4] {
        for muzzle in [false, true] {
            let original = pixels(64 * scale, 32 * scale);
            let expanded = convert(&original, &mut None, true, muzzle)
                .unwrap()
                .unwrap();
            assert_eq!(expanded.dimensions(), (64 * scale, 64 * scale));
            assert_eq!(
                imageops::crop_imm(&expanded, 0, 0, original.width(), original.height()).to_image(),
                original
            );
            assert_eq!(expanded.get_pixel(63 * scale, 63 * scale)[3], 0);
            let restored = convert(&expanded, &mut None, false, muzzle)
                .unwrap()
                .unwrap();
            assert_eq!(restored, original);
        }
    }
}

#[test]
fn cow_muzzle_uses_pack_pixels_and_flattens_back_to_the_head() {
    let source = pixels(128, 64);
    let mut expanded = convert(&source, &mut None, true, true).unwrap().unwrap();
    for y in 0..6 {
        for x in 0..12 {
            assert_eq!(
                expanded.get_pixel(4 + x, 68 + y),
                source.get_pixel(14 + x, 22 + y)
            );
        }
    }
    expanded.put_pixel(4, 68, Rgba([177, 33, 55, 200]));
    let restored = convert(&expanded, &mut None, false, true).unwrap().unwrap();
    assert_eq!(*restored.get_pixel(14, 22), Rgba([177, 33, 55, 200]));
}

#[test]
fn explicit_animation_frames_keep_order_timing_and_independent_uvs() {
    let first = pixels(128, 64);
    let second = RgbaImage::from_pixel(128, 64, Rgba([5, 66, 77, 90]));
    let mut source = RgbaImage::new(128, 128);
    imageops::replace(&mut source, &first, 0, 0);
    imageops::replace(&mut source, &second, 0, 64);
    let original_meta = json!({"animation":{"width":128,"height":64,"frametime":7,"interpolate":true,"frames":[1,{"index":0,"time":13}]},"texture":{"blur":false,"clamp":true}});
    let mut meta = Some(original_meta.clone());
    let expanded = convert(&source, &mut meta, true, true).unwrap().unwrap();
    assert_eq!(expanded.dimensions(), (128, 256));
    assert_eq!(meta.as_ref().unwrap()["animation"]["height"], 128);
    assert_eq!(
        meta.as_ref().unwrap()["animation"]["frames"],
        original_meta["animation"]["frames"]
    );
    assert_eq!(meta.as_ref().unwrap()["texture"], original_meta["texture"]);
    assert_eq!(*expanded.get_pixel(4, 128 + 68), Rgba([5, 66, 77, 90]));
    let restored = convert(&expanded, &mut meta, false, true).unwrap().unwrap();
    assert_eq!(restored, source);
    assert_eq!(meta, Some(original_meta));
}

#[test]
fn every_supported_target_gets_the_native_path_and_layout() {
    for (version, format) in &crate::tables().pack_formats {
        let work = tempfile::tempdir().unwrap();
        let root = work.path();
        for family in FAMILIES {
            put(root, family.legacy[0], &pixels(128, 64));
        }
        let mut report = AssetReport::default();
        migrate(root, 1, *format, &mut report).unwrap();
        assert!(
            report.warnings.is_empty(),
            "{version}: {:?}",
            report.warnings
        );
        for family in FAMILIES {
            assert_eq!(
                get(root, family.target(*format)).dimensions(),
                (128, if *format >= 55 { 128 } else { 64 }),
                "{version}"
            );
        }
        migrate(root, *format, 1, &mut report).unwrap();
        for family in FAMILIES {
            assert_eq!(get(root, family.legacy[0]), pixels(128, 64));
        }
    }
}

#[test]
fn broken_previous_outputs_are_repaired_but_correct_targets_and_other_animals_survive() {
    let work = tempfile::tempdir().unwrap();
    let root = work.path();
    put(root, "cow/temperate_cow", &pixels(128, 64));
    let native = pixels(128, 128);
    put(root, "pig/temperate_pig", &native);
    let sheep = pixels(128, 64);
    put(root, "sheep/sheep", &sheep);
    let pig = root.join("assets/minecraft/textures/entity/pig/temperate_pig.png");
    let before = std::fs::read(&pig).unwrap();
    let mut report = AssetReport::default();
    migrate(root, 75, 75, &mut report).unwrap();
    assert_eq!(get(root, "cow/temperate_cow").dimensions(), (128, 128));
    assert_eq!(std::fs::read(&pig).unwrap(), before);
    assert_eq!(get(root, "sheep/sheep"), sheep);
    let file = root.join("assets/minecraft/textures/entity/cow/temperate_cow.png");
    let fixed = std::fs::read(&file).unwrap();
    migrate(root, 75, 75, &mut report).unwrap();
    assert_eq!(std::fs::read(file).unwrap(), fixed);
    assert!(
        root.join(
            "fastpack/original-assets/assets/minecraft/textures/entity/cow/temperate_cow.png"
        )
        .is_file()
    );
}

#[test]
fn native_modern_only_pack_downgrades_without_an_old_alias() {
    for format in [55, 75, 84, 100] {
        let work = tempfile::tempdir().unwrap();
        let root = work.path();
        for family in FAMILIES {
            put(root, family.target(format), &pixels(128, 128));
        }
        migrate(root, format, 1, &mut AssetReport::default()).unwrap();
        for family in FAMILIES {
            assert_eq!(get(root, family.legacy[0]).dimensions(), (128, 64));
        }
    }
}

#[test]
fn custom_entity_models_and_invalid_layouts_are_not_guessed() {
    let work = tempfile::tempdir().unwrap();
    let root = work.path();
    put(root, "pig/pig", &pixels(128, 128));
    let model = root.join("assets/minecraft/optifine/cem/pig.jem");
    std::fs::create_dir_all(model.parent().unwrap()).unwrap();
    std::fs::write(model, br#"{"textureSize":[64,64]}"#).unwrap();
    put(root, "cow/temperate_cow", &pixels(128, 20));
    let mut report = AssetReport::default();
    migrate(root, 1, 75, &mut report).unwrap();
    assert!(
        !root
            .join("assets/minecraft/textures/entity/pig/temperate_pig.png")
            .exists()
    );
    assert_eq!(get(root, "cow/temperate_cow").dimensions(), (128, 20));
    assert_eq!(report.warnings.len(), 1);
}
