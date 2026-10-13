use super::assets::{AssetReport, backup};
use anyhow::Result;
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

mod audio {
    include!("../../assets/via-items/audio.rs");
}

static EVENTS: LazyLock<Map<String, Value>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../assets/via-items/sounds.json"))
        .expect("协议物品音效资源表无效")
});

fn read(path: &Path, report: &mut AssetReport) -> Result<Option<Map<String, Value>>> {
    if !path.is_file() {
        return Ok(Some(Map::new()));
    }
    match serde_json::from_slice(&fs::read(path)?) {
        Ok(value) => Ok(Some(value)),
        Err(_) => {
            report.warnings.push(format!(
                "音效定义 {} 不是有效的 JSON 对象，已保留原文件并跳过该命名空间的合并",
                path.display()
            ));
            Ok(None)
        }
    }
}

fn write(root: &Path, path: &Path, value: &Value) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    if path.is_file() {
        if fs::read(path)? == bytes {
            return Ok(());
        }
        backup(root, path)?;
    }
    fs::create_dir_all(path.parent().unwrap())?;
    let temporary = path.with_extension("json.fastpack-new");
    fs::write(&temporary, bytes)?;
    if path.is_file() {
        fs::remove_file(path)?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

fn files(value: &Value) -> BTreeSet<String> {
    value["sounds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|sound| sound.get("type").and_then(Value::as_str) != Some("event"))
        .filter_map(|sound| sound.as_str().or_else(|| sound["name"].as_str()))
        .filter_map(|name| {
            let (namespace, name) = name.split_once(':').unwrap_or(("minecraft", name));
            (namespace == "minecraft").then(|| format!("{name}.ogg"))
        })
        .collect()
}

pub(super) fn migrate(
    root: &Path,
    target: u32,
    version: &str,
    report: &mut AssetReport,
) -> Result<()> {
    if target >= 34 || !root.join("fastpack/via-bridge/manifest.json").is_file() {
        return Ok(());
    }
    let path = root.join("assets/minecraft/sounds.json");
    let Some(mut sounds) = read(&path, report)? else {
        return Ok(());
    };
    let alias_path = root.join("assets/fastpack/sounds.json");
    let mut aliases = read(&alias_path, report)?;
    let mut added = 0;
    let mut installed = 0;
    let mut events = Vec::new();
    for (name, definition) in EVENTS.iter() {
        if !sounds.contains_key(name) {
            let mut definition = definition.clone();
            definition["replace"] = json!(true);
            sounds.insert(name.clone(), definition);
            added += 1;
        }
        for file in files(&sounds[name]) {
            let Some(bytes) = audio::get(&file) else {
                continue;
            };
            let destination = root.join("assets/minecraft/sounds").join(&file);
            if destination.is_file() || file == "random/bow.ogg" {
                continue;
            }
            fs::create_dir_all(destination.parent().unwrap())?;
            fs::write(destination, bytes)?;
            installed += 1;
        }
        let alias =
            json!({"replace":true,"sounds":[{"name":format!("minecraft:{name}"),"type":"event"}]});
        let enabled = if let Some(aliases) = aliases.as_mut() {
            let entry = aliases.entry(name.clone()).or_insert_with(|| alias.clone());
            if *entry == alias {
                true
            } else {
                report.warnings.push(format!(
                    "音效入口 fastpack:{name} 已由原包定义，已保留原规则"
                ));
                false
            }
        } else {
            false
        };
        events.push(json!({
            "event":format!("minecraft:{name}"),
            "bridge_event":enabled.then(||format!("fastpack:{name}")),
            "source":"resource pack override or official fallback"
        }));
    }
    let languages: Map<String, Value> =
        serde_json::from_str(include_str!("../../assets/via-items/audio-languages.json"))?;
    for (language, entries) in languages {
        let path = root.join(format!("assets/minecraft/lang/{language}.json"));
        let Some(mut values) = read(&path, report)? else {
            continue;
        };
        for (key, value) in entries.as_object().unwrap() {
            values.entry(key.clone()).or_insert_with(|| value.clone());
        }
        write(root, &path, &Value::Object(values))?;
    }
    write(root, &path, &Value::Object(sounds))?;
    if let Some(aliases) = aliases {
        write(root, &alias_path, &Value::Object(aliases))?;
    }
    write(
        root,
        &root.join("fastpack/via-bridge/audio.json"),
        &json!({
            "schema":1,
            "client":version,
            "events":events,
            "requires":"The server must forward the original sound event or send the fastpack namespaced bridge event. Sound packets already replaced with shared legacy events or cancelled cannot be recovered by a resource pack.",
            "legacy_fallbacks":{
                "item.mace.smash_air":"entity.player.attack.knockback",
                "item.mace.smash_ground":"entity.player.attack.strong",
                "item.mace.smash_ground_heavy":"entity.player.attack.crit",
                "entity.wind_charge.throw":"entity.arrow.shoot",
                "entity.wind_charge.wind_burst":"entity.generic.wind_burst",
                "entity.breeze.wind_burst":"entity.generic.wind_burst"
            },
            "shared_events_replaced":false,
            "equipment_animation":"Controlled by the client renderer; 1.20.1 resource pack models cannot disable it."
        }),
    )?;
    report.generated.push(format!(
        "协议物品音效：为 {version} 补齐 {added} 个原声事件、{installed} 个 Ogg 文件及 fastpack 原声入口，保留原包音效；实际播放需要服务器传递原声事件，资源包无法恢复已替换或丢弃的声音，也无法关闭客户端抬手动画"
    ));
    Ok(())
}
