use super::assets::AssetReport;
use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

#[derive(Deserialize)]
struct Profile {
    reference: String,
    definitions: BTreeMap<String, Value>,
    models: BTreeMap<String, Value>,
}

static PROFILES: LazyLock<BTreeMap<u32, Profile>> = LazyLock::new(|| {
    serde_json::from_str(&crate::embedded::text(include_bytes!(concat!(
        env!("OUT_DIR"),
        "/modern-items.deflate"
    ))))
    .expect("新版物品模板无效")
});

fn profile(target: u32) -> Option<&'static Profile> {
    PROFILES
        .range(..=target)
        .next_back()
        .map(|(_, profile)| profile)
}

pub(super) fn model_reference(target: u32, reference: &str) -> Option<Value> {
    profile(target)?
        .models
        .get(reference.strip_prefix("minecraft:")?)
        .cloned()
}

pub(super) fn definition_reference(target: u32, name: &str) -> Option<Value> {
    profile(target)?.definitions.get(name).cloned()
}

fn id(reference: &str) -> Option<String> {
    let (namespace, path) = reference
        .split_once(':')
        .unwrap_or(("minecraft", reference));
    if namespace.is_empty()
        || path.is_empty()
        || !namespace
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c))
        || !path
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-/".contains(c))
        || path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return None;
    }
    let path = if namespace == "minecraft" {
        if let Some(name) = path.strip_prefix("item/") {
            format!(
                "item/{}",
                crate::tables::legacy_items()
                    .models
                    .get(name)
                    .map_or(name, String::as_str)
            )
        } else {
            path.into()
        }
    } else {
        path.into()
    };
    Some(format!("{namespace}:{path}"))
}

fn model_path(root: &Path, reference: &str) -> Option<PathBuf> {
    let reference = id(reference)?;
    let (namespace, name) = reference.split_once(':')?;
    Some(root.join(format!("assets/{namespace}/models/{name}.json")))
}

fn references(value: &Value, models: &mut BTreeSet<String>) {
    match value {
        Value::Object(values) => {
            for (key, value) in values {
                if matches!(key.as_str(), "model" | "base")
                    && let Some(reference) = value.as_str().and_then(id)
                {
                    models.insert(reference);
                } else {
                    references(value, models);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                references(value, models);
            }
        }
        _ => {}
    }
}

fn uploaded_texture(root: &Path, model: &Value) -> bool {
    model
        .get("textures")
        .and_then(Value::as_object)
        .is_some_and(|textures| {
            textures.values().any(|value| {
                value.as_str().and_then(id).is_some_and(|reference| {
                    let (namespace, path) = reference.split_once(':').unwrap();
                    root.join(format!("assets/{namespace}/textures/{path}.png"))
                        .is_file()
                })
            })
        })
}

fn compatible_overrides(model: &Value) -> bool {
    model
        .get("overrides")
        .and_then(Value::as_array)
        .is_some_and(|overrides| {
            overrides.iter().all(|entry| {
                entry
                    .get("predicate")
                    .and_then(Value::as_object)
                    .is_some_and(|predicate| {
                        !predicate.is_empty()
                            && predicate.keys().all(|key| {
                                matches!(
                                    key.as_str(),
                                    "pull" | "pulling" | "cast" | "angle" | "time" | "trim_type"
                                )
                            })
                    })
            })
        })
}

fn replacement(base: &str, reference: &str, model: &Value) -> Option<String> {
    let name = reference.strip_prefix("minecraft:item/")?;
    let (property, threshold) = if base == "bow" {
        (
            "pull",
            match name {
                "bow_pulling_0" => 0.0,
                "bow_pulling_1" => 0.65,
                "bow_pulling_2" => 0.9,
                _ => return None,
            },
        )
    } else if base == "fishing_rod" && name == "fishing_rod_cast" {
        ("cast", 1.0)
    } else if matches!(base, "clock" | "compass") {
        let frame = name
            .strip_prefix(&format!("{base}_"))?
            .parse::<u32>()
            .ok()?;
        (
            if base == "clock" { "time" } else { "angle" },
            f64::from(frame) / if base == "clock" { 64.0 } else { 32.0 },
        )
    } else if let Some(trim) = name
        .strip_prefix(&format!("{base}_"))
        .and_then(|name| name.strip_suffix("_trim"))
    {
        let trim = trim.trim_end_matches("_darker");
        let index = [
            "quartz",
            "iron",
            "netherite",
            "redstone",
            "copper",
            "gold",
            "emerald",
            "diamond",
            "lapis",
            "amethyst",
        ]
        .iter()
        .position(|name| *name == trim)?;
        ("trim_type", (index + 1) as f64 / 10.0)
    } else {
        return None;
    };
    model
        .get("overrides")?
        .as_array()?
        .iter()
        .rev()
        .find(|entry| {
            let Some(predicate) = entry.get("predicate").and_then(Value::as_object) else {
                return false;
            };
            predicate
                .get(property)
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                <= threshold + 1e-9
                && (!predicate.contains_key("pulling")
                    || predicate["pulling"]
                        .as_f64()
                        .is_some_and(|value| value <= 1.0))
                && (predicate.contains_key(property)
                    || property == "pull" && predicate.contains_key("pulling"))
        })
        .and_then(|entry| entry.get("model"))
        .and_then(Value::as_str)
        .and_then(id)
}

fn adapt(value: &mut Value, base: &str, model: &Value, aliases: &mut BTreeMap<String, String>) {
    match value {
        Value::Object(values) => {
            for (key, value) in values {
                if key == "model"
                    && let Some(reference) = value.as_str().and_then(id)
                {
                    if let Some(replacement) = replacement(base, &reference, model) {
                        aliases.insert(replacement.clone(), reference);
                        *value = Value::String(replacement);
                    }
                } else {
                    adapt(value, base, model, aliases);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                adapt(value, base, model, aliases);
            }
        }
        _ => {}
    }
}

pub(super) fn recognized(relative: &str, value: &Value, target: u32) -> bool {
    let Some(name) = relative
        .strip_prefix("assets/minecraft/models/item/")
        .and_then(|name| name.strip_suffix(".json"))
    else {
        return false;
    };
    let name = crate::tables::legacy_items()
        .models
        .get(name)
        .map_or(name, String::as_str);
    target >= 46
        && compatible_overrides(value)
        && profile(target).is_some_and(|profile| profile.definitions.contains_key(name))
}

pub(super) fn migrate(
    root: &Path,
    source: u32,
    target: u32,
    report: &mut AssetReport,
) -> Result<()> {
    if source >= 46 || target < 46 {
        return Ok(());
    }
    let Some(profile) = profile(target) else {
        return Ok(());
    };
    let directory = root.join("assets/minecraft/models/item");
    let mut existing = BTreeMap::new();
    if directory.is_dir() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            if !entry.path().is_file() || entry.path().extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let name = entry
                .path()
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let modern = crate::tables::legacy_items()
                .models
                .get(&name)
                .cloned()
                .unwrap_or(name);
            if let Ok(value) = serde_json::from_slice::<Value>(&fs::read(entry.path())?) {
                existing.entry(modern).or_insert(value);
            }
        }
    }
    let mut definitions = 0;
    let mut models = 0;
    for (name, template) in &profile.definitions {
        let mut native_references = BTreeSet::new();
        references(template, &mut native_references);
        let provided = existing.contains_key(name)
            || native_references.iter().any(|reference| {
                reference
                    .strip_prefix("minecraft:")
                    .and_then(|name| profile.models.get(name))
                    .is_some_and(|model| uploaded_texture(root, model))
            });
        if !provided {
            continue;
        }
        let path = root.join(format!("assets/minecraft/items/{name}.json"));
        if path.is_file() {
            continue;
        }
        let mut definition = template.clone();
        let mut aliases = BTreeMap::new();
        if let Some(model) = existing
            .get(name)
            .filter(|value| compatible_overrides(value))
        {
            adapt(&mut definition, name, model, &mut aliases);
        }
        let mut referenced = BTreeSet::new();
        references(&definition, &mut referenced);
        for reference in referenced {
            let Some(path) = model_path(root, &reference) else {
                continue;
            };
            if path.is_file() {
                continue;
            }
            let original = aliases.get(&reference).unwrap_or(&reference);
            let Some(model) = original
                .strip_prefix("minecraft:")
                .and_then(|name| profile.models.get(name))
            else {
                continue;
            };
            fs::create_dir_all(path.parent().unwrap())?;
            fs::write(path, serde_json::to_vec_pretty(model)?)?;
            models += 1;
        }
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, serde_json::to_vec_pretty(&definition)?)?;
        definitions += 1;
    }
    if definitions > 0 {
        report.generated.push(format!("新版物品定义：按官方 {} 结构生成 {definitions} 个调度 / 染色定义与 {models} 个模型变体，保留已有资源", profile.reference));
    }
    Ok(())
}
