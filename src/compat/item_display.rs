use anyhow::Result;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

#[derive(Clone, Debug, Deserialize)]
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
    let display = value.get("display")?;
    let first = display
        .get("firstperson")
        .and_then(|value| serde_json::from_value::<Transform>(value.clone()).ok());
    let third = display
        .get("thirdperson")
        .and_then(|value| serde_json::from_value::<Transform>(value.clone()).ok());
    PROFILES.profiles.values().find(|profile| {
        third.as_ref().is_some_and(|transform| {
            transform.rotation == profile.legacy["thirdperson"].rotation
                && transform.translation == profile.legacy["thirdperson"].translation
        }) && first
            .as_ref()
            .is_none_or(|transform| transform.rotation == profile.legacy["firstperson"].rotation)
    })
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
                if let Some(transform) = display.get(&key).cloned() {
                    let converted = reference.and_then(|profile| {
                        convert_transform(
                            &transform,
                            &profile.modern[&key],
                            &profile.legacy[view],
                            false,
                        )
                    });
                    display
                        .entry(view)
                        .or_insert_with(|| converted.unwrap_or(transform));
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
