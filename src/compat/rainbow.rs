use crate::packfs::{self as fs, PathExt};
use anyhow::{Context, Result, ensure};
use image::{RgbaImage, imageops};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    pub glint: bool,
    pub crosshair: bool,
    pub fishing_line: bool,
    pub block_outline: bool,
}

pub fn color(phase: f32) -> [u8; 3] {
    [0.0, 2.0 / 3.0, 1.0 / 3.0].map(|offset| {
        (((phase + offset).rem_euclid(1.0) * 6.0 - 3.0).abs() - 1.0)
            .clamp(0.0, 1.0)
            .mul_add(255.0, 0.0)
            .round() as u8
    })
}

pub(super) const GLSL: &str = "clamp(abs(fract(vec3(GameTime * 200.0) + vec3(0.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - 3.0) - 1.0, 0.0, 1.0)";

fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

pub(super) fn animate(
    image: &RgbaImage,
    metadata: Option<&[u8]>,
    mut paint: impl FnMut(&RgbaImage, [u8; 3]) -> RgbaImage,
) -> Result<(RgbaImage, Vec<u8>)> {
    let mut metadata: Value = metadata
        .map(serde_json::from_slice)
        .transpose()?
        .unwrap_or_else(|| json!({}));
    let animation = &metadata["animation"];
    let explicit_width = animation["width"].as_u64().map(|value| value as u32);
    let explicit_height = animation["height"].as_u64().map(|value| value as u32);
    let size = image.width().min(image.height());
    let (width, height) = match (explicit_width, explicit_height) {
        (Some(width), Some(height)) => (width, height),
        (Some(width), None) => (width, image.height()),
        (None, Some(height)) => (image.width(), height),
        _ => (size, size),
    };
    ensure!(
        width > 0
            && height > 0
            && image.width().is_multiple_of(width)
            && image.height().is_multiple_of(height),
        "动态彩色的原贴图帧尺寸无效"
    );
    let count = image.width() / width * (image.height() / height);
    let interpolate = count == 1 || animation["interpolate"].as_bool().unwrap_or(false);
    let frametime = animation["frametime"].as_u64().unwrap_or(1);
    ensure!(frametime > 0 && frametime <= 120000, "原贴图帧时间超出范围");
    let frames: Vec<(u32, u32)> = if let Some(frames) = animation["frames"].as_array() {
        frames
            .iter()
            .map(|frame| {
                let index = frame
                    .as_u64()
                    .or_else(|| frame["index"].as_u64())
                    .context("原贴图帧编号无效")?;
                let time = frame["time"].as_u64().unwrap_or(frametime);
                ensure!(
                    index < u64::from(count) && time > 0 && time <= 120000,
                    "原贴图帧超出范围"
                );
                Ok((index as u32, time as u32))
            })
            .collect::<Result<_>>()?
    } else {
        (0..count).map(|index| (index, frametime as u32)).collect()
    };
    ensure!(
        !frames.is_empty() && frames.len() <= 4096,
        "原动画帧列表无效或过长"
    );
    let cycle: u32 = frames.iter().map(|(_, time)| time).sum();
    ensure!(cycle <= 120000, "原动画周期过长，无法安全添加动态彩色");
    let duration = cycle
        .checked_div(gcd(cycle, 120))
        .and_then(|value| value.checked_mul(120))
        .context("动画周期溢出")?;
    ensure!(duration <= 240000, "原动画与彩色动画的公共周期过长");
    let mut sprites = Vec::new();
    let mut indexes = BTreeMap::new();
    let mut timeline: Vec<Value> = Vec::new();
    let mut tick = 0;
    let mut frame = 0;
    let mut remaining = frames[0].1;
    while tick < duration {
        let hue = (tick / 5) % 24;
        let pair = (frames[frame].0, hue);
        let index = if let Some(index) = indexes.get(&pair) {
            *index
        } else {
            let index = sprites.len() as u32;
            ensure!(
                (index + 1)
                    .checked_mul(height)
                    .is_some_and(|value| value <= 16384)
                    && u64::from(width) * u64::from(height) * u64::from(index + 1)
                        <= 16 * 1024 * 1024,
                "动态彩色图集超过安全尺寸；请缩小贴图分辨率或原动画帧数"
            );
            let columns = image.width() / width;
            let source = imageops::crop_imm(
                image,
                pair.0 % columns * width,
                pair.0 / columns * height,
                width,
                height,
            )
            .to_image();
            sprites.push(paint(&source, color(hue as f32 / 24.0)));
            indexes.insert(pair, index);
            index
        };
        let time = remaining.min(5 - tick % 5).min(duration - tick);
        if let Some(last) = timeline.last_mut().filter(|value| value["index"] == index) {
            last["time"] = json!(last["time"].as_u64().unwrap() + u64::from(time));
        } else {
            timeline.push(json!({"index":index,"time":time}));
        }
        tick += time;
        remaining -= time;
        if remaining == 0 {
            frame = (frame + 1) % frames.len();
            remaining = frames[frame].1;
        }
    }
    let mut result = RgbaImage::new(width, height * sprites.len() as u32);
    for (index, sprite) in sprites.iter().enumerate() {
        imageops::replace(&mut result, sprite, 0, index as i64 * i64::from(height));
    }
    metadata["animation"] = json!({"width":width,"height":height,"frametime":5,"interpolate":interpolate,"frames":timeline});
    Ok((result, serde_json::to_vec(&metadata)?))
}

pub(super) fn recolor(image: &RgbaImage, color: [u8; 3]) -> RgbaImage {
    let mut image = image.clone();
    for pixel in image.pixels_mut() {
        if pixel[3] > 0 {
            pixel[0] = color[0];
            pixel[1] = color[1];
            pixel[2] = color[2];
        }
    }
    image
}

pub(super) fn custom_animation(
    root: &Path,
    target: &Path,
    image: &RgbaImage,
    x: u32,
    y: u32,
    id: &str,
) -> Result<()> {
    let base = root.join("assets/minecraft/optifine/anim/fastpack_rainbow");
    fs::create_dir_all(&base)?;
    let (frames, _) = animate(image, None, recolor)?;
    fs::save_image(&frames, base.join(format!("{id}.png")))?;
    let relative = target
        .strip_prefix(root.join("assets/minecraft"))?
        .to_string_lossy()
        .replace('\\', "/");
    let values = super::properties::Properties::from([
        ("from".into(), format!("./{id}.png")),
        ("to".into(), format!("{relative}")),
        ("x".into(), x.to_string()),
        ("y".into(), y.to_string()),
        ("w".into(), image.width().to_string()),
        ("h".into(), image.height().to_string()),
        ("duration".into(), "5".into()),
        ("interpolate".into(), "true".into()),
    ]);
    fs::write_atomic(
        &base.join(format!("{id}.properties")),
        super::properties::write(&values).as_bytes(),
    )?;
    Ok(())
}

pub(crate) fn crosshair(
    root: &Path,
    target: u32,
    report: &mut super::assets::AssetReport,
) -> Result<()> {
    let path = root.join(if target >= 18 {
        "assets/minecraft/textures/gui/sprites/hud/crosshair.png"
    } else {
        "assets/minecraft/textures/gui/icons.png"
    });
    if !path.pack_is_file() {
        return Ok(());
    }
    let image = fs::open_image(&path)?.to_rgba8();
    let (width, height) = if target >= 18 {
        image.dimensions()
    } else {
        (image.width() / 256 * 15, image.width() / 256 * 15)
    };
    let tile = imageops::crop_imm(&image, 0, 0, width, height).to_image();
    if target >= 18 {
        let (animated, metadata) = animate(&tile, None, recolor)?;
        super::assets::save(root, &path, &animated)?;
        fs::write_atomic(&path.with_extension("png.mcmeta"), &metadata)?;
    } else {
        custom_animation(root, &path, &tile, 0, 0, "crosshair")?;
        report.warnings.push("动态彩色准星：目标早于 1.20.2，需要客户端支持 OptiFine 自定义纹理动画；普通原版客户端仅显示静态准星".into());
    }
    report.converted.push(
        "准星动态彩色：六秒循环，保留轮廓与透明度；实际显示颜色受游戏的准星反色混合影响".into(),
    );
    Ok(())
}

pub(crate) fn clear_crosshair_animation(root: &Path) -> Result<()> {
    for name in ["crosshair.png", "crosshair.properties"] {
        let path = root
            .join("assets/minecraft/optifine/anim/fastpack_rainbow")
            .join(name);
        if path.pack_is_file() {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "rainbow_tests.rs"]
mod tests;
