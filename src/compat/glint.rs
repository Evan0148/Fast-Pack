use super::assets::{AssetReport, backup, save};
use crate::packfs as fs;
use crate::packfs::PathExt;
use anyhow::{Context, Result, ensure};
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

const LEGACY: &str = "enchanted_item_glint";
const MODERN: [&str; 3] = [
    "enchanted_glint_item",
    "enchanted_glint_entity",
    "enchanted_glint_armor",
];
const LEGACY_TINT: [f32; 3] = [128.0 / 255.0, 64.0 / 255.0, 204.0 / 255.0];
pub const VANILLA_COLOR: [u8; 3] = [128, 64, 204];

pub fn parse_color(value: &str) -> Result<[u8; 3]> {
    let value = value.trim();
    let value = value.strip_prefix('#').unwrap_or(value);
    ensure!(
        matches!(value.len(), 3 | 6) && value.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "颜色格式应为 #RRGGBB，例如白色 #FFFFFF"
    );
    let mut color = [0; 3];
    for (index, channel) in color.iter_mut().enumerate() {
        *channel = if value.len() == 3 {
            u8::from_str_radix(&value[index..index + 1], 16)? * 17
        } else {
            u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)?
        };
    }
    Ok(color)
}

pub fn color_hex(color: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", color[0], color[1], color[2])
}

fn bake_legacy(image: &RgbaImage) -> RgbaImage {
    let mut output = image.clone();
    for pixel in output.pixels_mut() {
        for (index, tint) in LEGACY_TINT.into_iter().enumerate() {
            pixel[index] = (pixel[index] as f32 * tint).round() as u8;
        }
    }
    output
}

fn compensate_legacy(image: &RgbaImage) -> RgbaImage {
    let peak = image
        .pixels()
        .filter(|pixel| pixel[3] != 0)
        .flat_map(|pixel| {
            LEGACY_TINT
                .into_iter()
                .enumerate()
                .map(move |(index, tint)| pixel[index] as f32 / (255.0 * tint))
        })
        .fold(1.0_f32, f32::max);
    let mut output = image.clone();
    for pixel in output.pixels_mut() {
        for (index, tint) in LEGACY_TINT.into_iter().enumerate() {
            pixel[index] = (pixel[index] as f32 / (tint * peak))
                .round()
                .clamp(0.0, 255.0) as u8;
        }
    }
    output
}

fn recolor(image: &RgbaImage, color: [u8; 3], target: u32) -> RgbaImage {
    let peak = image
        .pixels()
        .filter(|pixel| pixel[3] != 0)
        .map(|pixel| pixel[0].max(pixel[1]).max(pixel[2]))
        .max()
        .unwrap_or(255)
        .max(1) as f32;
    let mut channels = color.map(|channel| channel as f32 / 255.0);
    if target < 5 {
        for (index, tint) in LEGACY_TINT.into_iter().enumerate() {
            channels[index] /= tint;
        }
        let peak = channels.into_iter().fold(1.0_f32, f32::max);
        for channel in &mut channels {
            *channel /= peak;
        }
    }
    let mut output = image.clone();
    for pixel in output.pixels_mut() {
        let brightness = (pixel[0].max(pixel[1]).max(pixel[2]) as f32 * 255.0 / peak).min(255.0);
        for (index, channel) in channels.into_iter().enumerate() {
            pixel[index] = (brightness * channel).round() as u8;
        }
    }
    output
}

fn template() -> RgbaImage {
    let tau = std::f32::consts::TAU;
    RgbaImage::from_fn(128, 128, |x, y| {
        let u = x as f32 / 128.0;
        let v = y as f32 / 128.0;
        let waves = ((u * 3.0 + v * 5.0) * tau).sin() * 0.30
            + ((u * 7.0 - v * 2.0) * tau).sin() * 0.17
            + ((u * 11.0 + v * 9.0) * tau).cos() * 0.10;
        let level = ((0.47 + waves).clamp(0.05, 0.95) * 255.0).round() as u8;
        Rgba([level, level, level, 255])
    })
}

struct Texture {
    image: RgbaImage,
    metadata: Option<Vec<u8>>,
}

#[derive(Default, Deserialize, Serialize)]
struct PatternState {
    outputs: BTreeMap<String, String>,
}

fn fingerprint(image: &RgbaImage) -> String {
    let hash = image
        .as_raw()
        .iter()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
    format!("{}x{}-{hash:016x}", image.width(), image.height())
}

fn original_pattern(
    directory: &Path,
    state: &PatternState,
    name: &str,
    image: &RgbaImage,
) -> RgbaImage {
    let hash = fingerprint(image);
    for cached_name in [name, LEGACY, MODERN[0], MODERN[1], MODERN[2]] {
        if state.outputs.get(cached_name) != Some(&hash) {
            continue;
        }
        let path = directory.join(format!("{cached_name}.png"));
        if let Ok(original) = crate::packfs::open_image(path) {
            let original = original.to_rgba8();
            if original.dimensions() == image.dimensions()
                && original
                    .as_raw()
                    .chunks_exact(4)
                    .zip(image.as_raw().chunks_exact(4))
                    .all(|(a, b)| a[3] == b[3])
            {
                return original;
            }
        }
    }
    image.clone()
}

fn select<'a>(
    textures: &'a BTreeMap<&str, Texture>,
    name: &str,
    source: u32,
) -> Option<&'a Texture> {
    if source < 13 {
        if name != LEGACY
            && let Some(texture) = textures.get(name)
        {
            return Some(texture);
        }
        textures
            .get(LEGACY)
            .or_else(|| MODERN.iter().find_map(|name| textures.get(name)))
    } else {
        let names = match name {
            "enchanted_glint_armor" => [MODERN[2], MODERN[1], MODERN[0]],
            "enchanted_glint_entity" => [MODERN[1], MODERN[2], MODERN[0]],
            _ => MODERN,
        };
        names
            .iter()
            .find_map(|name| textures.get(name))
            .or_else(|| textures.get(LEGACY))
    }
}

pub(crate) fn restore_vanilla(root: &Path, target: u32, report: &mut AssetReport) -> Result<()> {
    let mut restored = AssetReport::default();
    migrate(root, target, target, Some(VANILLA_COLOR), &mut restored)?;
    report
        .converted
        .push("附魔光效：恢复原版紫色，保留图案、透明度与游戏流动动画".into());
    report.generated.extend(restored.generated);
    report.warnings.extend(restored.warnings);
    Ok(())
}

pub(crate) fn migrate(
    root: &Path,
    source: u32,
    target: u32,
    color: Option<[u8; 3]>,
    report: &mut AssetReport,
) -> Result<()> {
    let directory = root.join("assets/minecraft/textures/misc");
    let mut textures = BTreeMap::new();
    for name in [LEGACY, MODERN[0], MODERN[1], MODERN[2]] {
        let path = directory.join(format!("{name}.png"));
        if path.pack_is_file() {
            let image = crate::packfs::open_image(&path)
                .with_context(|| format!("无法读取附魔光效 {}", path.display()))?
                .to_rgba8();
            let metadata = path.with_extension("png.mcmeta");
            let metadata = if metadata.pack_is_file() {
                Some(fs::read(metadata)?)
            } else {
                None
            };
            textures.insert(name, Texture { image, metadata });
        }
    }
    if textures.is_empty() && color.is_none() {
        return Ok(());
    }
    let fallback = template();
    let patterns = root.join("fastpack/glint-patterns");
    let state_path = patterns.join("state.json");
    let mut state = fs::read(&state_path)
        .ok()
        .and_then(|data| serde_json::from_slice::<PatternState>(&data).ok())
        .unwrap_or_default();
    let previous_state = PatternState {
        outputs: state.outputs.clone(),
    };
    let names: &[&str] = if target < 13 { &[LEGACY] } else { &MODERN };
    let mut changed = 0;
    for name in names {
        let texture = select(&textures, name, source);
        let image = texture.map(|texture| &texture.image).unwrap_or(&fallback);
        let output = if let Some(color) = color {
            let original = original_pattern(&patterns, &previous_state, name, image);
            fs::create_dir_all(&patterns)?;
            crate::packfs::save_image(&original, patterns.join(format!("{name}.png")))?;
            let output = recolor(&original, color, target);
            state.outputs.insert((*name).into(), fingerprint(&output));
            output
        } else if source < 5 && target >= 5 {
            let already_colored = *name != LEGACY
                && textures.get(name).is_some_and(|texture| {
                    textures
                        .get(LEGACY)
                        .is_none_or(|legacy| texture.image != legacy.image)
                });
            if already_colored {
                image.clone()
            } else {
                bake_legacy(image)
            }
        } else if source >= 5 && target < 5 {
            compensate_legacy(image)
        } else {
            image.clone()
        };
        let same_image = textures
            .get(name)
            .is_some_and(|texture| texture.image == output);
        let desired_metadata = texture.and_then(|texture| texture.metadata.as_ref());
        let same_metadata = textures
            .get(name)
            .is_some_and(|texture| texture.metadata.as_ref() == desired_metadata);
        if same_image && same_metadata {
            continue;
        }
        let path = directory.join(format!("{name}.png"));
        if !same_image {
            save(root, &path, &output)?;
        }
        let metadata = path.with_extension("png.mcmeta");
        if metadata.pack_is_file() {
            backup(root, &metadata)?;
        }
        if let Some(data) = desired_metadata {
            fs::write(metadata, data)?;
        } else if metadata.pack_is_file() {
            fs::remove_file(metadata)?;
        }
        changed += 1;
    }
    if let Some(color) = color {
        fs::write(state_path, serde_json::to_vec_pretty(&state)?)?;
        report.converted.push(format!(
            "附魔光效：应用自定义颜色 {}，保留图案、透明度与游戏流动动画",
            color_hex(color)
        ));
        if textures.is_empty() {
            report
                .generated
                .push("附魔光效：原包无光效贴图，自动生成可平铺图案".into());
        }
    } else if changed > 0 {
        report
            .converted
            .push(format!("附魔光效：自动适配 {changed} 张目标版本纹理"));
        if source < 5 && target >= 5 {
            report
                .converted
                .push("附魔光效：烘焙旧版紫色染色，修复跨版本变白".into());
        } else if source >= 5 && target < 5 {
            report
                .converted
                .push("附魔光效：补偿旧版渲染染色，按通道比例保留颜色".into());
        }
    }
    Ok(())
}
