use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use zip::ZipArchive;

fn id(name: &str) -> String {
    if name.contains(':') {
        name.into()
    } else {
        format!("minecraft:{name}")
    }
}
fn refs(value: &Value, names: &mut BTreeSet<String>) {
    match value {
        Value::Object(values) => {
            for (key, value) in values {
                if key == "model" {
                    if let Some(name) = value.as_str() {
                        names.insert(id(name));
                    }
                } else {
                    refs(value, names);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                refs(value, names);
            }
        }
        _ => {}
    }
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 2,
        "model_catalog <profile jars json> <output>"
    );
    let jars: BTreeMap<String, String> = serde_json::from_slice(&fs::read(&args[0])?)?;
    let mut profiles = BTreeMap::new();
    for (version, path) in jars {
        let target = fast_pack::pack_format_of(&version)?;
        let mut archive = ZipArchive::new(File::open(path)?)?;
        let mut models = BTreeMap::new();
        let mut roots = BTreeSet::new();
        let mut textures = BTreeSet::new();
        let mut atlas = None;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            let path = entry.name().to_string();
            let Some(relative) = path.strip_prefix("assets/minecraft/") else {
                continue;
            };
            if let Some(name) = relative
                .strip_prefix("textures/")
                .and_then(|path| path.strip_suffix(".png"))
            {
                textures.insert(format!("minecraft:{name}"));
                continue;
            }
            if !relative.ends_with(".json") {
                continue;
            }
            if !relative.starts_with("models/")
                && !relative.starts_with("blockstates/")
                && !relative.starts_with("items/")
                && relative != "atlases/blocks.json"
            {
                continue;
            }
            let mut bytes = vec![];
            entry.read_to_end(&mut bytes)?;
            let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
                continue;
            };
            if let Some(name) = relative
                .strip_prefix("models/")
                .and_then(|path| path.strip_suffix(".json"))
            {
                let mut slots = BTreeSet::new();
                for element in value["elements"].as_array().into_iter().flatten() {
                    for face in element["faces"]
                        .as_object()
                        .into_iter()
                        .flat_map(|faces| faces.values())
                    {
                        if let Some(texture) = face["texture"].as_str() {
                            slots.insert(texture.to_string());
                        }
                    }
                }
                let mut summary = json!({});
                for field in ["parent", "textures"] {
                    if let Some(value) = value.get(field) {
                        summary[field] = value.clone();
                    }
                }
                if value.get("elements").is_some() {
                    summary["slots"] = json!(slots);
                }
                models.insert(format!("minecraft:{name}"), summary);
                if name.starts_with("item/") {
                    roots.insert(format!("minecraft:{name}"));
                }
            } else if relative == "atlases/blocks.json" {
                atlas = Some(value);
            } else {
                refs(&value, &mut roots);
            }
        }
        let mut sprites = if target < 12 {
            textures.clone()
        } else {
            BTreeSet::new()
        };
        if let Some(atlas) = atlas {
            for source in atlas["sources"].as_array().into_iter().flatten() {
                match source["type"]
                    .as_str()
                    .unwrap_or("")
                    .trim_start_matches("minecraft:")
                {
                    "directory" => {
                        if let (Some(directory), Some(prefix)) =
                            (source["source"].as_str(), source["prefix"].as_str())
                        {
                            for texture in &textures {
                                if let Some(name) =
                                    texture.strip_prefix(&format!("minecraft:{directory}/"))
                                {
                                    sprites.insert(format!("minecraft:{prefix}{name}"));
                                }
                            }
                        }
                    }
                    "single" => {
                        if let Some(texture) = source["resource"].as_str() {
                            sprites.insert(id(source["sprite"].as_str().unwrap_or(texture)));
                        }
                    }
                    "paletted_permutations" => {
                        for texture in source["textures"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str)
                        {
                            for permutation in source["permutations"]
                                .as_object()
                                .into_iter()
                                .flat_map(|values| values.keys())
                            {
                                let texture = format!("{}_{permutation}", id(texture));
                                sprites.insert(texture.clone());
                                textures.insert(texture);
                            }
                        }
                    }
                    "unstitch" => {
                        for region in source["regions"].as_array().into_iter().flatten() {
                            if let Some(sprite) = region["sprite"].as_str() {
                                sprites.insert(id(sprite));
                                textures.insert(id(sprite));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        println!("{version}: {} models, {} roots", models.len(), roots.len());
        profiles.insert(target,json!({"format":target,"version":version,"models":models,"roots":roots,"textures":textures,"sprites":sprites}));
    }
    let mut output = String::new();
    for value in profiles.values() {
        output.push_str(&serde_json::to_string(value)?);
        output.push('\n');
    }
    fs::write(&args[1], output)?;
    Ok(())
}
