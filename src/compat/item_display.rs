use anyhow::Result;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

#[derive(Clone, Debug, Deserialize, serde::Serialize)]
struct Transform {
    #[serde(default)]
    rotation: [f64; 3],
    #[serde(default)]
    translation: [f64; 3],
    #[serde(default = "unit_scale")]
    scale: [f64; 3],
}

fn unit_scale() -> [f64; 3] {
    [1.0; 3]
}

#[derive(Deserialize)]
struct Profile {
    parent: String,
    legacy: BTreeMap<String, Transform>,
    modern: BTreeMap<String, Transform>,
}

#[derive(Deserialize)]
struct Profiles {
    profiles: BTreeMap<String, Profile>,
    models: BTreeMap<String, String>,
}

static PROFILES: LazyLock<Profiles> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../assets/item-display.json"))
        .expect("物品手持姿态参考表无效")
});

fn resource(name: &str) -> Option<String> {
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
    Some(format!("{namespace}:{name}"))
}

fn model_path(root: &Path, name: &str) -> Option<PathBuf> {
    let id = resource(name)?;
    let (namespace, name) = id.split_once(':')?;
    Some(root.join(format!("assets/{namespace}/models/{name}.json")))
}

pub(super) fn definition_model(node: &Value) -> Option<&str> {
    match node
        .get("type")
        .and_then(Value::as_str)?
        .strip_prefix("minecraft:")
        .unwrap_or(node["type"].as_str()?)
    {
        "model" => node.get("model").and_then(Value::as_str),
        "condition" => definition_model(node.get("on_false")?),
        "select" | "range_dispatch" => definition_model(node.get("fallback")?),
        _ => None,
    }
}

pub(super) fn bind_definitions(
    root: &Path,
    source: u32,
    target: u32,
    report: &mut super::assets::AssetReport,
) -> Result<()> {
    if source < 46 || target >= 46 {
        return Ok(());
    }
    let mut count = 0;
    for entry in walkdir::WalkDir::new(root.join("assets")).min_depth(1) {
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
        let Some((namespace, item)) = relative
            .strip_prefix("assets/")
            .and_then(|path| path.split_once("/items/"))
        else {
            continue;
        };
        if namespace.contains('/') {
            continue;
        }
        let Ok(definition) = serde_json::from_slice::<Value>(&fs::read(entry.path())?) else {
            continue;
        };
        let Some(model) = definition.get("model").and_then(definition_model) else {
            continue;
        };
        let Some(input) = model_path(root, model) else {
            continue;
        };
        if !input.is_file() {
            continue;
        }
        let Some(output) = model_path(
            root,
            &format!("{namespace}:item/{}", item.trim_end_matches(".json")),
        ) else {
            continue;
        };
        if input == output {
            continue;
        }
        let data = fs::read(&input)?;
        if output.is_file() {
            if fs::read(&output)? == data {
                continue;
            }
            super::assets::backup(root, &output)?;
        }
        fs::create_dir_all(output.parent().unwrap())?;
        fs::write(output, data)?;
        count += 1;
    }
    if count > 0 {
        report.converted.push(format!(
            "物品定义：将 {count} 个新版默认模型绑定到旧版物品入口"
        ));
    }
    Ok(())
}

pub(super) struct ModelResolver {
    models: BTreeMap<String, Value>,
}

impl ModelResolver {
    pub(super) fn load(root: &Path, paths: &[PathBuf]) -> Result<Self> {
        let mut models = BTreeMap::new();
        for path in paths {
            let relative = path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            let Some((namespace, name)) = relative
                .strip_prefix("assets/")
                .and_then(|path| path.split_once("/models/"))
            else {
                continue;
            };
            if namespace.contains('/') {
                continue;
            }
            let Some(id) = resource(&format!("{namespace}:{}", name.trim_end_matches(".json")))
            else {
                continue;
            };
            if let Ok(value) = serde_json::from_slice::<Value>(&fs::read(path)?) {
                models.insert(id, value);
            }
        }
        Ok(Self { models })
    }

    fn reference(id: &str) -> Option<Value> {
        let name = id.strip_prefix("minecraft:item/")?;
        let key = match name {
            "generated" => "profile_0",
            "handheld" => "profile_2",
            "handheld_rod" => "profile_5",
            _ => return super::item_bindings::reference(name).cloned(),
        };
        let profile = &PROFILES.profiles[key];
        Some(json!({"parent":"builtin/generated", "display":profile.modern}))
    }

    fn resolve(&self, value: &Value, visited: &mut BTreeSet<String>) -> Option<Value> {
        if value.get("elements").is_some() {
            return None;
        }
        let parent = resource(value.get("parent")?.as_str()?)?;
        let mut resolved = if parent == "minecraft:builtin/generated" {
            json!({"parent":"builtin/generated"})
        } else {
            if visited.len() >= 64 || !visited.insert(parent.clone()) {
                return None;
            }
            let model = self
                .models
                .get(&parent)
                .cloned()
                .or_else(|| Self::reference(&parent))?;
            let resolved = self.resolve(&model, visited)?;
            visited.remove(&parent);
            resolved
        };
        for field in ["display", "textures"] {
            if let Some(values) = value.get(field).and_then(Value::as_object) {
                if !resolved.get(field).is_some_and(Value::is_object) {
                    resolved[field] = json!({});
                }
                resolved[field].as_object_mut()?.extend(values.clone());
            }
        }
        Some(resolved)
    }

    pub(super) fn materialize(&self, value: &mut Value, relative: &str) -> bool {
        if !relative.contains("/models/item/") {
            return false;
        }
        let Some(mut inherited) = self.resolve(value, &mut BTreeSet::new()) else {
            return false;
        };
        let old = value.clone();
        for field in ["display", "textures"] {
            if let Some(content) = inherited.get_mut(field) {
                if field == "textures" {
                    legacy_texture_refs(content);
                }
                value[field] = content.clone();
            }
        }
        value["parent"] = json!("builtin/generated");
        value.as_object_mut().unwrap().remove("overrides");
        *value != old
    }
}

fn legacy_texture_refs(value: &mut Value) {
    match value {
        Value::Object(values) => {
            for value in values.values_mut() {
                legacy_texture_refs(value);
            }
        }
        Value::String(text) => {
            let (namespace, path) = text.split_once(':').unwrap_or(("minecraft", text));
            let Some((kind, name)) = path.split_once('/') else {
                return;
            };
            if !matches!(kind, "block" | "item") {
                return;
            }
            let path = Path::new(name);
            let stem = path.file_name().unwrap_or_default().to_string_lossy();
            let mapped = if namespace == "minecraft" {
                crate::legacy_texture_name(&stem, kind).unwrap_or_else(|| stem.to_string())
            } else {
                stem.to_string()
            };
            let name = path
                .with_file_name(mapped)
                .to_string_lossy()
                .replace('\\', "/");
            *text = format!("{namespace}:{kind}s/{name}");
        }
        _ => {}
    }
}

fn generated(value: &Value) -> bool {
    value.get("elements").is_none()
        && matches!(
            value.get("parent").and_then(Value::as_str),
            Some(
                "builtin/generated"
                    | "minecraft:builtin/generated"
                    | "item/generated"
                    | "minecraft:item/generated"
                    | "item/handheld"
                    | "minecraft:item/handheld"
                    | "item/handheld_rod"
                    | "minecraft:item/handheld_rod"
            )
        )
}

fn profile(value: &Value, relative: &str) -> Option<&'static Profile> {
    if !generated(value) {
        return None;
    }
    if let Some(name) = relative
        .strip_prefix("assets/minecraft/models/item/")
        .and_then(|name| name.strip_suffix(".json"))
    {
        let name = crate::tables::legacy_items()
            .models
            .get(name)
            .map_or(name, String::as_str);
        if let Some(key) = PROFILES.models.get(name) {
            return PROFILES.profiles.get(key);
        }
    }
    if let Some(parent) = value.get("parent").and_then(Value::as_str) {
        let name = parent.strip_prefix("minecraft:").unwrap_or(parent);
        let key = match name {
            "item/handheld" => Some("profile_2"),
            "item/handheld_rod" => Some("profile_5"),
            "item/generated" => Some("profile_0"),
            _ => None,
        };
        if let Some(key) = key {
            return PROFILES.profiles.get(key);
        }
    }
    let display = value.get("display")?;
    let first = display
        .get("firstperson")
        .and_then(|value| serde_json::from_value::<Transform>(value.clone()).ok());
    let third = display
        .get("thirdperson")
        .and_then(|value| serde_json::from_value::<Transform>(value.clone()).ok());
    PROFILES
        .profiles
        .values()
        .find(|profile| {
            third.as_ref().is_some_and(|transform| {
                transform.rotation == profile.legacy["thirdperson"].rotation
                    && transform.translation == profile.legacy["thirdperson"].translation
            }) && first.as_ref().is_none_or(|transform| {
                transform.rotation == profile.legacy["firstperson"].rotation
            })
        })
        .or_else(|| {
            let third = display
                .get("thirdperson_righthand")
                .and_then(|value| serde_json::from_value::<Transform>(value.clone()).ok());
            PROFILES.profiles.values().find(|profile| {
                third.as_ref().is_some_and(|transform| {
                    transform.rotation == profile.modern["thirdperson_righthand"].rotation
                        && transform.translation
                            == profile.modern["thirdperson_righthand"].translation
                })
            })
        })
        .or_else(|| PROFILES.profiles.get("profile_0"))
}

fn rounded(value: f64) -> f64 {
    (value * 1_000_000_000.0).round() / 1_000_000_000.0
}

fn convert_transform(
    value: &Value,
    from: &Transform,
    to: &Transform,
    mirror: bool,
) -> Option<Value> {
    let input = serde_json::from_value::<Transform>(value.clone()).ok()?;
    let mut output = value.clone();
    let mut rotation = [0.0; 3];
    let mut translation = [0.0; 3];
    let mut scale = [0.0; 3];
    for axis in 0..3 {
        let sign = if mirror && axis > 0 { -1.0 } else { 1.0 };
        rotation[axis] =
            rounded(to.rotation[axis] + sign * (input.rotation[axis] - from.rotation[axis]));
        translation[axis] =
            rounded(to.translation[axis] + input.translation[axis] - from.translation[axis]);
        scale[axis] = rounded(if from.scale[axis] == 0.0 {
            input.scale[axis]
        } else {
            to.scale[axis] * input.scale[axis] / from.scale[axis]
        });
    }
    output["rotation"] = json!(rotation);
    output["translation"] = json!(translation);
    output["scale"] = json!(scale);
    Some(output)
}

pub(super) fn migrate(value: &mut Value, source: u32, target: u32, relative: &str) -> bool {
    let old = value.clone();
    let reference = profile(value, relative);
    if source >= 2
        && target < 2
        && reference.is_some()
        && !value.get("display").is_some_and(Value::is_object)
    {
        value["display"] = Value::Object(Map::new());
    }
    if let Some(parent) = value.get_mut("parent").filter(|parent| parent.is_string()) {
        let text = parent.as_str().unwrap();
        if source < 2
            && target >= 2
            && matches!(text, "builtin/generated" | "minecraft:builtin/generated")
        {
            *parent = json!(reference.map_or("minecraft:item/generated", |profile| {
                profile.parent.as_str()
            }));
        } else if target < 2
            && source >= 2
            && matches!(
                text,
                "item/generated"
                    | "minecraft:item/generated"
                    | "item/handheld"
                    | "minecraft:item/handheld"
                    | "item/handheld_rod"
                    | "minecraft:item/handheld_rod"
            )
        {
            *parent = json!("builtin/generated");
        }
    }
    if let Some(display) = value.get_mut("display").and_then(Value::as_object_mut) {
        for view in ["firstperson", "thirdperson"] {
            if source < 2 && target >= 2 {
                let Some(transform) = display.remove(view) else {
                    continue;
                };
                for hand in ["righthand", "lefthand"] {
                    let key = format!("{view}_{hand}");
                    let converted = reference.and_then(|profile| {
                        convert_transform(
                            &transform,
                            &profile.legacy[view],
                            &profile.modern[&key],
                            hand == "lefthand"
                                && profile.modern[&key].rotation
                                    != profile.modern[&format!("{view}_righthand")].rotation,
                        )
                    });
                    display
                        .entry(key)
                        .or_insert_with(|| converted.unwrap_or_else(|| transform.clone()));
                }
            } else if target < 2 && source >= 2 {
                let key = format!("{view}_righthand");
                let transform = display
                    .get(&key)
                    .cloned()
                    .or_else(|| reference.map(|profile| json!(profile.modern[&key])));
                if let Some(transform) = transform {
                    let converted = reference.and_then(|profile| {
                        convert_transform(
                            &transform,
                            &profile.modern[&key],
                            &profile.legacy[view],
                            false,
                        )
                    });
                    display.insert(view.into(), converted.unwrap_or(transform));
                }
                display.remove(&key);
                display.remove(&format!("{view}_lefthand"));
            }
        }
    }
    *value != old
}

fn equivalent(left: &Value, right: &Value) -> bool {
    let Ok(left) = serde_json::from_value::<Transform>(left.clone()) else {
        return false;
    };
    let Ok(right) = serde_json::from_value::<Transform>(right.clone()) else {
        return false;
    };
    left.rotation == right.rotation
        && left.translation == right.translation
        && left.scale == right.scale
}

pub(super) fn recover(root: &Path, relative: &str, value: &mut Value, target: u32) -> Result<bool> {
    if target < 2 || !generated(value) {
        return Ok(false);
    }
    let original_root = root.join("fastpack/original-assets");
    let mut original = original_root.join(relative);
    if !original.is_file()
        && let Some(name) = relative
            .strip_prefix("assets/minecraft/models/item/")
            .and_then(|name| name.strip_suffix(".json"))
    {
        for (legacy, modern) in &crate::tables::legacy_items().models {
            if modern == name {
                let candidate =
                    original_root.join(format!("assets/minecraft/models/item/{legacy}.json"));
                if candidate.is_file() {
                    original = candidate;
                    break;
                }
            }
        }
    }
    if !original.is_file() {
        return Ok(false);
    }
    let Ok(old) = serde_json::from_slice::<Value>(&fs::read(original)?) else {
        return Ok(false);
    };
    if !generated(&old) || profile(&old, relative).is_none() {
        return Ok(false);
    }
    let Some(original_display) = old.get("display").and_then(Value::as_object) else {
        return Ok(false);
    };
    let Some(display) = value.get_mut("display").and_then(Value::as_object_mut) else {
        return Ok(false);
    };
    let mut recovered = false;
    for view in ["firstperson", "thirdperson"] {
        let Some(transform) = original_display.get(view) else {
            continue;
        };
        let right = format!("{view}_righthand");
        let left = format!("{view}_lefthand");
        if !display.contains_key(view)
            && display
                .get(&right)
                .is_some_and(|value| equivalent(value, transform))
            && display
                .get(&left)
                .is_some_and(|value| equivalent(value, transform))
        {
            display.remove(&right);
            display.remove(&left);
            display.insert(view.into(), transform.clone());
            recovered = true;
        }
    }
    if recovered {
        migrate(value, 1, target, relative);
        if let Some(reference) = profile(value, relative)
            && matches!(
                value.get("parent").and_then(Value::as_str),
                Some("item/generated" | "minecraft:item/generated")
            )
        {
            value["parent"] = json!(reference.parent);
        }
    }
    Ok(recovered)
}
