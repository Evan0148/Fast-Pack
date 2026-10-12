use anyhow::Result;
use image::{Rgba, RgbaImage};
use std::fs;
use std::path::{Path, PathBuf};

fn write(root: &Path, name: &str, bytes: impl AsRef<[u8]>) -> Result<()> {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(path, bytes)?;
    Ok(())
}

fn picture(root: &Path, name: &str, width: u32, height: u32) -> Result<()> {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap())?;
    RgbaImage::from_fn(width, height, |x, y| {
        Rgba([
            (x % 251) as u8,
            (y % 239) as u8,
            ((x + 3 * y) % 253) as u8,
            255,
        ])
    })
    .save(path)?;
    Ok(())
}

fn main() -> Result<()> {
    let root = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("validation/automatic-source"));
    write(
        &root,
        "pack.mcmeta",
        br#"{"pack":{"pack_format":1,"description":"Fast Pack automatic conversion fixture"}}"#,
    )?;
    for (name, w, h) in [
        ("entity/chest/normal.png", 64, 64),
        ("entity/chest/normal_double.png", 128, 64),
        ("entity/banner_base.png", 64, 64),
        ("entity/banner/cross.png", 64, 64),
        ("entity/shield_base.png", 64, 64),
        ("entity/shield/cross.png", 64, 64),
        ("entity/boat.png", 64, 32),
        ("entity/enderdragon/dragon.png", 256, 256),
        ("entity/minecart.png", 64, 32),
        ("gui/container/inventory.png", 256, 256),
        ("gui/container/gamemode_switcher.png", 128, 128),
        ("gui/icons.png", 512, 512),
        ("gui/widgets.png", 256, 256),
        ("misc/enchanted_item_glint.png", 128, 128),
        ("font/unicode_page_4e.png", 256, 256),
        ("environment/moon_phases.png", 128, 64),
        ("environment/sun.png", 32, 32),
    ] {
        picture(&root, &format!("assets/minecraft/textures/{name}"), w, h)?;
    }
    let sky = RgbaImage::from_fn(384, 256, |x, y| {
        let colors = [
            [30, 80, 120],
            [70, 115, 160],
            [115, 90, 170],
            [90, 150, 180],
            [35, 85, 140],
            [125, 95, 145],
        ];
        let tile = (y / 128 * 3 + x / 128) as usize;
        let color = colors[tile];
        let glow = ((127 - y % 128) / 4) as u8;
        Rgba([color[0] + glow, color[1] + glow, color[2] + glow, 255])
    });
    let path = root.join("assets/minecraft/mcpatcher/sky/world0/day.png");
    fs::create_dir_all(path.parent().unwrap())?;
    sky.save(path)?;
    write(&root,"assets/minecraft/mcpatcher/sky/world0/sky1.properties",b"source=./day.png\nrotate=false\nblend=add\nstartFadeIn=06:00\nendFadeIn=07:00\nstartFadeOut=18:00\nendFadeOut=19:00\nweather=clear\n")?;
    write(&root,"assets/minecraft/font/uniform.json",br#"{"providers":[{"type":"legacy_unicode","sizes":"minecraft:font/glyph_sizes.bin","template":"minecraft:font/unicode_page_%s.png"}]}"#)?;
    write(&root,"assets/minecraft/models/item/diamond_sword.json",br#"{"parent":"builtin/generated","display":{"firstperson":{"rotation":[0,30,0]}},"textures":{"layer0":"items/sword_diamond"}}"#)?;
    write(
        &root,
        "assets/minecraft/models/block/wool_red.json",
        br#"{"parent":"block/cube_all","textures":{"all":"blocks/wool_colored_red"}}"#,
    )?;
    write(
        &root,
        "assets/minecraft/blockstates/wool.json",
        br#"{"variants":{"color=red":{"model":"wool_red"}}}"#,
    )?;
    println!("{}", root.display());
    Ok(())
}
