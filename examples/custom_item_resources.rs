use anyhow::{Result, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 2,
        "custom_item_resources <era-jars.json> <output.zip>"
    );
    let references: BTreeMap<String, String> = serde_json::from_slice(&fs::read(&args[0])?)?;
    let mut output = ZipWriter::new(File::create(&args[1])?);
    let mut resources = BTreeMap::<String, [usize; 2]>::new();
    let mut objects = BTreeMap::new();
    let mut content = Vec::new();
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(9));
    for (era, path) in references {
        let mut source = ZipArchive::new(File::open(path)?)?;
        for index in 0..source.len() {
            let mut entry = source.by_index(index)?;
            let name = entry.name().to_string();
            let model = name.starts_with("assets/minecraft/models/") && name.ends_with(".json");
            let definition = name.starts_with("assets/minecraft/items/") && name.ends_with(".json");
            let texture = ["item", "items", "block", "blocks"].iter().any(|folder| {
                name.strip_prefix(&format!("assets/minecraft/textures/{folder}/"))
                    .is_some_and(|file| {
                        !file.contains('/')
                            && (file.ends_with(".png") || file.ends_with(".png.mcmeta"))
                    })
            });
            if !(model || texture || definition) {
                continue;
            }
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            if model || definition {
                let value: Value = serde_json::from_slice(&bytes)?;
                bytes = serde_json::to_vec(&value)?;
            }
            let digest = format!("{:x}", Sha256::digest(&bytes));
            let range = *objects.entry(digest).or_insert_with(|| {
                let range = [content.len(), bytes.len()];
                content.extend_from_slice(&bytes);
                range
            });
            resources.insert(format!("{era}/{name}"), range);
        }
    }
    output.start_file("index.json", options)?;
    output.write_all(&serde_json::to_vec(&resources)?)?;
    output.start_file("objects.bin", options)?;
    output.write_all(&content)?;
    output.finish()?;
    Ok(())
}
