use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use zip::ZipArchive;

const CHAIN: &[(&str, u32)] = &[
    ("1.16.2", 6),
    ("1.17", 7),
    ("1.18", 8),
    ("1.19", 9),
    ("1.19.3", 12),
    ("1.19.4", 13),
    ("1.20", 15),
    ("1.20.2", 18),
    ("1.20.3", 22),
    ("1.20.5", 32),
    ("1.21", 34),
    ("1.21.2", 42),
    ("1.21.4", 46),
    ("1.21.5", 55),
    ("1.21.6", 63),
    ("1.21.7", 64),
    ("1.21.9", 69),
    ("1.21.11", 75),
    ("26.1", 84),
    ("26.2", 88),
    ("26.3", 97),
    ("26.4", 98),
];

fn read(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn name(value: &str) -> &str {
    value.strip_prefix("minecraft:").unwrap_or(value)
}

fn mapped(item: &str, diff: &Value) -> Option<String> {
    if let Some(value) = diff
        .get("items")
        .and_then(|items| items.get(item))
        .and_then(Value::as_str)
    {
        return Some(name(value).into());
    }
    let materials = diff.get("materials")?.as_object()?;
    let padded = format!("_{item}_");
    let mut choices: Vec<_> = materials
        .iter()
        .filter(|(key, _)| padded.contains(&format!("_{key}_")))
        .collect();
    choices.sort_by_key(|(key, _)| std::cmp::Reverse(key.split('_').count()));
    let (material, value) = choices.first()?;
    let replacement = value.as_str().or_else(|| value.get("to")?.as_str())?;
    let mut output = padded.replace(&format!("_{material}_"), &format!("_{replacement}_"));
    if let Some(words) = value.get("words").and_then(Value::as_object) {
        for (from, to) in words {
            output = output.replace(&format!("_{from}_"), &format!("_{}_", to.as_str()?));
        }
    }
    Some(output.trim_matches('_').into())
}

fn archive(path: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut zip = ZipArchive::new(File::open(path)?)?;
    let mut result = BTreeMap::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        if entry.name().starts_with("assets/minecraft/")
            && (entry.name().ends_with(".json")
                || entry.name().ends_with(".png")
                || entry.name().ends_with(".png.mcmeta"))
        {
            let mut data = Vec::new();
            entry.read_to_end(&mut data)?;
            result.insert(entry.name().to_string(), data);
        }
    }
    Ok(result)
}

fn default_model(value: &Value) -> Option<&str> {
    let kind = value.get("type")?.as_str()?;
    match name(kind) {
        "model" => value.get("model")?.as_str(),
        "condition" => default_model(value.get("on_false")?),
        "select" | "range_dispatch" => {
            value.get("fallback").and_then(default_model).or_else(|| {
                value
                    .get("cases")
                    .or_else(|| value.get("entries"))?
                    .as_array()?
                    .first()?
                    .get("model")
                    .and_then(default_model)
            })
        }
        "special" => None,
        "composite" => value
            .get("models")?
            .as_array()?
            .first()
            .and_then(default_model),
        _ => None,
    }
}

fn cube(from: [f64; 3], size: [f64; 3], uv: [f64; 2], texture: &str) -> Value {
    let [w, h, d] = size;
    let [u, v] = uv;
    let mut faces = serde_json::Map::new();
    for (name, rect) in [
        ("down", [u + d, v, u + d + w, v + d]),
        ("up", [u + d + w, v, u + d + 2.0 * w, v + d]),
        ("west", [u, v + d, u + d, v + d + h]),
        ("north", [u + d, v + d, u + d + w, v + d + h]),
        ("east", [u + d + w, v + d, u + 2.0 * d + w, v + d + h]),
        (
            "south",
            [u + 2.0 * d + w, v + d, u + 2.0 * d + 2.0 * w, v + d + h],
        ),
    ] {
        faces.insert(
            name.into(),
            json!({"texture":texture,"uv":rect.map(|value|value/4.0)}),
        );
    }
    json!({"from":from,"to":std::array::from_fn::<_,3,_>(|i|from[i]+size[i]),"faces":faces})
}

fn node(value: &Value) -> &Value {
    match value.get("type").and_then(Value::as_str).map(name) {
        Some("condition") => node(&value["on_false"]),
        Some("select" | "range_dispatch") if value.get("fallback").is_some() => {
            node(&value["fallback"])
        }
        _ => value,
    }
}

fn entity_item(
    item: &str,
    definition: &Value,
    source: &BTreeMap<String, Vec<u8>>,
) -> Option<(String, Value, String)> {
    let node = node(&definition["model"]);
    if node["type"].as_str().map(name) != Some("special") {
        return None;
    }
    let special = &node["model"];
    let kind = name(special["type"].as_str()?);
    let texture = name(special["texture"].as_str()?);
    let texture = match kind {
        "chest" => format!("entity/chest/{texture}"),
        _ => return None,
    };
    if !source.contains_key(&format!("assets/minecraft/textures/{texture}.png")) {
        return None;
    }
    let base = node["base"]
        .as_str()
        .map(name)
        .and_then(|base| source.get(&format!("assets/minecraft/models/{base}.json")))
        .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
        .unwrap_or_else(|| json!({}));
    let elements = vec![
        cube([1.0, 0.0, 1.0], [14.0, 10.0, 14.0], [0.0, 19.0], "#body"),
        cube([1.0, 10.0, 1.0], [14.0, 5.0, 14.0], [0.0, 0.0], "#body"),
        cube([7.0, 8.0, 0.0], [2.0, 4.0, 1.0], [0.0, 0.0], "#body"),
    ];
    let mut model = json!({"textures":{"body":format!("minecraft:{texture}")},"elements":elements});
    if let Some(display) = base.get("display") {
        model["display"] = display.clone();
    } else {
        model["parent"] = json!("minecraft:block/block");
    }
    Some((format!("item/fastpack_bridge/{item}"), model, texture))
}

fn gather(
    model: &str,
    source: &BTreeMap<String, Vec<u8>>,
    models: &mut BTreeMap<String, Value>,
    textures: &mut BTreeSet<String>,
) -> Result<()> {
    let model = name(model);
    if model.starts_with("builtin/") || models.contains_key(model) {
        return Ok(());
    }
    let Some(data) = source.get(&format!("assets/minecraft/models/{model}.json")) else {
        return Ok(());
    };
    let value: Value = serde_json::from_slice(data)?;
    models.insert(model.into(), value.clone());
    if let Some(parent) = value.get("parent").and_then(Value::as_str) {
        gather(parent, source, models, textures)?;
    }
    if let Some(values) = value.get("textures").and_then(Value::as_object) {
        for value in values
            .values()
            .filter_map(Value::as_str)
            .filter(|value| !value.starts_with('#'))
        {
            textures.insert(name(value).into());
        }
    }
    if let Some(overrides) = value.get("overrides").and_then(Value::as_array) {
        for entry in overrides {
            if let Some(model) = entry.get("model").and_then(Value::as_str) {
                gather(model, source, models, textures)?;
            }
        }
    }
    Ok(())
}

fn renderable(model: &str, models: &BTreeMap<String, Value>) -> bool {
    let mut next = name(model);
    let mut seen = BTreeSet::new();
    while seen.len() < 64 && seen.insert(next) {
        if next == "builtin/generated" {
            return true;
        }
        let Some(value) = models.get(next) else {
            return false;
        };
        if let Some(elements) = value.get("elements").and_then(Value::as_array) {
            return !elements.is_empty();
        }
        let Some(parent) = value.get("parent").and_then(Value::as_str) else {
            return false;
        };
        next = name(parent);
    }
    false
}

fn main() -> Result<()> {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let mappings = &args[0];
    let newest = archive(&args[1])?;
    let base = archive(&args[2])?;
    let output = &args[3];
    let mut registries = Vec::new();
    let mut diffs = Vec::new();
    for &(version, _) in CHAIN {
        registries.push(
            read(&mappings.join(format!("mapping-{version}.json")))?["items"]
                .as_array()
                .context("Items missing")?
                .iter()
                .map(|v| name(v.as_str().unwrap()).to_string())
                .collect::<Vec<_>>(),
        );
    }
    diffs.push(json!({}));
    for i in 1..CHAIN.len() {
        diffs.push(read(&mappings.join(format!(
            "diff/mapping-{}to{}.json",
            CHAIN[i].0,
            CHAIN[i - 1].0
        )))?);
    }
    let mut items = BTreeMap::new();
    let mut models = BTreeMap::new();
    let mut textures = BTreeSet::new();
    let mut profiles = BTreeMap::new();
    let mut missing = BTreeSet::new();
    for client in 0..CHAIN.len() - 1 {
        let mut rules = Vec::new();
        let mut carriers = BTreeMap::new();
        for introduced in client + 1..CHAIN.len() {
            let Some(commands) = diffs[introduced]
                .get("custom_model_data")
                .and_then(Value::as_object)
            else {
                continue;
            };
            for (item, cmd) in commands {
                if registries[client].iter().any(|value| value == item) {
                    continue;
                }
                let Some(original_id) = registries[introduced]
                    .iter()
                    .position(|value| value == item)
                else {
                    continue;
                };
                let mut carrier = item.clone();
                for boundary in (client + 1..=introduced).rev() {
                    if registries[boundary - 1]
                        .iter()
                        .any(|value| value == &carrier)
                    {
                        continue;
                    }
                    let Some(mapped) = mapped(&carrier, &diffs[boundary]) else {
                        carrier.clear();
                        break;
                    };
                    carrier = mapped;
                }
                if carrier.is_empty() || !registries[client].iter().any(|v| v == &carrier) {
                    missing.insert(item.clone());
                    continue;
                }
                if !items.contains_key(item) {
                    let definition = newest
                        .get(&format!("assets/minecraft/items/{item}.json"))
                        .and_then(|v| serde_json::from_slice::<Value>(v).ok());
                    let model = definition
                        .as_ref()
                        .and_then(|definition| entity_item(item, definition, &newest))
                        .map(|(id, value, texture)| {
                            models.insert(id.clone(), value);
                            textures.insert(texture);
                            id
                        })
                        .or_else(|| {
                            definition
                                .as_ref()
                                .and_then(|v| v.get("model"))
                                .and_then(default_model)
                                .map(name)
                                .map(str::to_string)
                        })
                        .or_else(|| {
                            newest
                                .contains_key(&format!("assets/minecraft/models/item/{item}.json"))
                                .then(|| format!("item/{item}"))
                        });
                    let Some(model) = model else {
                        missing.insert(item.clone());
                        continue;
                    };
                    gather(&model, &newest, &mut models, &mut textures)?;
                    if !renderable(&model, &models) {
                        missing.insert(item.clone());
                        continue;
                    }
                    items.insert(item.clone(), json!({"model":model}));
                }
                let older = match CHAIN[introduced].0 {
                    "1.17" => "1.16.4",
                    "1.19" => "1.18.2",
                    "1.19.3" => "1.19.1",
                    _ => CHAIN[introduced - 1].0,
                };
                let key = format!(
                    "VB|Protocol{}To{}|id",
                    CHAIN[introduced].0.replace('.', "_"),
                    older.replace('.', "_")
                );
                rules.push(
                    json!({"item":item,"carrier":carrier,"cmd":cmd,"key":key,"id":original_id}),
                );
                if !carriers.contains_key(&carrier) {
                    let path = format!("assets/minecraft/models/item/{carrier}.json");
                    let value=base.get(&path).or_else(||newest.get(&path)).and_then(|bytes|serde_json::from_slice::<Value>(bytes).ok()).unwrap_or_else(||json!({"parent":"minecraft:item/generated","textures":{"layer0":format!("minecraft:item/{carrier}")}}));
                    carriers.insert(carrier, value);
                }
            }
        }
        rules.sort_by_key(|rule| rule["cmd"].as_u64().unwrap_or(0));
        profiles.insert(
            CHAIN[client].1,
            json!({"version":CHAIN[client].0,"rules":rules,"carriers":carriers}),
        );
    }
    fs::create_dir_all(output.join("textures"))?;
    let mut copied = 0;
    for texture in &textures {
        let path = format!("assets/minecraft/textures/{texture}.png");
        if let Some(bytes) = newest.get(&path) {
            let destination = output.join("textures").join(format!("{texture}.png"));
            fs::create_dir_all(destination.parent().unwrap())?;
            fs::write(&destination, bytes)?;
            if let Some(metadata) = newest.get(&format!("{path}.mcmeta")) {
                fs::write(destination.with_extension("png.mcmeta"), metadata)?;
            }
            copied += 1;
        }
    }
    let native_textures: Vec<_> = base
        .keys()
        .filter_map(|path| {
            path.strip_prefix("assets/minecraft/textures/")
                .and_then(|p| p.strip_suffix(".png"))
        })
        .collect();
    let languages: BTreeMap<String, Value> = ["en_us", "zh_cn", "zh_tw"]
        .into_iter()
        .filter_map(|language| {
            let value: Value = args
                .get(4)
                .and_then(|folder| fs::read(folder.join(format!("{language}.json"))).ok())
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .or_else(|| {
                    serde_json::from_slice(
                        newest.get(&format!("assets/minecraft/lang/{language}.json"))?,
                    )
                    .ok()
                })?;
            let translations: serde_json::Map<String, Value> = value
                .as_object()?
                .iter()
                .filter(|(key, _)| {
                    key.starts_with("item.minecraft.") || key.starts_with("block.minecraft.")
                })
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            Some((language.into(), Value::Object(translations)))
        })
        .collect();
    let catalog = json!({"reference":"Minecraft Java 26.4 snapshot 3 / 1.20.1; independently generated registry facts","profiles":profiles,"items":items,"models":models,"native_textures":native_textures,"languages":languages});
    fs::write(output.join("catalog.json"), serde_json::to_vec(&catalog)?)?;
    let mut image_source = String::from(
        "pub(super) fn get(name: &str) -> Option<&'static [u8]> {\n    match name {\n",
    );
    let mut paths: Vec<_> = walkdir::WalkDir::new(output.join("textures"))
        .into_iter()
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .collect();
    paths.sort();
    for path in paths {
        let relative = path
            .strip_prefix(output.join("textures"))?
            .to_string_lossy()
            .replace('\\', "/");
        image_source.push_str(&format!(
            "        {relative:?} => Some(include_bytes!({:?})),\n",
            format!("textures/{relative}")
        ));
    }
    image_source.push_str("        _ => None,\n    }\n}\n");
    fs::write(output.join("images.rs"), image_source)?;
    fs::write(
        output.join("generation-evidence.json"),
        serde_json::to_vec_pretty(
            &json!({"items":catalog["items"].as_object().unwrap().len(),"models":catalog["models"].as_object().unwrap().len(),"textures":copied,"missing":missing}),
        )?,
    )?;
    ensure!(
        catalog["profiles"]["15"]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["item"] == "mace" && v["cmd"] == 647),
        "Mace mapping missing"
    );
    println!(
        "Items: {}; models: {}; textures: {}; missing mappings: {}",
        catalog["items"].as_object().unwrap().len(),
        catalog["models"].as_object().unwrap().len(),
        copied,
        missing.len()
    );
    Ok(())
}
