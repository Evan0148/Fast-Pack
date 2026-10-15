use anyhow::{Context, Result, ensure};
use fast_pack::flatten_texture_name;
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

fn read(path: &str) -> Result<BTreeMap<String, Value>> {
    let mut zip = ZipArchive::new(File::open(path)?)?;
    let mut result = BTreeMap::new();
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        let Some(name) = entry
            .name()
            .strip_prefix("assets/minecraft/models/")
            .and_then(|name| name.strip_suffix(".json"))
        else {
            continue;
        };
        let name = format!("minecraft:{name}");
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        if let Ok(value) = serde_json::from_slice(&bytes) {
            result.insert(name, value);
        }
    }
    Ok(result)
}

fn resolve(name: &str, models: &BTreeMap<String, Value>, seen: &mut BTreeSet<String>) -> Value {
    if !seen.insert(name.into()) || seen.len() > 64 {
        return json!({});
    }
    let Some(value) = models.get(name) else {
        return json!({});
    };
    let mut result = value["parent"]
        .as_str()
        .map(|parent| resolve(&id(parent), models, seen))
        .unwrap_or_else(|| json!({}));
    for field in ["textures", "display"] {
        if let Some(values) = value[field].as_object() {
            if !result[field].is_object() {
                result[field] = json!({});
            }
            result[field]
                .as_object_mut()
                .unwrap()
                .extend(values.clone());
        }
    }
    if let Some(elements) = value.get("elements") {
        result["elements"] = elements.clone();
    }
    seen.remove(name);
    result
}

fn canonical(value: Value) -> Value {
    match value {
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, canonical(value)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(canonical).collect()),
        Value::Number(number) if number.as_f64().is_some_and(|n| n.fract() == 0.0) => {
            json!(number.as_f64().unwrap() as i64)
        }
        value => value,
    }
}

fn signature(model: &Value, legacy: bool) -> Option<String> {
    let mut elements = model["elements"].as_array()?.clone();
    for element in &mut elements {
        let from: [f64; 3] = serde_json::from_value(element["from"].clone()).ok()?;
        let to: [f64; 3] = serde_json::from_value(element["to"].clone()).ok()?;
        element
            .as_object_mut()?
            .entry("shade")
            .or_insert(json!(true));
        for (direction, face) in element["faces"].as_object_mut()? {
            if face.get("uv").is_none() {
                face["uv"] = match direction.as_str() {
                    "down" => json!([from[0], 16.0 - to[2], to[0], 16.0 - from[2]]),
                    "up" => json!([from[0], from[2], to[0], to[2]]),
                    "north" => json!([16.0 - to[0], 16.0 - to[1], 16.0 - from[0], 16.0 - from[1]]),
                    "south" => json!([from[0], 16.0 - to[1], to[0], 16.0 - from[1]]),
                    "west" => json!([from[2], 16.0 - to[1], to[2], 16.0 - from[1]]),
                    "east" => json!([16.0 - to[2], 16.0 - to[1], 16.0 - from[2], 16.0 - from[1]]),
                    _ => return None,
                };
            }
            let mut texture = face["texture"].as_str()?.to_string();
            let mut seen = BTreeSet::new();
            while let Some(slot) = texture.strip_prefix('#') {
                if !seen.insert(slot.to_string()) {
                    return None;
                }
                texture = model["textures"][slot].as_str()?.into();
            }
            if let Some((kind, name)) = texture.trim_start_matches("minecraft:").split_once('/') {
                let name = if legacy && matches!(kind, "blocks" | "items") {
                    flatten_texture_name(name, if kind == "blocks" { "block" } else { "item" })
                } else {
                    name.into()
                };
                face["texture"] = json!(format!(
                    "{}/{name}",
                    if kind == "blocks" {
                        "block"
                    } else if kind == "items" {
                        "item"
                    } else {
                        kind
                    }
                ));
            }
        }
    }
    serde_json::to_string(&canonical(json!(elements))).ok()
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 3,
        "legacy_model_catalog <1.8.9.jar> <1.20.1.jar> <output.json>"
    );
    let legacy = read(&args[0])?;
    let modern = read(&args[1])?;
    let modern_resolved: BTreeMap<_, _> = modern
        .keys()
        .map(|name| (name.clone(), resolve(name, &modern, &mut BTreeSet::new())))
        .collect();
    let mut signatures = BTreeMap::<String, Vec<String>>::new();
    for (name, value) in &modern_resolved {
        if name.starts_with("minecraft:block/")
            && let Some(signature) = signature(value, false)
        {
            signatures.entry(signature).or_default().push(name.clone());
        }
    }
    let items = &fast_pack::tables::legacy_items().models;
    let mut aliases = BTreeMap::new();
    let mut displays = BTreeMap::new();
    let mut unresolved = Vec::new();
    for name in legacy
        .keys()
        .filter(|name| name.starts_with("minecraft:block/"))
    {
        let resolved = resolve(name, &legacy, &mut BTreeSet::new());
        let matching = signature(&resolved, true).and_then(|signature| signatures.get(&signature));
        let candidate = matching
            .and_then(|names| {
                names
                    .iter()
                    .find(|candidate| *candidate == name)
                    .or_else(|| names.first())
            })
            .cloned()
            .or_else(|| modern.contains_key(name).then(|| name.clone()));
        let candidate = if name == "minecraft:block/grass" {
            Some("minecraft:block/grass_block".into())
        } else {
            candidate
        };
        if let Some(modern_name) = candidate {
            aliases.insert(name.clone(), modern_name.clone());
            displays.insert(modern_name.clone(), json!({"legacy":resolved["display"].as_object().cloned().unwrap_or_default(),"modern":modern_resolved[&modern_name]["display"].as_object().cloned().unwrap_or_default()}));
        } else {
            unresolved.push(name.clone());
        }
    }
    for name in legacy
        .keys()
        .filter(|name| name.starts_with("minecraft:item/"))
    {
        let short = name.strip_prefix("minecraft:item/").unwrap();
        let mapped = if short == "grass" {
            "grass_block"
        } else if short == "tall_grass" {
            "grass"
        } else {
            items.get(short).map(String::as_str).unwrap_or(short)
        };
        let modern_name = format!("minecraft:item/{mapped}");
        if modern.contains_key(&modern_name) {
            let resolved = resolve(name, &legacy, &mut BTreeSet::new());
            displays.insert(modern_name.clone(), json!({"legacy":resolved["display"].as_object().cloned().unwrap_or_default(),"modern":modern_resolved[&modern_name]["display"].as_object().cloned().unwrap_or_default()}));
            aliases.insert(name.clone(), modern_name);
        }
    }
    let references: BTreeMap<_, _> = modern
        .values()
        .filter_map(|value| value["parent"].as_str())
        .map(id)
        .filter(|name| name.starts_with("minecraft:block/"))
        .filter_map(|name| modern.get(&name).map(|value| (name, value.clone())))
        .collect();
    fs::write(
        &args[2],
        serde_json::to_vec(
            &json!({"source":"1.20.1","target":"1.8.9","aliases":aliases,"displays":displays,"references":references}),
        )?,
    )
    .context("无法保存模型映射")?;
    println!(
        "aliases={}, displays={}, unresolved={}",
        aliases.len(),
        displays.len(),
        unresolved.len()
    );
    Ok(())
}
