use super::assets::{AssetReport, save};
use crate::packfs::PathExt;
use anyhow::{Result, ensure};
use image::{RgbaImage, imageops};
use std::path::Path;

fn legacy_skin(image: &RgbaImage) -> Result<RgbaImage> {
    if image.width() > 0 && !image.width().is_multiple_of(64) {
        let scale = (image.width() / 64).max(1);
        return legacy_skin(&imageops::resize(
            image,
            64 * scale,
            32 * scale,
            imageops::FilterType::Nearest,
        ));
    }
    let scale = image.width() / 64;
    ensure!(
        scale > 0 && image.width().is_multiple_of(64) && image.height() == 32 * scale,
        "旧默认皮肤比例无效"
    );
    let mut output = RgbaImage::new(64 * scale, 64 * scale);
    imageops::replace(&mut output, image, 0, 0);
    for [sx, sy, dx, dy, width, height] in [
        [4, 16, 20, 48, 4, 4],
        [8, 16, 24, 48, 4, 4],
        [0, 20, 24, 52, 4, 12],
        [4, 20, 20, 52, 4, 12],
        [8, 20, 16, 52, 4, 12],
        [12, 20, 28, 52, 4, 12],
        [44, 16, 36, 48, 4, 4],
        [48, 16, 40, 48, 4, 4],
        [40, 20, 40, 52, 4, 12],
        [44, 20, 36, 52, 4, 12],
        [48, 20, 32, 52, 4, 12],
        [52, 20, 44, 52, 4, 12],
    ] {
        let tile = imageops::crop_imm(image, sx * scale, sy * scale, width * scale, height * scale)
            .to_image();
        imageops::replace(
            &mut output,
            &imageops::flip_horizontal(&tile),
            i64::from(dx * scale),
            i64::from(dy * scale),
        );
    }
    let hat_opaque =
        (0..32 * scale).all(|y| (32 * scale..64 * scale).all(|x| image.get_pixel(x, y)[3] >= 128));
    if hat_opaque {
        for y in 0..16 * scale {
            for x in 32 * scale..64 * scale {
                output.get_pixel_mut(x, y)[3] = 0;
            }
        }
    }
    for [x, y, width, height] in [[0, 0, 32, 16], [0, 16, 64, 16], [16, 48, 32, 16]] {
        for row in y * scale..(y + height) * scale {
            for column in x * scale..(x + width) * scale {
                output.get_pixel_mut(column, row)[3] = 255;
            }
        }
    }
    Ok(output)
}

pub(super) fn migrate(
    root: &Path,
    source: u32,
    target: u32,
    report: &mut AssetReport,
) -> Result<()> {
    if source >= 12 || target < 12 {
        return Ok(());
    }
    let textures = root.join("assets/minecraft/textures/entity");
    for (name, model) in [("steve", "wide"), ("alex", "slim")] {
        let input = textures.join(format!("{name}.png"));
        let output = textures.join(format!("player/{model}/{name}.png"));
        if !input.pack_is_file() || output.pack_is_file() {
            continue;
        }
        let image = crate::packfs::open_image(&input)?.to_rgba8();
        let image = if image.width() == image.height() * 2 {
            legacy_skin(&image)?
        } else {
            image
        };
        save(root, &output, &image)?;
        report.converted.push(format!(
            "默认玩家资源：迁移 {name} 到 1.19.3+ 路径，旧 64×32 皮肤按比例补齐左右肢体"
        ));
    }
    Ok(())
}
