use crate::packfs as fs;
use crate::packfs::PathExt;
use anyhow::Result;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

static DEFAULTS: LazyLock<BTreeMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../assets/legacy-defaults.json"))
        .expect("旧版原版资源指纹表无效")
});

static MODERN_DEFAULTS: LazyLock<BTreeMap<String, BTreeSet<String>>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../assets/modern-defaults.json"))
        .expect("新版原版资源指纹表无效")
});

fn canonical(value: Value) -> Value {
    match value {
        Value::Object(values) => {
            let sorted: BTreeMap<_, _> = values.into_iter().collect();
            Value::Object(
                sorted
                    .into_iter()
                    .map(|(key, value)| (key, canonical(value)))
                    .collect(),
            )
        }
        Value::Array(values) => Value::Array(values.into_iter().map(canonical).collect()),
        Value::Number(number) => {
            if let Some(number) = number
                .as_f64()
                .filter(|number| number.fract() == 0.0 && number.abs() < 9_007_199_254_740_992.0)
            {
                Value::from(number as i64)
            } else {
                Value::Number(number)
            }
        }
        value => value,
    }
}

fn model_path(name: &str, blockstate: bool) -> Option<String> {
    let (namespace, name) = name.split_once(':').unwrap_or(("minecraft", name));
    if namespace.is_empty()
        || name.is_empty()
        || !namespace
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c))
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-/".contains(c))
        || name
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return None;
    }
    let name = if blockstate && !name.contains('/') {
        format!("block/{name}")
    } else {
        name.to_string()
    };
    Some(format!("assets/{namespace}/models/{name}.json"))
}

fn dependencies(value: &Value, blockstate: bool, result: &mut BTreeSet<String>) {
    match value {
        Value::Object(values) => {
            for (key, value) in values {
                if matches!(key.as_str(), "parent" | "model")
                    && let Some(name) = value.as_str()
                    && let Some(path) = model_path(name, key == "model" && blockstate)
                {
                    result.insert(path);
                } else {
                    dependencies(value, blockstate, result);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                dependencies(value, blockstate, result);
            }
        }
        _ => {}
    }
}

struct Resource {
    path: PathBuf,
    stock: bool,
    dependencies: BTreeSet<String>,
}

pub(crate) fn prepare(root: &Path, source: u32, target: u32) -> Result<Vec<String>> {
    let downgrade = source >= 4 && target < 2;
    let modern = downgrade || source >= 4 && source < 46 && target >= 46;
    if !modern && (source != 1 || target < 4) {
        return Ok(Vec::new());
    }
    let mut resources = BTreeMap::new();
    for entry in crate::packfs::WalkDir::new(root.join("assets")).min_depth(1) {
        let entry = entry?;
        if !entry.file_type().is_file() || entry.path().extension().is_none_or(|ext| ext != "json")
        {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        if !relative.contains("/models/") && !relative.contains("/blockstates/") {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<Value>(&fs::read(entry.path())?) else {
            continue;
        };
        let bytes = serde_json::to_vec(&canonical(value.clone()))?;
        let digest = format!("{:x}", Sha256::digest(bytes));
        let stock = if modern {
            MODERN_DEFAULTS
                .get(&relative)
                .is_some_and(|expected| expected.contains(&digest))
        } else {
            DEFAULTS
                .get(&relative)
                .is_some_and(|expected| digest == *expected)
        };
        let mut references = BTreeSet::new();
        dependencies(&value, relative.contains("/blockstates/"), &mut references);
        resources.insert(
            relative,
            Resource {
                path: entry.into_path(),
                stock,
                dependencies: references,
            },
        );
    }
    let mut influenced: BTreeSet<String> = resources
        .iter()
        .filter(|(name, resource)| name.contains("/models/") && !resource.stock)
        .map(|(name, _)| name.clone())
        .collect();
    loop {
        let additions: Vec<_> = resources
            .iter()
            .filter(|(name, resource)| {
                name.contains("/models/")
                    && !influenced.contains(*name)
                    && resource
                        .dependencies
                        .iter()
                        .any(|dependency| influenced.contains(dependency))
            })
            .map(|(name, _)| name.clone())
            .collect();
        if additions.is_empty() {
            break;
        }
        influenced.extend(additions);
    }
    let mut protected: BTreeSet<_> = resources
        .iter()
        .filter(|(name, resource)| {
            !resource.stock
                || influenced.contains(*name)
                || resource
                    .dependencies
                    .iter()
                    .any(|dependency| influenced.contains(dependency))
        })
        .map(|(name, _)| name.clone())
        .collect();
    let mut pending: Vec<_> = protected.iter().cloned().collect();
    while let Some(name) = pending.pop() {
        if let Some(resource) = resources.get(&name) {
            for dependency in &resource.dependencies {
                if resources.contains_key(dependency) && protected.insert(dependency.clone()) {
                    pending.push(dependency.clone());
                }
            }
        }
    }
    let mut model_count = 0;
    let mut state_count = 0;
    for (name, resource) in resources {
        if !resource.stock || protected.contains(&name) {
            continue;
        }
        let destination = root
            .join(if modern {
                "fastpack/modern-defaults"
            } else {
                "fastpack/legacy-defaults"
            })
            .join(&name);
        if !destination.pack_exists() {
            fs::create_dir_all(destination.parent().unwrap())?;
            fs::copy(&resource.path, destination)?;
        }
        fs::remove_file(&resource.path)?;
        if name.contains("/models/") {
            model_count += 1;
        } else {
            state_count += 1;
        }
    }
    if model_count + state_count == 0 {
        return Ok(Vec::new());
    }
    Ok(vec![format!(
        "{}原版资源：校验并备份 {model_count} 个未修改模型与 {state_count} 个未修改方块状态，使用目标版本原生资源；保留自定义模型、状态及其依赖",
        if modern { "新版" } else { "旧版" }
    )])
}
