use super::*;
use image::Rgba;

#[test]
fn six_color_cycle_is_periodic_and_keeps_alpha() {
    assert_eq!(color(0.0), [255, 0, 0]);
    assert_eq!(color(1.0 / 3.0), [0, 255, 0]);
    assert_eq!(color(2.0 / 3.0), [0, 0, 255]);
    assert_eq!(color(1.0), color(0.0));
    let mut image = RgbaImage::new(4, 4);
    image.put_pixel(2, 2, Rgba([25, 50, 75, 128]));
    let (animated, metadata) = animate(&image, None, recolor).unwrap();
    assert_eq!(animated.dimensions(), (4, 96));
    let metadata: Value = serde_json::from_slice(&metadata).unwrap();
    assert_eq!(
        metadata["animation"]["frames"].as_array().unwrap().len(),
        24
    );
    for frame in 0..24 {
        assert_eq!(animated.get_pixel(2, frame * 4 + 2)[3], 128);
        assert_eq!(animated.get_pixel(0, frame * 4)[3], 0);
    }
    assert_ne!(animated.get_pixel(2, 2), animated.get_pixel(2, 34));
}

#[test]
fn outline_animation_preserves_original_frame_order_duration_and_pixels() {
    let mut image = RgbaImage::new(4, 8);
    image.put_pixel(1, 1, Rgba([12, 34, 56, 255]));
    image.put_pixel(2, 6, Rgba([98, 76, 54, 180]));
    let original = json!({"texture":{"blur":false},"animation":{"frametime":3,"frames":[1,{"index":0,"time":7}]}});
    let (animated, metadata) = animate(
        &image,
        Some(&serde_json::to_vec(&original).unwrap()),
        |image, color| {
            super::super::custom_items::outlined(
                image,
                &super::super::custom_items::Outline {
                    color,
                    width: 1,
                    rainbow: true,
                },
            )
        },
    )
    .unwrap();
    let metadata: Value = serde_json::from_slice(&metadata).unwrap();
    assert_eq!(metadata["texture"], original["texture"]);
    let timeline = metadata["animation"]["frames"].as_array().unwrap();
    let mut tick = 0;
    for entry in timeline {
        let index = entry["index"].as_u64().unwrap() as u32;
        let time = entry["time"].as_u64().unwrap() as u32;
        let original_frame = if tick % 10 < 3 { 1 } else { 0 };
        for y in 0..4 {
            for x in 0..4 {
                let before = image.get_pixel(x, original_frame * 4 + y);
                if before[3] > 0 {
                    assert_eq!(animated.get_pixel(x, index * 4 + y), before);
                }
            }
        }
        for step in tick..tick + time {
            assert_eq!(if step % 10 < 3 { 1 } else { 0 }, original_frame);
        }
        tick += time;
    }
    assert_eq!(tick, 120);
}

#[test]
fn crosshair_animation_is_local_and_fixed_color_removes_old_dynamic_resources() {
    for target in [1, 15, 18, 34, 100] {
        let temp = tempfile::tempdir().unwrap();
        let crosshair = crate::crosshair::Crosshair {
            name: "test".into(),
            image: RgbaImage::from_pixel(15, 15, Rgba([255, 255, 255, 120])),
        };
        let mut report = super::super::assets::AssetReport::default();
        crate::crosshair::apply(temp.path(), target, &crosshair, &mut report).unwrap();
        super::crosshair(temp.path(), target, &mut report).unwrap();
        let sprite = temp
            .path()
            .join("assets/minecraft/textures/gui/sprites/hud/crosshair.png");
        let custom = temp
            .path()
            .join("assets/minecraft/optifine/anim/fastpack_rainbow/crosshair.properties");
        if target >= 18 {
            let image = fs::open_image(&sprite).unwrap();
            assert_eq!(image.height(), 360);
            assert!(sprite.with_extension("png.mcmeta").pack_is_file());
        } else {
            let values = super::super::properties::parse(&fs::read(&custom).unwrap()).unwrap();
            assert_eq!(values["to"], "textures/gui/icons.png");
            assert_eq!(values["w"], "15");
            assert_eq!(values["h"], "15");
        }
        crate::crosshair::apply(temp.path(), target, &crosshair, &mut report).unwrap();
        assert!(!custom.pack_is_file());
        if target >= 18 {
            assert_eq!(fs::open_image(&sprite).unwrap().height(), 15);
            assert!(!sprite.with_extension("png.mcmeta").pack_is_file());
        }
    }
}

#[test]
fn rainbow_line_shader_has_clock_per_supported_version_and_can_return_to_static() {
    for (version, format) in &crate::tables().pack_formats {
        if *format < 7 {
            continue;
        }
        let temp = tempfile::tempdir().unwrap();
        let mut report = super::super::assets::AssetReport::default();
        super::super::line_colors::apply_animated(
            temp.path(),
            version,
            Some([12, 34, 56]),
            Some([90, 80, 70]),
            true,
            true,
            &mut report,
        )
        .unwrap();
        let base = temp.path().join("assets/minecraft/shaders/core");
        let shader = fs::read_to_string(base.join("rendertype_lines.vsh")).unwrap();
        assert!(shader.contains(GLSL), "{version}");
        assert!(
            shader.contains("uniform float GameTime;") || shader.contains("globals.glsl"),
            "{version}"
        );
        let program = base.join("rendertype_lines.json");
        if program.pack_is_file() {
            let value: Value = serde_json::from_slice(&fs::read(&program).unwrap()).unwrap();
            assert!(
                value["uniforms"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|value| value["name"] == "GameTime"),
                "{version}"
            );
        }
        super::super::line_colors::apply_animated(
            temp.path(),
            version,
            Some([12, 34, 56]),
            Some([90, 80, 70]),
            false,
            false,
            &mut report,
        )
        .unwrap();
        let shader = fs::read_to_string(base.join("rendertype_lines.vsh")).unwrap();
        assert!(!shader.contains(GLSL), "{version}");
        assert_eq!(shader.matches("vec4 fastpack_line_color(").count(), 1);
    }
}

#[test]
fn rainbow_glint_keeps_alpha_and_static_mode_uses_original_pattern() {
    for (source, target) in [(1, 1), (1, 15), (15, 34)] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let path = root.join("assets/minecraft/textures/misc/enchanted_item_glint.png");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = RgbaImage::from_fn(16, 16, |x, y| {
            Rgba([
                x as u8 * 12,
                y as u8 * 12,
                120,
                if x == 0 { 0 } else { 180 },
            ])
        });
        fs::save_image(&original, &path).unwrap();
        let mut report = super::super::assets::AssetReport::default();
        super::super::glint::rainbow(root, source, target, &mut report).unwrap();
        let out = root.join(if target < 13 {
            "assets/minecraft/textures/misc/enchanted_item_glint.png"
        } else {
            "assets/minecraft/textures/misc/enchanted_glint_item.png"
        });
        let image = fs::open_image(&out).unwrap().to_rgba8();
        for (a, b) in original.pixels().zip(image.pixels()) {
            assert_eq!(a[3], b[3]);
        }
        assert!(image.pixels().any(|pixel| pixel[0] > pixel[1]));
        assert!(image.pixels().any(|pixel| pixel[1] > pixel[0]));
        super::super::glint::migrate(root, target, target, Some([255, 0, 0]), &mut report).unwrap();
        let red = fs::open_image(out).unwrap().to_rgba8();
        assert!(red.pixels().all(|pixel| pixel[1] == 0 && pixel[2] == 0));
    }
}
