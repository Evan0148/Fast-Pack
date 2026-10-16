use super::{Layer, Preview, atlas, layout, ticks};
use anyhow::Result;
use image::RgbaImage;
use std::collections::BTreeMap;

fn alpha(layer: &Layer) -> Option<u16> {
    let p = &layer.properties;
    let timed = ["startFadeIn", "endFadeIn", "startFadeOut", "endFadeOut"]
        .iter()
        .any(|name| p.contains_key(*name));
    if !timed {
        let hint = format!(
            "{} {} {}",
            layer.name,
            layer.texture,
            p.get("source").map_or("", String::as_str)
        )
        .to_lowercase();
        let day = ["day", "noon", "白天", "日间"]
            .iter()
            .any(|word| hint.contains(word));
        let night = [
            "night", "sunset", "sunrise", "dusk", "dawn", "star", "moon", "夜", "黄昏", "日落",
            "日出",
        ]
        .iter()
        .any(|word| hint.contains(word));
        return (!night || day).then_some(1000);
    }
    let a = ticks(p.get("startFadeIn")?).ok()?;
    let b = ticks(p.get("endFadeIn")?).ok()?;
    let d = ticks(p.get("endFadeOut")?).ok()?;
    let rise = (b - a).rem_euclid(24000);
    let c = p
        .get("startFadeOut")
        .map(|value| ticks(value).ok())
        .unwrap_or_else(|| Some((d - rise).rem_euclid(24000)))?;
    let plateau = (c - b).rem_euclid(24000);
    let fall = (d - c).rem_euclid(24000);
    let span = rise + plateau + fall;
    if span == 0 || span > 24000 {
        return None;
    }
    let at = (6000 - a).rem_euclid(24000);
    let value = if at >= span {
        0
    } else if at < rise {
        at * 1000 / rise.max(1)
    } else if at <= rise + plateau {
        1000
    } else {
        (span - at) * 1000 / fall.max(1)
    };
    (value > 0).then_some(value as u16)
}

pub(super) fn compose(
    layers: Vec<Layer>,
    mut read: impl FnMut(&Layer) -> Result<RgbaImage>,
) -> Result<Option<Preview>> {
    if let Some(preview) = select(&layers, &mut read, true)? {
        return Ok(Some(preview));
    }
    select(&layers, &mut read, false)
}

fn select(
    layers: &[Layer],
    read: &mut impl FnMut(&Layer) -> Result<RgbaImage>,
    daytime: bool,
) -> Result<Option<Preview>> {
    let eligible: Vec<_> = layers
        .iter()
        .filter(|layer| !layer.decoration && (!daytime || layer.world == "world0"))
        .filter_map(|layer| {
            if daytime { alpha(layer) } else { Some(1000) }.map(|alpha| (layer, alpha))
        })
        .collect();
    let names: Vec<_> = eligible
        .iter()
        .map(|(layer, _)| layer.name.clone())
        .collect();
    let mut images = BTreeMap::new();
    let mut selected = None;
    for (layer, alpha) in eligible {
        let blend = match layer.properties.get("blend").map(String::as_str) {
            Some("multiply" | "burn" | "subtract") => continue,
            Some("replace" | "alpha") => 3,
            Some("screen") => 2,
            _ => 1,
        };
        let key = (layer.texture.clone(), layer.layout.clone());
        if !images.contains_key(&key) {
            let Ok(image) = read(layer) else { continue };
            let Ok(size) = atlas::dimensions(&image) else {
                continue;
            };
            let Ok(image) = atlas::resize_faces(&image, size.min(256)) else {
                continue;
            };
            let Ok(image) = atlas::remap(
                &image,
                layout(&layer.layout),
                atlas::AtlasLayout::Optifine,
                layer.world == "world0",
            ) else {
                continue;
            };
            images.insert(key.clone(), image);
        }
        let image = &images[&key];
        let thumbnail = image::imageops::thumbnail(image, 192, 128);
        let count = thumbnail.width() as usize * thumbnail.height() as usize;
        let visible = thumbnail
            .pixels()
            .filter(|pixel| pixel[3] > 16 && pixel[0].max(pixel[1]).max(pixel[2]) > 12)
            .count();
        if visible * 20 <= count {
            continue;
        }
        let general = !["biomes", "heights", "days"]
            .iter()
            .any(|name| layer.properties.contains_key(*name));
        let clear = layer
            .properties
            .get("weather")
            .is_none_or(|weather| weather.split_whitespace().any(|value| value == "clear"));
        let day = format!(
            "{} {} {}",
            layer.name,
            layer.texture,
            layer.properties.get("source").map_or("", String::as_str)
        )
        .to_lowercase();
        let named = ["day", "noon", "白天", "日间"]
            .iter()
            .any(|word| day.contains(word));
        let brightness: u64 = thumbnail
            .pixels()
            .map(|pixel| u64::from(pixel[0].max(pixel[1]).max(pixel[2])) * u64::from(pixel[3]))
            .sum::<u64>()
            / count.max(1) as u64;
        let score = (
            layer.world == "world0",
            general && clear,
            named,
            blend,
            alpha,
            visible * 1000 / count.max(1),
            brightness,
        );
        if selected
            .as_ref()
            .is_none_or(|(previous, _)| score > *previous)
        {
            let selected_layer = layer
                .properties
                .get("source")
                .and_then(|source| std::path::Path::new(source).file_stem())
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| layer.name.clone());
            selected = Some((
                score,
                Preview {
                    names: names.clone(),
                    selected_layer,
                    daytime,
                    image: image.clone(),
                },
            ));
        }
    }
    Ok(selected.map(|(_, preview)| preview))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn layer(name: &str, from: &str, until: &str) -> Layer {
        Layer {
            name: name.into(),
            world: "world0".into(),
            texture: format!("assets/test/{name}.png"),
            layout: "optifine".into(),
            properties: BTreeMap::from([
                ("startFadeIn".into(), from.into()),
                ("endFadeIn".into(), from.into()),
                ("startFadeOut".into(), until.into()),
                ("endFadeOut".into(), until.into()),
            ]),
            original: format!("assets/test/optifine/sky/world0/{name}.properties"),
            decoration: false,
        }
    }

    #[test]
    fn selects_noon_and_never_reads_dusk_night_or_other_dimensions() {
        let mut nether = layer("nether", "06:00", "18:00");
        nether.world = "world-1".into();
        let layers = vec![
            layer("sunset", "17:00", "20:00"),
            layer("night", "19:00", "05:00"),
            nether,
            layer("day", "06:00", "18:00"),
        ];
        let mut read_names = Vec::new();
        let preview = compose(layers, |layer| {
            read_names.push(layer.name.clone());
            Ok(RgbaImage::from_pixel(12, 8, Rgba([40, 120, 220, 255])))
        })
        .unwrap()
        .unwrap();
        assert_eq!(preview.selected_layer, "day");
        assert_eq!(read_names, ["day"]);
        assert_eq!(preview.names, ["day"]);
    }

    #[test]
    fn no_daytime_layer_falls_back_to_an_available_other_time() {
        let result = compose(vec![layer("night", "19:00", "05:00")], |_| {
            Ok(RgbaImage::from_pixel(12, 8, Rgba([40, 60, 120, 255])))
        })
        .unwrap()
        .unwrap();
        assert_eq!(result.selected_layer, "night");
        assert!(!result.daytime);
    }

    #[test]
    fn implied_fade_out_and_midnight_wrapping_obey_noon() {
        let mut night = layer("night", "19:00", "05:00");
        night.properties.insert("endFadeIn".into(), "20:00".into());
        night.properties.remove("startFadeOut");
        assert_eq!(alpha(&night), None);
        let mut day = layer("day", "05:30", "18:40");
        day.properties.insert("endFadeIn".into(), "06:00".into());
        day.properties.remove("startFadeOut");
        assert_eq!(alpha(&day), Some(1000));
    }

    #[test]
    fn previews_are_scoped_to_each_pack_even_with_identical_layer_names() {
        let layers = vec![layer("sky1", "06:00", "18:00")];
        let a = compose(layers.clone(), |_| {
            Ok(RgbaImage::from_pixel(12, 8, Rgba([200, 40, 20, 255])))
        })
        .unwrap()
        .unwrap();
        let b = compose(layers, |_| {
            Ok(RgbaImage::from_pixel(12, 8, Rgba([20, 80, 220, 255])))
        })
        .unwrap()
        .unwrap();
        assert_ne!(a.image, b.image);
    }

    #[test]
    fn active_daytime_texture_overrides_old_cache_for_folders_and_zips() {
        use std::fs;
        use std::io::Write;
        use zip::{ZipWriter, write::SimpleFileOptions};
        let work = tempfile::tempdir().unwrap();
        let root = work.path().join("pack");
        let directory = root.join("assets/minecraft/optifine/sky/world0");
        fs::create_dir_all(&directory).unwrap();
        fs::write(root.join("pack.mcmeta"), br#"{"pack":{"pack_format":15}}"#).unwrap();
        let blue = RgbaImage::from_pixel(12, 8, Rgba([20, 90, 220, 255]));
        blue.save(directory.join("day.png")).unwrap();
        fs::write(directory.join("sky2.properties"), b"source=./day.png\nstartFadeIn=06:00\nendFadeIn=07:00\nstartFadeOut=17:00\nendFadeOut=18:00\n").unwrap();
        let cached = root.join("fastpack/canonical-sky");
        fs::create_dir_all(&cached).unwrap();
        RgbaImage::from_pixel(12, 8, Rgba([220, 40, 10, 255]))
            .save(cached.join("old.png"))
            .unwrap();
        let mut old = layer("old_day", "06:00", "18:00");
        old.texture = "fastpack/canonical-sky/old.png".into();
        fs::write(
            cached.join("layers.json"),
            serde_json::to_vec(&vec![old]).unwrap(),
        )
        .unwrap();
        let folder = super::super::preview(&root).unwrap().unwrap();
        assert_eq!(folder.selected_layer, "day");
        assert_eq!(folder.image, blue);
        let target = work.path().join("pack.zip");
        let mut zip = ZipWriter::new(fs::File::create(&target).unwrap());
        for entry in walkdir::WalkDir::new(&root)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
        {
            let name = entry
                .path()
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(&fs::read(entry.path()).unwrap()).unwrap();
        }
        zip.finish().unwrap();
        let archive = super::super::preview(&target).unwrap().unwrap();
        assert_eq!(archive.selected_layer, "day");
        assert_eq!(archive.image, folder.image);
    }
}
