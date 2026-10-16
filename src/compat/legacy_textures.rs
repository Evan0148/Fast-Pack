use super::assets::{AssetReport, open, save};
use anyhow::Result;
use image::{RgbaImage, imageops};
use serde_json::json;
use std::fs;
use std::path::Path;

const ALIASES: &[(&str, &str)] = &[
    ("blocks/tallgrass", "block/short_grass"),
    ("blocks/command_block", "block/command_block_front"),
    ("blocks/door_acacia_lower", "block/acacia_door_bottom"),
    ("blocks/door_acacia_upper", "block/acacia_door_top"),
    ("blocks/door_birch_lower", "block/birch_door_bottom"),
    ("blocks/door_birch_upper", "block/birch_door_top"),
    ("blocks/door_dark_oak_lower", "block/dark_oak_door_bottom"),
    ("blocks/door_dark_oak_upper", "block/dark_oak_door_top"),
    ("blocks/door_jungle_lower", "block/jungle_door_bottom"),
    ("blocks/door_jungle_upper", "block/jungle_door_top"),
    ("blocks/door_spruce_lower", "block/spruce_door_bottom"),
    ("blocks/door_spruce_upper", "block/spruce_door_top"),
    ("blocks/flower_paeonia", "block/peony_bottom"),
    ("blocks/iron_bars", "block/iron_bars"),
    ("blocks/iron_trapdoor", "block/iron_trapdoor"),
    ("blocks/prismarine_dark", "block/dark_prismarine"),
    (
        "blocks/red_sandstone_carved",
        "block/chiseled_red_sandstone",
    ),
    ("blocks/red_sandstone_normal", "block/red_sandstone"),
    ("blocks/red_sandstone_smooth", "block/cut_red_sandstone"),
    ("blocks/sapling_roofed_oak", "block/dark_oak_sapling"),
    ("blocks/slime", "block/slime_block"),
    ("blocks/sponge_wet", "block/wet_sponge"),
    ("entity/alex", "entity/player/slim/alex"),
    ("entity/steve", "entity/player/wide/steve"),
    ("entity/arrow", "entity/projectiles/arrow"),
    ("entity/iron_golem", "entity/iron_golem/iron_golem"),
    ("entity/sign", "entity/signs/oak"),
    ("entity/snowman", "entity/snow_golem"),
    ("entity/squid", "entity/squid/squid"),
    ("entity/cow/mooshroom", "entity/cow/red_mooshroom"),
    (
        "entity/endercrystal/endercrystal",
        "entity/end_crystal/end_crystal",
    ),
    (
        "entity/endercrystal/endercrystal_beam",
        "entity/end_crystal/end_crystal_beam",
    ),
];

fn source(root: &Path, name: &str) -> Option<std::path::PathBuf> {
    let (kind, short) = name.split_once('/')?;
    let legacy = crate::tables::legacy_texture_name(short, kind).unwrap_or_else(|| short.into());
    [
        format!("{kind}/{short}"),
        format!("{kind}s/{legacy}"),
        format!("{kind}s/{short}"),
    ]
    .into_iter()
    .map(|name| root.join(format!("{name}.png")))
    .find(|p| p.is_file())
}

pub(super) fn migrate(
    root: &Path,
    source_format: u32,
    target: u32,
    report: &mut AssetReport,
) -> Result<()> {
    if source_format < 4 || target >= 2 {
        return Ok(());
    }
    let textures = root.join("assets/minecraft/textures");
    let mut count = 0;
    for (legacy, modern) in ALIASES {
        let output = textures.join(format!("{legacy}.png"));
        if output.is_file() {
            continue;
        }
        if let Some(input) = source(&textures, modern) {
            fs::create_dir_all(output.parent().unwrap())?;
            fs::copy(&input, &output)?;
            if input.with_extension("png.mcmeta").is_file() {
                fs::copy(
                    input.with_extension("png.mcmeta"),
                    output.with_extension("png.mcmeta"),
                )?;
            }
            count += 1;
        }
    }
    for (name, frames) in [("clock", 64), ("compass", 32)] {
        let output = textures.join(format!("items/{name}.png"));
        if output.is_file() {
            continue;
        }
        let mut provided = Vec::new();
        for index in 0..frames {
            if let Some(file) = source(&textures, &format!("item/{name}_{index:02}")) {
                let image = open(&file)?;
                if image.width() > 0 && image.height() >= image.width() {
                    provided.push((
                        index,
                        imageops::crop_imm(&image, 0, 0, image.width(), image.width()).to_image(),
                    ));
                }
            }
        }
        if provided.is_empty() {
            continue;
        }
        let size = provided
            .iter()
            .map(|(_, image)| image.width())
            .max()
            .unwrap();
        let mut atlas = RgbaImage::new(size, frames * size);
        for index in 0..frames {
            let (_, image) = provided
                .iter()
                .min_by_key(|(frame, _)| {
                    let distance = frame.abs_diff(index);
                    distance.min(frames - distance)
                })
                .unwrap();
            let image = imageops::resize(image, size, size, imageops::FilterType::Nearest);
            imageops::replace(&mut atlas, &image, 0, i64::from(index * size));
        }
        fs::create_dir_all(output.parent().unwrap())?;
        save(root, &output, &atlas)?;
        fs::write(
            output.with_extension("png.mcmeta"),
            serde_json::to_vec(&json!({"animation":{}}))?,
        )?;
        report.converted.push(format!(
            "{name}：将 {} 张独立帧重建为 {frames} 帧旧版动态贴图",
            provided.len()
        ));
    }
    if count > 0 {
        report.converted.push(format!("1.8.9 纹理路径：补齐 {count} 个门、海晶石、红砂岩、树苗及实体旧版入口，保留动画元数据与已有贴图"));
    }
    Ok(())
}
