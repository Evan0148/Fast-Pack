use super::assets::{AssetReport, backup};
use anyhow::Result;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

#[derive(Deserialize)]
struct Profile {
    models: BTreeMap<String, Value>,
    definitions: BTreeMap<String, Value>,
}

static PROFILES: LazyLock<BTreeMap<u32, Profile>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../assets/item-transitions.json"))
        .expect("物品状态参考无效")
});

pub(super) fn upgrade_templates(
    target: u32,
) -> Option<(
    &'static BTreeMap<String, Value>,
    &'static BTreeMap<String, Value>,
)> {
    if target < 69 {
        return None;
    }
    let profile = PROFILES.range(46..=target).next_back()?.1;
    Some((&profile.definitions, &profile.models))
}

pub(super) fn model_reference(target: u32, name: &str) -> Option<Value> {
    PROFILES
        .range(..=target)
        .next_back()?
        .1
        .models
        .get(name)
        .cloned()
}

pub(super) fn override_replacement(base: &str, variant: &str, model: &Value) -> Option<String> {
    let reference = PROFILES.get(&15)?.models.get(base)?;
    let canonical = |name: &str| id(name).replace("_darker_trim", "_trim");
    let sample = reference["overrides"].as_array()?.iter().find(|entry| {
        entry["model"]
            .as_str()
            .is_some_and(|name| canonical(name) == canonical(variant))
    })?["predicate"]
        .as_object()?;
    model["overrides"].as_array()?.iter().rev().find(|entry| {
        entry["predicate"].as_object().is_some_and(|predicate| {
            predicate.iter().all(|(key, value)| {
                let actual = sample.get(key).and_then(Value::as_f64).unwrap_or(0.0);
                value
                    .as_f64()
                    .is_some_and(|threshold| actual + 1e-9 >= threshold)
            })
        })
    })?["model"]
        .as_str()
        .map(id)
}

fn id(name: &str) -> String {
    if name.contains(':') {
        name.into()
    } else {
        format!("minecraft:{name}")
    }
}

fn path(root: &Path, name: &str) -> Option<PathBuf> {
    let name = id(name);
    let (namespace, name) = name.split_once(':')?;
    if namespace.is_empty()
        || name.is_empty()
        || !namespace
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c))
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-/".contains(c))
        || name.split('/').any(|s| matches!(s, "" | "." | ".."))
    {
        return None;
    }
    Some(root.join(format!("assets/{namespace}/models/{name}.json")))
}

fn write(root: &Path, path: &Path, value: &Value) -> Result<()> {
    if let Ok(bytes) = fs::read(path) {
        if serde_json::from_slice::<Value>(&bytes).ok().as_ref() == Some(value) {
            return Ok(());
        }
        backup(root, path)?;
    }
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn pairs(reference: &Value, provided: &Value, mappings: &mut BTreeMap<String, String>) {
    if let (Some(a), Some(b)) = (reference.as_object(), provided.as_object()) {
        for (key, value) in a {
            let Some(other) = b.get(key) else {
                continue;
            };
            if matches!(key.as_str(), "model" | "base") {
                if let (Some(a), Some(b)) = (value.as_str(), other.as_str()) {
                    mappings.insert(id(a), id(b));
                } else {
                    pairs(value, other, mappings);
                }
            } else {
                pairs(value, other, mappings);
            }
        }
    } else if let (Some(a), Some(b)) = (reference.as_array(), provided.as_array()) {
        for (index, left) in a.iter().enumerate() {
            let right = if let Some(when) = left.get("when") {
                b.iter().find(|entry| entry.get("when") == Some(when))
            } else if let Some(threshold) = left.get("threshold") {
                b.iter()
                    .find(|entry| entry.get("threshold") == Some(threshold))
            } else {
                b.get(index)
            };
            if let Some(right) = right {
                pairs(left, right, mappings);
            }
        }
    }
}

fn special(node: &Value) -> bool {
    node["type"]
        .as_str()
        .is_some_and(|t| t.trim_start_matches("minecraft:") == "special")
        || node.get("on_false").is_some_and(special)
        || node.get("fallback").is_some_and(special)
}

fn legacy_textures(model: &mut Value, target: u32) {
    if target >= 4 {
        return;
    }
    if let Some(textures) = model["textures"].as_object_mut() {
        for value in textures.values_mut() {
            let Some(reference) = value.as_str() else {
                continue;
            };
            let (namespace, name) = reference
                .split_once(':')
                .unwrap_or(("minecraft", reference));
            if namespace != "minecraft" {
                continue;
            }
            if let Some((kind, name)) = name.split_once('/')
                && matches!(kind, "block" | "item" | "blocks" | "items")
            {
                let singular = kind.trim_end_matches('s');
                let name = if singular == kind {
                    crate::tables::legacy_texture_name(name, kind).unwrap_or_else(|| name.into())
                } else {
                    name.into()
                };
                *value = json!(format!("minecraft:{singular}s/{name}"));
            }
        }
    }
}

fn special_model(
    native: &Value,
    reference: &str,
    models: &BTreeMap<String, Value>,
    modern: &Profile,
) -> Value {
    fn display(
        name: &str,
        provided: Option<&BTreeMap<String, Value>>,
        modern: &Profile,
        seen: &mut std::collections::BTreeSet<String>,
    ) -> serde_json::Map<String, Value> {
        if seen.len() >= 64 || !seen.insert(name.into()) {
            return serde_json::Map::new();
        }
        let value = provided
            .and_then(|models| models.get(&id(name)))
            .or_else(|| {
                modern
                    .models
                    .get(name.trim_start_matches("minecraft:item/"))
            });
        let Some(value) = value else {
            return serde_json::Map::new();
        };
        let mut result = value["parent"]
            .as_str()
            .map(|p| display(&id(p), provided, modern, seen))
            .unwrap_or_default();
        if let Some(values) = value["display"].as_object() {
            result.extend(values.clone());
        }
        result
    }
    let mut model = native.clone();
    let actual = display(
        &id(reference),
        Some(models),
        modern,
        &mut std::collections::BTreeSet::new(),
    );
    let baseline = display(
        &id(reference),
        None,
        modern,
        &mut std::collections::BTreeSet::new(),
    );
    for (view, value) in actual {
        if baseline.get(&view) == Some(&value) {
            continue;
        }
        let mut converted = json!({});
        for field in ["rotation", "translation", "scale"] {
            let vector = |value: &Value| -> [f64; 3] {
                value
                    .get(field)
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or(if field == "scale" { [1.0; 3] } else { [0.0; 3] })
            };
            let a = vector(&value);
            let b = vector(&baseline.get(&view).cloned().unwrap_or(Value::Null));
            let c = vector(&native["display"][&view]);
            converted[field] = json!(std::array::from_fn::<_, 3, _>(|i| {
                if field == "scale" && b[i] != 0.0 {
                    a[i] * c[i] / b[i]
                } else {
                    a[i] + c[i] - b[i]
                }
            }));
        }
        if !model["display"].is_object() {
            model["display"] = json!({});
        }
        model["display"][view] = converted;
    }
    model
}

pub(super) fn bind(root: &Path, source: u32, target: u32, report: &mut AssetReport) -> Result<()> {
    if source < 46 || target >= 46 {
        return Ok(());
    }
    let old = PROFILES.range(..=target.max(15)).next_back().unwrap().1;
    let modern = PROFILES
        .range(46..=source)
        .next_back()
        .or_else(|| PROFILES.range(46..).next())
        .map(|(_, p)| p);
    let Some(modern) = modern else {
        return Ok(());
    };
    let mut models = BTreeMap::new();
    let mut definitions = Vec::new();
    for entry in walkdir::WalkDir::new(root.join("assets")).min_depth(1) {
        let entry = entry?;
        if !entry.file_type().is_file() || entry.path().extension().is_none_or(|s| s != "json") {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(value) = serde_json::from_slice::<Value>(&fs::read(entry.path())?) else {
            continue;
        };
        if let Some((namespace, name)) = relative
            .strip_prefix("assets/")
            .and_then(|r| r.split_once("/models/"))
        {
            if !namespace.contains('/') {
                models.insert(
                    format!("{namespace}:{}", name.trim_end_matches(".json")),
                    value,
                );
            }
        } else if let Some((namespace, name)) = relative
            .strip_prefix("assets/")
            .and_then(|r| r.split_once("/items/"))
        {
            if !namespace.contains('/') {
                definitions.push((
                    namespace.to_string(),
                    name.trim_end_matches(".json").to_string(),
                    value,
                ));
            }
        }
    }
    let mut bindings = 0;
    let mut states = 0;
    for (namespace, name, definition) in definitions {
        let Some(reference) = super::item_display::definition_model(&definition["model"]) else {
            continue;
        };
        let Some(output) = path(root, &format!("{namespace}:item/{name}")) else {
            continue;
        };
        let legacy = (target < 2 && namespace == "minecraft")
            .then(|| {
                let name = crate::tables::legacy_items()
                    .models
                    .iter()
                    .find_map(|(legacy, modern)| (modern == &name).then(|| legacy.clone()))
                    .unwrap_or_else(|| super::assets::old_model_name(&name));
                super::model_integrity::native_model(target, &format!("minecraft:item/{name}"))
            })
            .flatten();
        let native = (namespace == "minecraft")
            .then(|| old.models.get(&name))
            .flatten();
        let input = models.get(&id(reference));
        let stock = input.is_some_and(|input| {
            modern
                .models
                .get(reference.trim_start_matches("minecraft:item/"))
                .is_some_and(|value| {
                    let mut value = value.clone();
                    legacy_textures(&mut value, target);
                    let mut input = input.clone();
                    legacy_textures(&mut input, target);
                    value == input
                })
        });
        let mut model = if special(&definition["model"]) {
            let Some(native) = legacy.as_ref().or(native) else {
                continue;
            };
            special_model(native, reference, &models, modern)
        } else if stock && legacy.is_some() {
            legacy.clone().unwrap()
        } else if let Some(input) = input {
            input.clone()
        } else if let Some(native) = legacy.as_ref().or(native) {
            native.clone()
        } else {
            continue;
        };
        let mut mappings = BTreeMap::new();
        if let Some(reference) = modern.definitions.get(&name) {
            pairs(reference, &definition, &mut mappings);
        }
        if let Some(overrides) = native.and_then(|model| model["overrides"].as_array()) {
            let mut restored = Vec::new();
            for entry in overrides {
                let Some(variant) = entry["model"].as_str() else {
                    continue;
                };
                let key = id(variant);
                let modern_key = key.replace("_darker_trim", "_trim");
                let replacement = mappings
                    .get(&key)
                    .or_else(|| mappings.get(&modern_key))
                    .unwrap_or(&modern_key);
                let mut converted = entry.clone();
                if let Some(provided) = models.get(replacement) {
                    let mut value = if special(&definition["model"]) {
                        old.models
                            .get(key.trim_start_matches("minecraft:item/"))
                            .map(|native| special_model(native, replacement, &models, modern))
                            .unwrap_or_else(|| provided.clone())
                    } else {
                        provided.clone()
                    };
                    legacy_textures(&mut value, target);
                    if let Some(path) = path(root, &key) {
                        write(root, &path, &value)?;
                    }
                }
                converted["model"] = json!(key);
                restored.push(converted);
            }
            if !restored.is_empty() {
                states += restored.len();
                model["overrides"] = json!(restored);
            }
        }
        legacy_textures(&mut model, target);
        write(root, &output, &model)?;
        bindings += 1;
    }
    if bindings > 0 {
        report.converted.push(format!("新版物品降级：恢复 {bindings} 个默认入口与 {states} 个拉弓、抛竿、时钟、罗盘、弩、盾牌及纹饰状态，特殊渲染器使用目标原版结构"));
    }
    Ok(())
}
