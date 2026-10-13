use super::assets::AssetReport;
use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

#[derive(Deserialize)]
struct Bindings {
    models: BTreeMap<String, Value>,
}

static BINDINGS: LazyLock<Bindings> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../assets/item-bindings.json"))
        .expect("物品模型绑定参考表无效")
});

pub(super) fn reference(name: &str) -> Option<&'static Value> {
    BINDINGS.models.get(name)
}

fn provided_texture(root: &Path, model: &Value) -> bool {
    model
        .get("textures")
        .and_then(Value::as_object)
        .is_some_and(|textures| {
            textures.values().any(|value| {
                value.as_str().is_some_and(|name| {
                    let name = name.strip_prefix("minecraft:").unwrap_or(name);
                    name.starts_with("item/")
                        && root
                            .join(format!("assets/minecraft/textures/{name}.png"))
                            .is_file()
                })
            })
        })
}

pub(super) fn migrate(
    root: &Path,
    source: u32,
    target: u32,
    report: &mut AssetReport,
) -> Result<()> {
    if !(4..46).contains(&target) || source >= 4 && !root.join("fastpack/original-assets").is_dir()
    {
        return Ok(());
    }
    let directory = root.join("assets/minecraft/models/item");
    let mut count = 0;
    for (name, reference) in &BINDINGS.models {
        if name.contains("_trim") || !provided_texture(root, reference) {
            continue;
        }
        let path = directory.join(format!("{name}.json"));
        if path.exists() {
            continue;
        }
        let mut model = reference.clone();
        if target < 15
            && let Some(overrides) = model.get_mut("overrides").and_then(Value::as_array_mut)
        {
            overrides.retain(|entry| {
                entry
                    .get("predicate")
                    .and_then(|predicate| predicate.get("trim_type"))
                    .is_none()
            });
            if overrides.is_empty() {
                model.as_object_mut().unwrap().remove("overrides");
            }
        }
        fs::create_dir_all(&directory)?;
        fs::write(&path, serde_json::to_vec_pretty(&model)?)?;
        count += 1;
        if target >= 15
            && let Some(overrides) = model.get("overrides").and_then(Value::as_array)
        {
            for entry in overrides {
                if entry
                    .get("predicate")
                    .and_then(|predicate| predicate.get("trim_type"))
                    .is_none()
                {
                    continue;
                }
                let Some(name) = entry.get("model").and_then(Value::as_str) else {
                    continue;
                };
                let name = name
                    .strip_prefix("minecraft:item/")
                    .or_else(|| name.strip_prefix("item/"))
                    .unwrap_or(name);
                let Some(variant) = BINDINGS.models.get(name) else {
                    continue;
                };
                let path = directory.join(format!("{name}.json"));
                if !path.exists() {
                    fs::write(path, serde_json::to_vec_pretty(variant)?)?;
                    count += 1;
                }
            }
        }
    }
    if count > 0 {
        report.generated.push(format!(
            "物品模型：为上传的贴图补齐 {count} 个明确绑定与盔甲纹饰变体，保留已有自定义模型"
        ));
    }
    Ok(())
}
