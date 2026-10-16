use anyhow::{Result, ensure};
use image::imageops;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

#[derive(Deserialize)]
struct Sprite {
    source: String,
    target: String,
    source_width: u32,
    source_height: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 3,
        "audit_hud <source.zip> <output folder> <report.json>"
    );
    let mut source = ZipArchive::new(File::open(&args[0])?)?;
    let recipes: Vec<Sprite> = serde_json::from_str(include_str!("../assets/gui-sprites.json"))?;
    let mut atlases = BTreeMap::new();
    let mut checks = Vec::new();
    for recipe in recipes {
        if recipe.target.contains("attack_indicator") {
            continue;
        }
        if !matches!(recipe.source.as_str(), "gui/icons.png" | "gui/widgets.png") {
            continue;
        }
        if !atlases.contains_key(&recipe.source) {
            let mut bytes = Vec::new();
            if let Ok(mut entry) =
                source.by_name(&format!("assets/minecraft/textures/{}", recipe.source))
            {
                entry.read_to_end(&mut bytes)?;
            } else {
                continue;
            }
            atlases.insert(
                recipe.source.clone(),
                image::load_from_memory(&bytes)?.to_rgba8(),
            );
        }
        let image = &atlases[&recipe.source];
        let sx = image.width() / recipe.source_width;
        let sy = image.height() / recipe.source_height;
        let expected = imageops::crop_imm(
            image,
            recipe.x * sx,
            recipe.y * sy,
            recipe.width * sx,
            recipe.height * sy,
        )
        .to_image();
        let path = Path::new(&args[1]).join(format!("assets/minecraft/textures/{}", recipe.target));
        let actual = image::open(path)?.to_rgba8();
        ensure!(actual == expected, "HUD pixel mismatch: {}", recipe.target);
        checks.push(json!({"sprite":recipe.target,"pixels_equal":true,"size":[actual.width(),actual.height()]}));
    }
    ensure!(
        checks
            .iter()
            .any(|check| check["sprite"] == "gui/sprites/hud/crosshair.png"),
        "No crosshair verification"
    );
    ensure!(
        checks
            .iter()
            .any(|check| check["sprite"] == "gui/sprites/hud/experience_bar_progress.png"),
        "No experience verification"
    );
    fs::write(&args[2], serde_json::to_vec_pretty(&checks)?)?;
    println!("HUD sprites checked={}, pixel mismatches=0", checks.len());
    Ok(())
}
