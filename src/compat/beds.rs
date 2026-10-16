use super::assets::{AssetReport, open, save};
use crate::packfs::PathExt;
use anyhow::{Result, ensure};
use image::{Rgba, RgbaImage, imageops};
use std::collections::BTreeMap;
use std::path::Path;

type Parts = BTreeMap<&'static str, RgbaImage>;

#[derive(Clone, Copy)]
enum Face {
    Down,
    Up,
    West,
    North,
    East,
    South,
}

impl Face {
    fn region(self, [u, v]: [u32; 2], [w, h, d]: [u32; 3]) -> [u32; 4] {
        match self {
            Self::Down => [u + d, v, w, d],
            Self::Up => [u + d + w, v, w, d],
            Self::West => [u, v + d, d, h],
            Self::North => [u + d, v + d, w, h],
            Self::East => [u + d + w, v + d, d, h],
            Self::South => [u + 2 * d + w, v + d, w, h],
        }
    }

    fn point(self, x: f64, y: f64, [w, h, d]: [u32; 3]) -> [f64; 3] {
        let [w, h, d] = [w, h, d].map(f64::from);
        match self {
            Self::Down => [x, 0.0, d - y],
            Self::Up => [x, h, d - y],
            Self::West => [0.0, y, d - x],
            Self::North => [x, y, 0.0],
            Self::East => [w, y, x],
            Self::South => [w - x, y, d],
        }
    }

    fn normal(self) -> [f64; 3] {
        match self {
            Self::Down => [0.0, -1.0, 0.0],
            Self::Up => [0.0, 1.0, 0.0],
            Self::West => [-1.0, 0.0, 0.0],
            Self::North => [0.0, 0.0, -1.0],
            Self::East => [1.0, 0.0, 0.0],
            Self::South => [0.0, 0.0, 1.0],
        }
    }
}

fn leg_rotation([x, y, z]: [f64; 3], turns: u32) -> [f64; 3] {
    match turns {
        0 => [x, -z, y],
        1 => [z, x, y],
        2 => [-x, z, y],
        _ => [-z, -x, y],
    }
}

fn sample(image: &RgbaImage, u: f64, v: f64, scale: u32) -> Rgba<u8> {
    let maximum = 16 * scale - 1;
    let x = (u * f64::from(scale))
        .floor()
        .clamp(0.0, f64::from(maximum)) as u32;
    let y = (v * f64::from(scale))
        .floor()
        .clamp(0.0, f64::from(maximum)) as u32;
    *image.get_pixel(x, y)
}

fn paint_cube(
    atlas: &mut RgbaImage,
    parts: &Parts,
    wood: &RgbaImage,
    head: bool,
    leg: Option<([f64; 3], u32)>,
    uv: [u32; 2],
    scale: u32,
) {
    let dimensions = if leg.is_some() {
        [3, 3, 3]
    } else {
        [16, 16, 6]
    };
    let prefix = if head { "bed_head" } else { "bed_feet" };
    for face in [
        Face::Down,
        Face::Up,
        Face::West,
        Face::North,
        Face::East,
        Face::South,
    ] {
        let [u, v, width, height] = face.region(uv, dimensions);
        let mut normal = face.normal();
        if let Some((_, turns)) = leg {
            normal = leg_rotation(normal, turns);
        }
        let normal = [-normal[0], -normal[2], -normal[1]];
        for y in 0..height * scale {
            for x in 0..width * scale {
                let mut point = face.point(
                    (f64::from(x) + 0.5) / f64::from(scale),
                    (f64::from(y) + 0.5) / f64::from(scale),
                    dimensions,
                );
                if let Some((origin, turns)) = leg {
                    for axis in 0..3 {
                        point[axis] += origin[axis];
                    }
                    point = leg_rotation(point, turns);
                }
                let [world_x, world_y, world_z] =
                    [16.0 - point[0], 9.0 - point[2], 16.0 - point[1]];
                let color = if normal[1] > 0.0 && leg.is_none() {
                    sample(
                        &parts[format!("{prefix}_top").as_str()],
                        world_z,
                        world_x,
                        scale,
                    )
                } else if normal[1] != 0.0 {
                    sample(wood, world_x, 16.0 - world_z, scale)
                } else if normal[0] != 0.0 {
                    parts.get(format!("{prefix}_side").as_str()).map_or_else(
                        || sample(wood, world_z, 16.0 - world_y, scale),
                        |image| sample(image, world_z, 16.0 - world_y, scale),
                    )
                } else if leg.is_some() || head && normal[2] > 0.0 || !head && normal[2] < 0.0 {
                    let horizontal = if normal[2] > 0.0 {
                        world_x
                    } else {
                        16.0 - world_x
                    };
                    parts.get(format!("{prefix}_end").as_str()).map_or_else(
                        || sample(wood, horizontal, 16.0 - world_y, scale),
                        |image| sample(image, horizontal, 16.0 - world_y, scale),
                    )
                } else {
                    Rgba([0, 0, 0, 0])
                };
                atlas.put_pixel(u * scale + x, v * scale + y, color);
            }
        }
    }
}

fn previous_atlas(parts: &Parts, wood: &RgbaImage, scale: u32) -> RgbaImage {
    let mut atlas = RgbaImage::new(64 * scale, 64 * scale);
    for (prefix, y) in [("head", 0), ("feet", 22)] {
        let top = imageops::rotate270(&parts[format!("bed_{prefix}_top").as_str()]);
        imageops::replace(
            &mut atlas,
            &top,
            6 * i64::from(scale),
            i64::from((y + 6) * scale),
        );
        imageops::replace(
            &mut atlas,
            wood,
            28 * i64::from(scale),
            i64::from((y + 6) * scale),
        );
        let side = parts
            .get(format!("bed_{prefix}_side").as_str())
            .map(|image| imageops::crop_imm(image, 0, 7 * scale, 16 * scale, 6 * scale).to_image())
            .unwrap_or_else(|| {
                imageops::resize(wood, 16 * scale, 6 * scale, imageops::FilterType::Nearest)
            });
        let end = parts
            .get(format!("bed_{prefix}_end").as_str())
            .map(|image| imageops::crop_imm(image, 0, 7 * scale, 16 * scale, 6 * scale).to_image())
            .unwrap_or_else(|| side.clone());
        let side = imageops::rotate90(&side);
        imageops::replace(&mut atlas, &side, 0, i64::from((y + 6) * scale));
        imageops::replace(
            &mut atlas,
            &imageops::flip_horizontal(&side),
            22 * i64::from(scale),
            i64::from((y + 6) * scale),
        );
        imageops::replace(&mut atlas, &end, 6 * i64::from(scale), i64::from(y * scale));
        imageops::replace(
            &mut atlas,
            &end,
            22 * i64::from(scale),
            i64::from(y * scale),
        );
    }
    for y in [0, 6, 12, 18] {
        let leg = imageops::resize(wood, 12 * scale, 6 * scale, imageops::FilterType::Nearest);
        imageops::replace(
            &mut atlas,
            &leg,
            50 * i64::from(scale),
            i64::from(y * scale),
        );
    }
    atlas
}

pub(super) fn migrate(
    root: &Path,
    target: u32,
    version: &str,
    report: &mut AssetReport,
) -> Result<()> {
    if target < 3 || version.starts_with("1.11") {
        return Ok(());
    }
    let textures = root.join("assets/minecraft/textures");
    let output = textures.join("entity/bed/red.png");
    let mut parts = BTreeMap::new();
    for name in [
        "bed_head_top",
        "bed_feet_top",
        "bed_head_side",
        "bed_feet_side",
        "bed_head_end",
        "bed_feet_end",
    ] {
        let alternate = format!("{}_bed", name.strip_prefix("bed_").unwrap());
        let directory = if target < 4 { "blocks" } else { "block" };
        let path = [
            textures.join(format!("{directory}/{name}.png")),
            textures.join(format!("{directory}/{alternate}.png")),
        ]
        .into_iter()
        .find(|path| path.pack_is_file());
        if let Some(path) = path {
            parts.insert(name, open(&path)?);
        }
    }
    if parts.is_empty() {
        return Ok(());
    }
    if !parts.contains_key("bed_head_top") || !parts.contains_key("bed_feet_top") {
        report
            .warnings
            .push("旧床纹理：缺少床头或床尾顶面，保留原图并使用目标原版床渲染".into());
        return Ok(());
    }
    ensure!(
        parts
            .values()
            .all(|image| image.width() > 0 && image.height() >= image.width()),
        "旧床纹理比例无效"
    );
    let scale = parts
        .values()
        .map(|image| image.width().div_ceil(16))
        .max()
        .unwrap()
        .max(1);
    for image in parts.values_mut() {
        let frame = imageops::crop_imm(image, 0, 0, image.width(), image.width()).to_image();
        *image = imageops::resize(
            &frame,
            16 * scale,
            16 * scale,
            imageops::FilterType::Nearest,
        );
    }
    let wood_path = textures.join(if target < 4 {
        "blocks/planks_oak.png"
    } else {
        "block/oak_planks.png"
    });
    let wood = if wood_path.pack_is_file() {
        let image = open(&wood_path)?;
        let size = image.width().min(image.height());
        let frame = imageops::crop_imm(&image, 0, 0, size, size).to_image();
        imageops::resize(
            &frame,
            16 * scale,
            16 * scale,
            imageops::FilterType::Nearest,
        )
    } else {
        RgbaImage::from_fn(16 * scale, 16 * scale, |x, y| {
            Rgba([142 + ((x / scale + y / scale) % 4) as u8 * 3, 110, 65, 255])
        })
    };
    let mut atlas = RgbaImage::new(64 * scale, 64 * scale);
    paint_cube(&mut atlas, &parts, &wood, true, None, [0, 0], scale);
    paint_cube(&mut atlas, &parts, &wood, false, None, [0, 22], scale);
    for (head, origin, turns, v) in [
        (true, [0.0, 6.0, 0.0], 1, 6),
        (true, [-16.0, 6.0, 0.0], 2, 18),
        (false, [0.0, 6.0, -16.0], 0, 0),
        (false, [-16.0, 6.0, -16.0], 3, 12),
    ] {
        paint_cube(
            &mut atlas,
            &parts,
            &wood,
            head,
            Some((origin, turns)),
            [50, v],
            scale,
        );
    }
    let repaired = if output.pack_is_file() {
        let existing = open(&output)?;
        if existing == atlas
            || output.with_extension("png.mcmeta").pack_is_file()
            || existing != previous_atlas(&parts, &wood, scale)
        {
            return Ok(());
        }
        true
    } else {
        false
    };
    save(root, &output, &atlas)?;
    report.converted.push(if repaired {
        "旧床资源：识别并修复旧转换生成的错位图集，按官方床模型重新对齐顶面、侧面、端面与四条床腿"
            .into()
    } else {
        "旧床资源：按官方模型 UV 重建红床图集，对齐床头、床尾、侧面与四条床腿；保留已有自定义实体床"
            .into()
    });
    Ok(())
}
