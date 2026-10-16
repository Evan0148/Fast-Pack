use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

fn id(value: &str) -> String {
    if value.contains(':') {
        value.into()
    } else {
        format!("minecraft:{value}")
    }
}

fn models(files: &BTreeMap<String, Vec<u8>>) -> BTreeMap<String, Value> {
    files
        .iter()
        .filter_map(|(path, bytes)| {
            let path = path.strip_prefix("assets/")?;
            let (namespace, name) = path.split_once("/models/")?;
            if namespace.contains('/') || !name.ends_with(".json") {
                return None;
            }
            Some((
                format!("{namespace}:{}", name.trim_end_matches(".json")),
                serde_json::from_slice(bytes).ok()?,
            ))
        })
        .collect()
}

fn resolve(
    model: &str,
    values: &BTreeMap<String, Value>,
    seen: &mut BTreeSet<String>,
    issues: &mut BTreeSet<String>,
) -> Value {
    if model.starts_with("minecraft:builtin/") {
        return json!({});
    }
    if !seen.insert(model.into()) {
        issues.insert(format!("parent cycle {model}"));
        return json!({});
    }
    let Some(value) = values.get(model) else {
        issues.insert(format!("missing model {model}"));
        seen.remove(model);
        return json!({});
    };
    let mut result = value
        .get("parent")
        .and_then(Value::as_str)
        .map(|parent| resolve(&id(parent), values, seen, issues))
        .unwrap_or_else(|| json!({}));
    if let Some(textures) = value["textures"].as_object() {
        if !result["textures"].is_object() {
            result["textures"] = json!({});
        }
        result["textures"]
            .as_object_mut()
            .unwrap()
            .extend(textures.clone());
    }
    for field in ["elements", "display", "ambientocclusion"] {
        if let Some(value) = value.get(field) {
            result[field] = value.clone();
        }
    }
    seen.remove(model);
    result
}

fn references(value: &Value, result: &mut BTreeSet<String>) {
    match value {
        Value::Object(values) => {
            for (key, value) in values {
                if matches!(key.as_str(), "model" | "base")
                    && let Some(value) = value.as_str()
                {
                    result.insert(id(value));
                } else {
                    references(value, result);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                references(value, result);
            }
        }
        _ => {}
    }
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 3,
        "audit_models <pack folder> <reference jar> <report>"
    );
    let root = Path::new(&args[0]);
    let mut reference = BTreeMap::new();
    let mut archive = ZipArchive::new(File::open(&args[1])?)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if !entry.is_file() || !entry.name().starts_with("assets/") {
            continue;
        }
        let name = entry.name().to_string();
        let mut bytes = vec![];
        entry.read_to_end(&mut bytes)?;
        reference.insert(name, bytes);
    }
    let mut uploaded = BTreeMap::new();
    for entry in walkdir::WalkDir::new(root.join("assets")) {
        let entry = entry?;
        if entry.file_type().is_file() {
            uploaded.insert(
                entry
                    .path()
                    .strip_prefix(root)?
                    .to_string_lossy()
                    .replace('\\', "/"),
                fs::read(entry.path())?,
            );
        }
    }
    let mut files = reference.clone();
    files.extend(uploaded.clone());
    let all = models(&files);
    let native = models(&reference);
    let mut sprites = BTreeSet::new();
    let mut virtual_textures = BTreeSet::new();
    for collection in [&reference, &uploaded] {
        for (_, bytes) in collection.iter().filter(|(path, _)| {
            path.ends_with("/atlases/blocks.json") || path.ends_with("/atlases/items.json")
        }) {
            let value: Value = serde_json::from_slice(bytes)?;
            for source in value["sources"].as_array().into_iter().flatten() {
                match source["type"]
                    .as_str()
                    .unwrap_or("")
                    .trim_start_matches("minecraft:")
                {
                    "directory" => {
                        if let (Some(directory), Some(prefix)) =
                            (source["source"].as_str(), source["prefix"].as_str())
                        {
                            for path in files.keys() {
                                if let Some((namespace, name)) =
                                    path.strip_prefix("assets/").and_then(|path| {
                                        path.split_once(&format!("/textures/{directory}/"))
                                    })
                                {
                                    if let Some(name) = name.strip_suffix(".png") {
                                        sprites.insert(format!("{namespace}:{prefix}{name}"));
                                    }
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
                            for key in source["permutations"]
                                .as_object()
                                .into_iter()
                                .flat_map(|values| values.keys())
                            {
                                let texture = format!("{}_{key}", id(texture));
                                sprites.insert(texture.clone());
                                virtual_textures.insert(texture);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    let mut roots = BTreeMap::<String, BTreeSet<String>>::new();
    for (path, bytes) in &files {
        if (path.contains("/blockstates/") || path.contains("/items/")) && path.ends_with(".json") {
            if let Ok(value) = serde_json::from_slice(bytes) {
                let mut names = BTreeSet::new();
                references(&value, &mut names);
                for name in names {
                    roots.entry(name).or_default().insert(path.clone());
                }
            }
        } else if path.contains("/models/item/") && path.ends_with(".json") {
            let (namespace, name) = path
                .strip_prefix("assets/")
                .unwrap()
                .split_once("/models/")
                .unwrap();
            roots
                .entry(format!("{namespace}:{}", name.trim_end_matches(".json")))
                .or_default()
                .insert(path.clone());
            if let Ok(value) = serde_json::from_slice(bytes) {
                let mut variants = BTreeSet::new();
                references(&value, &mut variants);
                for name in variants {
                    roots.entry(name).or_default().insert(path.clone());
                }
            }
        }
    }
    let mut broken = vec![];
    let mut missing_sprites = vec![];
    for (model, owners) in &roots {
        let mut issues = BTreeSet::new();
        let value = resolve(model, &all, &mut BTreeSet::new(), &mut issues);
        let mut texture_slots = BTreeSet::new();
        if let Some(elements) = value["elements"].as_array() {
            for element in elements {
                if let Some(faces) = element["faces"].as_object() {
                    for face in faces.values() {
                        if let Some(texture) = face["texture"].as_str() {
                            texture_slots.insert(texture.to_string());
                        }
                    }
                }
            }
        }
        if let Some(textures) = value["textures"].as_object() {
            for (key, texture) in textures {
                if (key == "particle" || key.starts_with("layer")) && texture.is_string() {
                    texture_slots.insert(format!("#{key}"));
                }
            }
        }
        for slot in texture_slots {
            let mut texture = slot.as_str();
            let mut seen = BTreeSet::new();
            while let Some(key) = texture.strip_prefix('#') {
                if !seen.insert(key) {
                    issues.insert(format!("texture variable cycle {slot}"));
                    break;
                }
                let Some(value) = value["textures"].get(key).and_then(Value::as_str) else {
                    issues.insert(format!("unbound {slot}"));
                    break;
                };
                texture = value;
            }
            if texture.starts_with('#') {
                continue;
            }
            let full = id(texture);
            let (namespace, name) = full.split_once(':').unwrap();
            let path = format!("assets/{namespace}/textures/{name}.png");
            if !files.contains_key(&path) && !virtual_textures.contains(&full) {
                issues.insert(format!("missing texture {full}"));
            } else if !sprites.contains(&full) {
                missing_sprites.push(json!({"model":model,"texture":full}));
            }
        }
        if !issues.is_empty() {
            let mut chain = vec![model.clone()];
            let mut current = model.as_str();
            let mut seen = BTreeSet::new();
            while seen.insert(current.to_string()) {
                let Some(parent) = all.get(current).and_then(|value| value["parent"].as_str())
                else {
                    break;
                };
                let parent = id(parent);
                chain.push(parent.clone());
                if chain.len() > 64 {
                    break;
                }
                current = chain.last().unwrap();
            }
            let overrides: Vec<_> = chain
                .iter()
                .filter(|name| {
                    uploaded.contains_key(&format!(
                        "assets/{}/models/{}.json",
                        name.split_once(':').unwrap().0,
                        name.split_once(':').unwrap().1
                    ))
                })
                .collect();
            broken.push(json!({"model":model,"owners":owners,"issues":issues,"parents":chain,"pack_overrides":overrides,"native":native.contains_key(model)}));
        }
    }
    let result = json!({"pack_models":models(&uploaded).len(),"native_models":native.len(),"render_roots":roots.len(),"broken_count":broken.len(),"broken":broken,"missing_sprites":missing_sprites});
    fs::write(&args[2], serde_json::to_vec_pretty(&result)?)?;
    println!(
        "models={}, roots={}, broken={}",
        result["pack_models"], result["render_roots"], result["broken_count"]
    );
    Ok(())
}
