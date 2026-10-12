use image::RgbaImage;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

fn search(source: &RgbaImage, target: &RgbaImage) -> Option<(u32, u32)> {
    if target.width() > source.width() || target.height() > source.height() {
        return None;
    }
    let mut colors: HashMap<[u8; 4], Vec<(u32, u32)>> = HashMap::new();
    for (x, y, pixel) in source.enumerate_pixels() {
        if pixel[3] > 0 {
            colors.entry(pixel.0).or_default().push((x, y));
        }
    }
    let anchor = target
        .enumerate_pixels()
        .filter(|(_, _, pixel)| pixel[3] > 0)
        .filter_map(|(x, y, pixel)| {
            colors
                .get(&pixel.0)
                .map(|positions| (positions.len(), x, y, positions))
        })
        .min_by_key(|(count, _, _, _)| *count)?;
    let pixels: Vec<_> = target.enumerate_pixels().collect();
    let mut best = None;
    for &(x, y) in anchor.3 {
        if x < anchor.1 || y < anchor.2 {
            continue;
        }
        let left = x - anchor.1;
        let top = y - anchor.2;
        if left + target.width() > source.width() || top + target.height() > source.height() {
            continue;
        }
        let count = pixels
            .iter()
            .filter(|(px, py, p)| {
                let source = source.get_pixel(left + *px, top + *py);
                if p[3] == 0 {
                    source[3] == 0
                } else {
                    source == *p
                }
            })
            .count();
        let score = count as f32 / pixels.len() as f32;
        if score >= 0.985 && best.is_none_or(|(_, _, old)| score > old) {
            best = Some((left, top, score));
        }
    }
    best.map(|(x, y, _)| (x, y))
}

fn main() -> anyhow::Result<()> {
    let root = Path::new(".backups/compat-research");
    let mut recipes = Vec::<Value>::new();
    let atlases = [
        (
            "gui/icons.png",
            image::open(root.join("reference-1.20.1/assets/minecraft/textures/gui/icons.png"))?
                .to_rgba8(),
        ),
        (
            "gui/widgets.png",
            image::open(root.join("reference-1.20.1/assets/minecraft/textures/gui/widgets.png"))?
                .to_rgba8(),
        ),
        (
            "gui/container/inventory.png",
            image::open(
                root.join("reference-1.20.1/assets/minecraft/textures/gui/container/inventory.png"),
            )?
            .to_rgba8(),
        ),
    ];
    let directory = root.join("reference-1.21.5/assets/minecraft/textures/gui/sprites");
    for entry in WalkDir::new(&directory).min_depth(1) {
        let entry = entry?;
        if !entry.file_type().is_file() || entry.path().extension().is_none_or(|ext| ext != "png") {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(&directory)?
            .to_string_lossy()
            .replace('\\', "/");
        let target = image::open(entry.path())?.to_rgba8();
        for (name, source) in &atlases {
            if let Some((x, y)) = search(source, &target) {
                recipes.push(json!({"source":name,"target":format!("gui/sprites/{relative}"),"source_width":source.width(),"source_height":source.height(),"x":x,"y":y,"width":target.width(),"height":target.height()}));
                break;
            }
        }
    }
    fs::write(
        "assets/gui-sprites.json",
        serde_json::to_vec_pretty(&recipes)?,
    )?;
    println!("Derived {} GUI sprite regions", recipes.len());
    Ok(())
}
