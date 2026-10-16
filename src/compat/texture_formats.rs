use super::assets::{AssetReport, open, save};
use crate::packfs::File;
use crate::packfs::PathExt;
use crate::packfs::WalkDir;
use anyhow::Result;
use std::io::Read;
use std::path::Path;

pub(super) fn migrate(root: &Path, report: &mut AssetReport) -> Result<()> {
    let assets = root.join("assets");
    if !assets.pack_is_dir() {
        return Ok(());
    }
    for entry in WalkDir::new(assets).min_depth(1) {
        let entry = entry?;
        if !entry.file_type().is_file()
            || entry
                .path()
                .extension()
                .is_none_or(|extension| extension != "png")
        {
            continue;
        }
        let mut signature = [0_u8; 8];
        let length = File::open(entry.path())?.read(&mut signature)?;
        if image::guess_format(&signature[..length]).ok() != Some(image::ImageFormat::Jpeg) {
            continue;
        }
        let relative = entry.path().strip_prefix(root)?.to_string_lossy();
        match open(entry.path()) {
            Ok(image) => {
                save(root, entry.path(), &image)?;
                report.converted.push(format!(
                    "纹理格式：{relative} 的实际内容为 JPEG，已自动重编码为 PNG 并保留原图"
                ));
            }
            Err(error) => report.warnings.push(format!(
                "纹理格式：{relative} 无法解码，已保留原文件：{error:#}"
            )),
        }
    }
    Ok(())
}
