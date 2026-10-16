use super::assets::{AssetReport, backup};
use anyhow::Result;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

#[derive(Deserialize)]
struct Display {
    legacy: Map<String, Value>,
    modern: Map<String, Value>,
}

#[derive(Deserialize)]
struct Catalog {
    aliases: BTreeMap<String, String>,
    displays: BTreeMap<String, Display>,
    #[serde(default)]
    references: BTreeMap<String, Value>,
}

static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../assets/legacy-models.json"))
        .expect("旧版方块模型参考表无效")
});

fn id(value: &str) -> String {
    if value.contains(':') {
        value.into()
    } else {
        format!("minecraft:{value}")
    }
}

fn path(root: &Path, id: &str) -> PathBuf {
    let (namespace, name) = id.split_once(':').unwrap();
    root.join(format!("assets/{namespace}/models/{name}.json"))
}

fn write(root: &Path, id: &str, value: &Value) -> Result<()> {
    let path = path(root, id);
    if path.is_file() {
        if serde_json::from_slice::<Value>(&fs::read(&path)?)
            .ok()
            .as_ref()
            == Some(value)
        {
            return Ok(());
        }
        backup(root, &path)?;
    }
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

pub(super) struct Migration {
    models: BTreeMap<String, Value>,
}

pub(super) fn capture(root: &Path, source: u32, target: u32) -> Result<Option<Migration>> {
    let directory = root.join("assets/minecraft/models");
    if source < 4 || target >= 2 || !directory.is_dir() {
        return Ok(None);
    }
    let mut models = BTreeMap::new();
    for entry in walkdir::WalkDir::new(&directory).min_depth(1) {
        let entry = entry?;
        if !entry.file_type().is_file() || entry.path().extension().is_none_or(|ext| ext != "json")
        {
            continue;
        }
        let name = entry
            .path()
            .strip_prefix(&directory)?
            .to_string_lossy()
            .replace('\\', "/");
        if let Ok(value) = serde_json::from_slice(&fs::read(entry.path())?) {
            models.insert(
                format!("minecraft:{}", name.trim_end_matches(".json")),
                value,
            );
        }
    }
    if source >= 22 {
        for kind in ["block", "item"] {
            if let Some(model) = models
                .get(&format!("minecraft:{kind}/short_grass"))
                .cloned()
            {
                models
                    .entry(format!("minecraft:{kind}/grass"))
                    .or_insert(model);
            }
        }
    }
    let mut pending: Vec<_> = models
        .values()
        .filter_map(|value: &Value| value["parent"].as_str().map(id))
        .collect();
    while let Some(name) = pending.pop() {
        if models.contains_key(&name) {
            continue;
        }
        if let Some(reference) = CATALOG.references.get(&name) {
            let mut model = reference.clone();
            if let Some(textures) = model["textures"].as_object_mut() {
                for value in textures.values_mut() {
                    if let Some(reference) = value.as_str() {
                        let reference = reference.trim_start_matches("minecraft:");
                        if let Some((kind, name)) = reference.split_once('/')
                            && matches!(kind, "block" | "item")
                        {
                            let legacy = crate::tables::legacy_texture_name(name, kind)
                                .unwrap_or_else(|| name.into());
                            *value = json!(format!("{kind}s/{legacy}"));
                        }
                    }
                }
            }
            if let Some(parent) = model["parent"].as_str() {
                pending.push(id(parent));
            }
            models.insert(name, model);
        }
    }
    Ok(Some(Migration { models }))
}

impl Migration {
    fn block_refs(&self, value: &Value, names: &mut BTreeSet<String>) {
        match value {
            Value::Object(values) => {
                for (key, value) in values {
                    if matches!(key.as_str(), "parent" | "model")
                        && let Some(reference) = value.as_str()
                    {
                        let reference = id(reference);
                        if reference.starts_with("minecraft:block/")
                            && self.models.contains_key(&reference)
                        {
                            names.insert(reference);
                        }
                    } else {
                        self.block_refs(value, names);
                    }
                }
            }
            Value::Array(values) => {
                for value in values {
                    self.block_refs(value, names);
                }
            }
            _ => {}
        }
    }

    fn reference(&self, name: &str, visited: &mut BTreeSet<String>) -> Option<&Display> {
        if !visited.insert(name.into()) || visited.len() > 64 {
            return None;
        }
        CATALOG.displays.get(name).or_else(|| {
            self.models.get(name)?["parent"]
                .as_str()
                .and_then(|parent| self.reference(&id(parent), visited))
        })
    }

    fn display(&self, name: &str, visited: &mut BTreeSet<String>) -> Map<String, Value> {
        if !visited.insert(name.into()) || visited.len() > 64 {
            return Map::new();
        }
        let Some(model) = self.models.get(name) else {
            return Map::new();
        };
        let mut display = model["parent"]
            .as_str()
            .map(|parent| self.display(&id(parent), visited))
            .unwrap_or_default();
        if let Some(values) = model["display"].as_object() {
            display.extend(values.clone());
        }
        visited.remove(name);
        display
    }

    fn rewrite_refs(&self, value: &mut Value) {
        match value {
            Value::Object(values) => {
                for (key, value) in values {
                    if matches!(key.as_str(), "parent" | "model")
                        && let Some(reference) = value.as_str()
                    {
                        let reference = id(reference);
                        if self.models.contains_key(&reference)
                            && reference.starts_with("minecraft:block/")
                        {
                            *value = json!(format!(
                                "fastpack:legacy_models/{}",
                                reference.trim_start_matches("minecraft:")
                            ));
                        }
                    } else {
                        self.rewrite_refs(value);
                    }
                }
            }
            Value::Array(values) => {
                for value in values {
                    self.rewrite_refs(value);
                }
            }
            _ => {}
        }
    }

    fn convert_display(&self, name: &str, model: &mut Value) {
        let provided = self.display(name, &mut BTreeSet::new());
        if provided.is_empty() {
            return;
        }
        let reference = self
            .reference(name, &mut BTreeSet::new())
            .or_else(|| CATALOG.displays.get("minecraft:block/cube"));
        let Some(reference) = reference else {
            return;
        };
        let mut display = provided.clone();
        for view in ["gui", "firstperson", "thirdperson"] {
            let key = if view == "gui" {
                view.into()
            } else {
                format!("{view}_righthand")
            };
            let Some(input) = provided.get(&key).or_else(|| provided.get(view)) else {
                continue;
            };
            let modern = reference
                .modern
                .get(&key)
                .cloned()
                .unwrap_or_else(|| json!({}));
            let legacy = reference.legacy.get(view).cloned().unwrap_or_else(|| {
                match view {
                    "thirdperson" => json!({"rotation":[10,-45,170],"translation":[0,1.5,-2.75],"scale":[0.375,0.375,0.375]}),
                    _ => json!({}),
                }
            });
            let vector = |value: &Value, key: &str| -> [f64; 3] {
                value
                    .get(key)
                    .and_then(|value| serde_json::from_value(value.clone()).ok())
                    .unwrap_or(if key == "scale" { [1.0; 3] } else { [0.0; 3] })
            };
            let mut converted = json!({});
            for property in ["rotation", "translation", "scale"] {
                let a = vector(input, property);
                let b = vector(&modern, property);
                let c = vector(&legacy, property);
                converted[property] = json!(std::array::from_fn::<_, 3, _>(|axis| {
                    let v = if property == "scale" && b[axis].abs() > f64::EPSILON {
                        a[axis] * c[axis] / b[axis]
                    } else {
                        a[axis] + c[axis] - b[axis]
                    };
                    (v * 1_000_000.0).round() / 1_000_000.0
                }));
            }
            display.insert(view.into(), converted);
            if view != "gui" {
                display.remove(&key);
                display.remove(&format!("{view}_lefthand"));
            }
        }
        model["display"] = Value::Object(display);
    }

    pub(super) fn apply(self, root: &Path, report: &mut AssetReport) -> Result<()> {
        let mut blocks = BTreeSet::new();
        for name in CATALOG.aliases.values() {
            if let Some(model) = self.models.get(name) {
                if name.starts_with("minecraft:block/") {
                    blocks.insert(name.clone());
                }
                self.block_refs(model, &mut blocks);
            }
        }
        let mut pending = blocks.clone();
        while let Some(name) = pending.pop_first() {
            let mut dependencies = BTreeSet::new();
            self.block_refs(&self.models[&name], &mut dependencies);
            for dependency in dependencies {
                if blocks.insert(dependency.clone()) {
                    pending.insert(dependency);
                }
            }
        }
        for name in &blocks {
            let mut model = self.models[name].clone();
            self.convert_display(name, &mut model);
            self.rewrite_refs(&mut model);
            write(
                root,
                &format!(
                    "fastpack:legacy_models/{}",
                    name.trim_start_matches("minecraft:")
                ),
                &model,
            )?;
        }
        let mut converted_items = BTreeMap::new();
        for modern in CATALOG
            .aliases
            .values()
            .filter(|name| name.starts_with("minecraft:item/"))
        {
            if self.models.contains_key(modern) && !converted_items.contains_key(modern) {
                let value = serde_json::from_slice(&fs::read(path(root, modern))?)
                    .unwrap_or_else(|_| self.models[modern].clone());
                converted_items.insert(modern, value);
            }
        }
        let mut bindings = 0;
        for (legacy, modern) in &CATALOG.aliases {
            let Some(original) = self.models.get(modern) else {
                if matches!(
                    legacy.as_str(),
                    "minecraft:block/grass" | "minecraft:item/grass"
                ) && self.models.contains_key(legacy)
                {
                    let conflicting = path(root, legacy);
                    if conflicting.is_file() {
                        backup(root, &conflicting)?;
                        fs::remove_file(conflicting)?;
                    }
                }
                continue;
            };
            let model = if modern.starts_with("minecraft:block/") {
                json!({"parent":format!("fastpack:legacy_models/{}", modern.trim_start_matches("minecraft:"))})
            } else {
                let mut model = if original["parent"]
                    .as_str()
                    .is_some_and(|parent| id(parent).starts_with("minecraft:block/"))
                {
                    let mut model = original.clone();
                    self.convert_display(modern, &mut model);
                    model
                } else {
                    converted_items[modern].clone()
                };
                self.rewrite_refs(&mut model);
                model
            };
            write(root, legacy, &model)?;
            bindings += 1;
        }
        report.converted.push(format!("1.8.9 方块模型：隔离 {} 个新版方块模型，按官方资源语义绑定 {bindings} 个旧版入口，区分草与草方块并换算 GUI / 手持显示参数", blocks.len()));
        Ok(())
    }
}
