use anyhow::{Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use zip::ZipArchive;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 2,
        "line_color_catalog <version-jars.json> <output.json>"
    );
    let jars: BTreeMap<String, String> = serde_json::from_slice(&fs::read(&args[0])?)?;
    let mut versions = BTreeMap::new();
    let mut profiles = BTreeMap::new();
    for (version, path) in jars {
        if fast_pack::pack_format_of(&version)? < 7 {
            continue;
        }
        let mut zip = ZipArchive::new(File::open(path)?)?;
        let mut read = |extension: &str| -> Result<String> {
            let mut text = String::new();
            zip.by_name(&format!(
                "assets/minecraft/shaders/core/rendertype_lines.{extension}"
            ))?
            .read_to_string(&mut text)?;
            Ok(text)
        };
        let vertex = read("vsh")?;
        let fragment = read("fsh")?;
        let program = if fast_pack::pack_format_of(&version)? < 55 {
            serde_json::from_str(&read("json")?)?
        } else {
            Value::Null
        };
        let profile = json!({"vertex":vertex,"fragment":fragment,"program":program});
        let key = format!("{:x}", Sha256::digest(serde_json::to_vec(&profile)?))[..16].to_string();
        profiles.insert(key.clone(), profile);
        versions.insert(version, key);
    }
    fs::write(
        &args[1],
        serde_json::to_vec(
            &json!({"source":"Official Mojang client resources","versions":versions,"profiles":profiles}),
        )?,
    )?;
    Ok(())
}
