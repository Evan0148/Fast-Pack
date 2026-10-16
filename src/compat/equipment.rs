use super::assets::AssetReport;
use crate::packfs as fs;
use crate::packfs::PathExt;
use anyhow::Result;
use std::path::Path;

pub(super) fn migrate(
    root: &Path,
    source: u32,
    target: u32,
    report: &mut AssetReport,
) -> Result<()> {
    let upgrade = source < 42 && target >= 42;
    let downgrade = target < 42 && source >= 42;
    if !upgrade && !downgrade {
        return Ok(());
    }
    let textures = root.join("assets/minecraft/textures");
    let mut count = 0;
    for material in [
        "chainmail",
        "diamond",
        "gold",
        "iron",
        "leather",
        "netherite",
        "turtle",
        "copper",
    ] {
        let modern = if material == "turtle" {
            "turtle_scute"
        } else {
            material
        };
        for (layer, directory) in [(1, "humanoid"), (2, "humanoid_leggings")] {
            for overlay in [false, true] {
                if overlay && material != "leather" {
                    continue;
                }
                let suffix = if overlay { "_overlay" } else { "" };
                let legacy =
                    textures.join(format!("models/armor/{material}_layer_{layer}{suffix}.png"));
                let modern =
                    textures.join(format!("entity/equipment/{directory}/{modern}{suffix}.png"));
                let (input, output) = if upgrade {
                    (&legacy, &modern)
                } else {
                    (&modern, &legacy)
                };
                if !input.pack_is_file() || output.pack_is_file() {
                    continue;
                }
                fs::create_dir_all(output.parent().unwrap())?;
                fs::copy(input, output)?;
                let metadata = input.with_extension("png.mcmeta");
                if metadata.pack_is_file() {
                    fs::copy(metadata, output.with_extension("png.mcmeta"))?;
                }
                count += 1;
            }
        }
    }
    if count > 0 {
        report.converted.push(format!(
            "装备穿戴纹理：自动迁移 {count} 张盔甲与皮革叠层，适配 1.21.2+ 装备目录"
        ));
    }
    Ok(())
}
