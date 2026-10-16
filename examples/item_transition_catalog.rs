use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use zip::ZipArchive;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 2,
        "item_transition_catalog <reference jars.json> <output.json>"
    );
    let jars: BTreeMap<String, String> = serde_json::from_slice(&fs::read(&args[0])?)?;
    let mut profiles = BTreeMap::new();
    for (version, path) in jars {
        let mut zip = ZipArchive::new(File::open(path)?)?;
        let mut models = BTreeMap::<String, Value>::new();
        let mut definitions = BTreeMap::<String, Value>::new();
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index)?;
            let name = entry.name().to_string();
            let model = name
                .strip_prefix("assets/minecraft/models/item/")
                .and_then(|n| n.strip_suffix(".json"));
            let definition = name
                .strip_prefix("assets/minecraft/items/")
                .and_then(|n| n.strip_suffix(".json"));
            if model.is_none() && definition.is_none() {
                continue;
            }
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            let value = serde_json::from_slice(&bytes)?;
            if let Some(name) = model {
                models.insert(name.into(), value);
            } else if let Some(name) = definition {
                definitions.insert(name.into(), value);
            }
        }
        profiles.insert(
            fast_pack::pack_format_of(&version)?,
            json!({"version":version,"models":models,"definitions":definitions}),
        );
    }
    fs::write(&args[1], serde_json::to_vec(&profiles)?)?;
    Ok(())
}
