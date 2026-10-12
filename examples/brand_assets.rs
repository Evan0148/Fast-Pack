use anyhow::{Context, Result};
use image::{ImageFormat, Rgba, RgbaImage, imageops};
use serde_json::json;
use std::collections::VecDeque;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

fn main() -> Result<()> {
    let source = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .context("需要原始 logo PNG")?;
    let mut image = image::open(&source)?.to_rgba8();
    let (w, h) = image.dimensions();
    let corner = *image.get_pixel(0, 0);
    let matte = corner[3] > 0
        && (corner.0[..3].iter().all(|value| *value > 240)
            || corner.0[..3].iter().all(|value| *value < 15));
    let palette = [[112u8, 217, 238], [8, 123, 145]];
    for pixel in image.pixels_mut() {
        if pixel[3] < 16 || matte && (0..3).all(|index| pixel[index].abs_diff(corner[index]) < 24) {
            *pixel = Rgba([0, 0, 0, 0]);
            continue;
        }
        let color = palette
            .iter()
            .min_by_key(|color| {
                (0..3)
                    .map(|index| (i32::from(pixel[index]) - i32::from(color[index])).pow(2))
                    .sum::<i32>()
            })
            .unwrap();
        *pixel = Rgba([color[0], color[1], color[2], pixel[3]]);
    }
    let mut visited = vec![false; (w * h) as usize];
    let mut discarded = 0;
    for index in 0..w * h {
        if visited[index as usize] || image.get_pixel(index % w, index / w)[3] == 0 {
            continue;
        }
        let mut queue = VecDeque::from([index]);
        let mut component = Vec::new();
        visited[index as usize] = true;
        while let Some(index) = queue.pop_front() {
            component.push(index);
            let x = index % w;
            let y = index / w;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let nx = x as i64 + dx;
                    let ny = y as i64 + dy;
                    if nx < 0 || ny < 0 || nx >= i64::from(w) || ny >= i64::from(h) {
                        continue;
                    }
                    let next = ny as u32 * w + nx as u32;
                    if !visited[next as usize] && image.get_pixel(nx as u32, ny as u32)[3] > 0 {
                        visited[next as usize] = true;
                        queue.push_back(next);
                    }
                }
            }
        }
        if component.len() < (w * h / 2048) as usize {
            discarded += component.len();
            for index in component {
                image.put_pixel(index % w, index / w, Rgba([0, 0, 0, 0]));
            }
        }
    }
    let mut bounds = (w, h, 0, 0);
    for (x, y, pixel) in image.enumerate_pixels() {
        if pixel[3] > 0 {
            bounds.0 = bounds.0.min(x);
            bounds.1 = bounds.1.min(y);
            bounds.2 = bounds.2.max(x);
            bounds.3 = bounds.3.max(y);
        }
    }
    anyhow::ensure!(
        bounds.0 <= bounds.2 && bounds.1 <= bounds.3,
        "logo 没有前景"
    );
    let crop = imageops::crop_imm(
        &image,
        bounds.0,
        bounds.1,
        bounds.2 - bounds.0 + 1,
        bounds.3 - bounds.1 + 1,
    )
    .to_image();
    let scale = 870.0 / f64::from(crop.width().max(crop.height()));
    let foreground = imageops::resize(
        &crop,
        (f64::from(crop.width()) * scale).round() as u32,
        (f64::from(crop.height()) * scale).round() as u32,
        imageops::FilterType::Lanczos3,
    );
    let mut logo = RgbaImage::new(1024, 1024);
    imageops::replace(
        &mut logo,
        &foreground,
        i64::from((1024 - foreground.width()) / 2),
        i64::from((1024 - foreground.height()) / 2),
    );
    logo.save("assets/branding/fast-pack-logo.png")?;
    let app = imageops::resize(&logo, 256, 256, imageops::FilterType::Lanczos3);
    app.save("assets/app.png")?;
    let mut blobs = Vec::new();
    let sizes = [16u32, 24, 32, 48, 64, 128, 256];
    for size in sizes {
        let tile = imageops::resize(&logo, size, size, imageops::FilterType::Lanczos3);
        let mut buffer = Cursor::new(Vec::new());
        tile.write_to(&mut buffer, ImageFormat::Png)?;
        blobs.push(buffer.into_inner());
    }
    let mut ico = Vec::new();
    ico.extend(0u16.to_le_bytes());
    ico.extend(1u16.to_le_bytes());
    ico.extend((sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (size, blob) in sizes.into_iter().zip(&blobs) {
        ico.extend([if size == 256 { 0 } else { size as u8 }; 2]);
        ico.extend([0, 0]);
        ico.extend(1u16.to_le_bytes());
        ico.extend(32u16.to_le_bytes());
        ico.extend((blob.len() as u32).to_le_bytes());
        ico.extend(offset.to_le_bytes());
        offset += blob.len() as u32;
    }
    for blob in blobs {
        ico.extend(blob);
    }
    fs::write("assets/app.ico", ico)?;
    let metadata = json!({"model":"gpt-image-2.5-sunburst","requested_size":"1K","requested_pixels":"1024x1024","original_size":format!("{w}x{h}"),"quality":"low","prompt":"logo-prompt.txt","original":"fast-pack-logo-sunburst-original.png","logo":"fast-pack-logo.png","logo_size":"1024x1024","transparent":true,"processing":["two-color normalization","remove isolated edge specks","balanced margins","multi-size Windows ICO"],"discarded_noise_pixels":discarded,"ico_sizes":sizes});
    fs::write(
        "assets/branding/generation.json",
        serde_json::to_vec_pretty(&metadata)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&metadata)?);
    Ok(())
}
