use super::atlas::{self, AtlasLayout};
use super::properties::{self, Properties};
use super::{CompatibilityReport, SkyRuntime};
use anyhow::{Context, Result, bail, ensure};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use walkdir::WalkDir;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Layer {
    pub name: String,
    pub world: String,
    pub texture: String,
    pub layout: String,
    pub properties: Properties,
    pub original: String,
    pub decoration: bool,
}

#[derive(Clone, Debug)]
pub struct Preview {
    pub names: Vec<String>,
    pub image: RgbaImage,
}

fn layout(name: &str) -> AtlasLayout {
    match name {
        "fsb" => AtlasLayout::FabricSkyboxes,
        "nuit" => AtlasLayout::Nuit,
        _ => AtlasLayout::Optifine,
    }
}
fn local(root: &Path, value: &str) -> Result<PathBuf> {
    let value = value.replace('\\', "/");
    let path = Path::new(&value);
    ensure!(
        !path.is_absolute()
            && !value.contains(':')
            && !path.components().any(|component| matches!(
                component,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )),
        "资源路径越界: {value}"
    );
    Ok(root.join(path))
}

fn resolve(root: &Path, property: &Path, source: Option<&str>, base: &str) -> Result<PathBuf> {
    let relative = property
        .strip_prefix(root)?
        .to_string_lossy()
        .replace('\\', "/");
    let namespace = relative.split('/').nth(1).unwrap_or("minecraft");
    let value = source.map(str::trim).filter(|value| !value.is_empty());
    let candidate = match value {
        None => property.with_extension("png"),
        Some(value) if value.starts_with("assets/") => local(root, value)?,
        Some(value) if value.starts_with("./") => local(
            root,
            &format!(
                "{}/{}",
                property.parent().unwrap().strip_prefix(root)?.display(),
                &value[2..]
            ),
        )?,
        Some(value) if value.starts_with("~/") => {
            local(root, &format!("assets/{namespace}/{base}/{}", &value[2..]))?
        }
        Some(value) if value.starts_with('/') => local(
            root,
            &format!("assets/{namespace}/{}", value.trim_start_matches('/')),
        )?,
        Some(value) if value.contains(':') => {
            let (namespace, path) = value.split_once(':').unwrap();
            ensure!(
                !namespace.is_empty()
                    && namespace
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c)),
                "资源命名空间无效"
            );
            local(root, &format!("assets/{namespace}/{path}"))?
        }
        Some(value) => {
            let candidate = local(root, &format!("assets/{namespace}/{value}"))?;
            if candidate.exists() {
                candidate
            } else {
                local(
                    root,
                    &format!(
                        "{}/{}",
                        property.parent().unwrap().strip_prefix(root)?.display(),
                        value
                    ),
                )?
            }
        }
    };
    ensure!(
        candidate.is_file(),
        "天空纹理不存在: {}",
        candidate.display()
    );
    Ok(candidate)
}

fn ticks(value: &str) -> Result<i32> {
    let Some((hour, minute)) = value.split_once(':') else {
        bail!("天空时间无效: {value}");
    };
    let hour = hour.parse::<i32>()?;
    let minute = minute.parse::<i32>()?;
    ensure!(
        (0..24).contains(&hour) && (0..60).contains(&minute),
        "天空时间超出范围: {value}"
    );
    Ok(((hour * 1000 + minute * 1000 / 60) - 6000).rem_euclid(24000))
}

fn clock(tick: i32) -> String {
    let minutes = ((tick.rem_euclid(24000) + 6000) % 24000) * 60 / 1000;
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

fn fade(props: &Properties) -> Result<[i32; 4]> {
    let start = ticks(props.get("startFadeIn").context("缺少 startFadeIn")?)?;
    let middle = ticks(props.get("endFadeIn").context("缺少 endFadeIn")?)?;
    let end = ticks(props.get("endFadeOut").context("缺少 endFadeOut")?)?;
    let stop = props
        .get("startFadeOut")
        .map(|value| ticks(value))
        .transpose()?
        .unwrap_or_else(|| (end - (middle - start).rem_euclid(24000)).rem_euclid(24000));
    ensure!(
        [start, middle, stop, end]
            .windows(2)
            .all(|pair| (pair[1] - pair[0]).rem_euclid(24000) > 0),
        "天空淡入淡出时间重合"
    );
    Ok([start, middle, stop, end])
}

fn json_props(value: &Value, nuit: bool) -> Result<Properties> {
    let mut props = Properties::new();
    let p = value.get("properties").unwrap_or(&Value::Null);
    let rotation = p.get("rotation").unwrap_or(&Value::Null);
    let speeds = if nuit {
        vec![rotation.get("speed").and_then(Value::as_f64).unwrap_or(0.0)]
    } else {
        ["rotationSpeedX", "rotationSpeedY", "rotationSpeedZ"]
            .iter()
            .map(|key| rotation.get(key).and_then(Value::as_f64).unwrap_or(0.0))
            .collect()
    };
    ensure!(
        speeds.iter().all(|value| *value == 0.0),
        "该原生天空包含动态旋转，不能确定旧版旋转等价关系"
    );
    let static_mapping = rotation.get(if nuit { "mapping" } else { "static" });
    ensure!(
        static_mapping.is_none_or(|mapping| mapping
            .as_array()
            .is_some_and(|values| values.iter().all(|value| value.as_f64() == Some(0.0)))),
        "该原生天空包含额外旋转关键帧"
    );
    props.insert("rotate".into(), "false".into());
    let blend = if nuit {
        p.get("blend").and_then(Value::as_str)
    } else {
        value
            .get("blend")
            .and_then(|value| value.get("type"))
            .and_then(Value::as_str)
    }
    .unwrap_or("alpha");
    ensure!(
        blend != "custom",
        "自定义混合因子没有可证明的 OptiFine 等价模式"
    );
    props.insert("blend".into(), blend.into());
    let fade = p.get("fade").unwrap_or(&Value::Null);
    if nuit {
        if let Some(keys) = fade
            .get("keyFrames")
            .and_then(Value::as_object)
            .filter(|keys| !keys.is_empty())
        {
            ensure!(keys.len() == 4, "只可无损转回四关键帧的天空淡入淡出");
            let mut pairs = keys
                .iter()
                .map(|(tick, alpha)| {
                    Ok((
                        tick.parse::<i32>()?,
                        alpha.as_f64().context("淡入淡出数值无效")?,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            pairs.sort_by_key(|(tick, _)| *tick);
            let start = pairs
                .iter()
                .enumerate()
                .find(|(i, (_, alpha))| *alpha == 0.0 && pairs[(i + 1) % 4].1 == 1.0)
                .map(|(index, _)| index)
                .context("天空关键帧不是 0→1→1→0")?;
            for (offset, key) in ["startFadeIn", "endFadeIn", "startFadeOut", "endFadeOut"]
                .into_iter()
                .enumerate()
            {
                props.insert(key.into(), clock(pairs[(start + offset) % 4].0));
            }
        }
    } else if fade.get("alwaysOn").and_then(Value::as_bool) != Some(true) {
        for key in ["startFadeIn", "endFadeIn", "startFadeOut", "endFadeOut"] {
            if let Some(value) = fade.get(key).and_then(Value::as_i64) {
                props.insert(key.into(), clock(value as i32));
            }
        }
    }
    let conditions = value.get("conditions").unwrap_or(&Value::Null);
    for (field, key) in [("weather", "weather"), ("biomes", "biomes")] {
        if let Some(value) = conditions.get(field) {
            ensure!(
                value.get("excludes").and_then(Value::as_bool) != Some(true),
                "排除条件无法在旧天空格式中无损表达"
            );
            let value = if nuit {
                value.get("entries").unwrap_or(value)
            } else {
                value
            };
            if let Some(array) = value.as_array().filter(|values| !values.is_empty()) {
                props.insert(
                    key.into(),
                    array
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" "),
                );
            }
        }
    }
    if let Some(array) = conditions
        .get("yRanges")
        .and_then(|value| {
            if nuit {
                value.get("entries")
            } else {
                Some(value)
            }
        })
        .and_then(Value::as_array)
        .filter(|values| !values.is_empty())
    {
        props.insert(
            "heights".into(),
            array
                .iter()
                .map(|value| {
                    format!(
                        "({})-({})",
                        value.get("min").and_then(Value::as_i64).unwrap_or(0),
                        value.get("max").and_then(Value::as_i64).unwrap_or(319)
                    )
                })
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    ensure!(
        conditions.get("xRanges").is_none()
            && conditions.get("zRanges").is_none()
            && conditions.get("effects").is_none(),
        "位置或状态效果条件没有旧格式等价表达"
    );
    Ok(props)
}

fn read_layers(root: &Path) -> Result<(Vec<Layer>, Vec<String>)> {
    let cache = root.join("fastpack/canonical-sky/layers.json");
    if cache.is_file() {
        let layers: Vec<Layer> = serde_json::from_slice(&fs::read(cache)?)?;
        for layer in &layers {
            ensure!(
                layer
                    .world
                    .strip_prefix("world")
                    .is_some_and(|suffix| suffix.parse::<i32>().is_ok()),
                "天空维度缓存无效"
            );
            ensure!(
                !layer.decoration || matches!(layer.name.as_str(), "sun" | "moon_phases"),
                "天空装饰缓存名称无效"
            );
            local(root, &layer.texture)?;
            local(root, &layer.original)?;
        }
        let notes = root.join("fastpack/canonical-sky/warnings.json");
        let warnings = if notes.exists() {
            serde_json::from_slice(&fs::read(notes)?)?
        } else {
            Vec::new()
        };
        return Ok((layers, warnings));
    }
    let mut candidates = Vec::new();
    for entry in WalkDir::new(root).min_depth(1) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry
            .path()
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        if path.starts_with("fastpack/") {
            continue;
        }
        if path.ends_with(".properties")
            && (path.contains("/mcpatcher/sky/") || path.contains("/optifine/sky/"))
            || path.ends_with(".json") && path.contains("/sky/") && path.starts_with("assets/")
        {
            candidates.push(entry.into_path());
        }
    }
    candidates.sort();
    let mut layers = Vec::new();
    let mut warnings = Vec::new();
    let mut seen = BTreeSet::new();
    for path in candidates {
        let original = path
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        let result = (|| -> Result<Layer> {
            let name = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            if path.extension().is_some_and(|ext| ext == "properties") {
                let properties = properties::parse(&fs::read(&path)?)?;
                let base = if original.contains("/mcpatcher/") {
                    "mcpatcher"
                } else {
                    "optifine"
                };
                let world = path
                    .parent()
                    .and_then(Path::file_name)
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let texture = resolve(
                    root,
                    &path,
                    properties.get("source").map(String::as_str),
                    base,
                )?;
                Ok(Layer {
                    name: name.clone(),
                    world,
                    texture: texture
                        .strip_prefix(root)?
                        .to_string_lossy()
                        .replace('\\', "/"),
                    layout: "optifine".into(),
                    properties,
                    original: original.clone(),
                    decoration: matches!(name.as_str(), "sun" | "moon_phases"),
                })
            } else {
                let value: Value = serde_json::from_slice(&fs::read(&path)?)?;
                let nuit = original.starts_with("assets/nuit/");
                let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
                ensure!(
                    kind == if nuit {
                        "square-textured"
                    } else {
                        "single-sprite-square-textured"
                    },
                    "天空类型 {kind} 暂无无损自动变换"
                );
                let texture = value
                    .get("texture")
                    .and_then(Value::as_str)
                    .context("天空 JSON 缺少纹理")?;
                let texture = resolve(root, &path, Some(texture), "")?;
                let properties = json_props(&value, nuit)?;
                let worlds = value
                    .get("conditions")
                    .and_then(|value| {
                        value
                            .get("dimensions")
                            .or_else(|| value.get(if nuit { "skyboxes" } else { "worlds" }))
                    })
                    .and_then(|value| {
                        if nuit {
                            value.get("entries")
                        } else {
                            Some(value)
                        }
                    })
                    .and_then(Value::as_array);
                let world = match worlds
                    .and_then(|array| array.first())
                    .and_then(Value::as_str)
                    .unwrap_or("minecraft:overworld")
                {
                    "minecraft:overworld" => "world0",
                    "minecraft:the_end" | "minecraft:end" => "world1",
                    other => bail!("天空维度 {other} 不适用于旧天空格式"),
                };
                ensure!(
                    worlds.is_none_or(|array| array.len() <= 1),
                    "多维度天空需要独立版本变体"
                );
                Ok(Layer {
                    name,
                    world: world.into(),
                    texture: texture
                        .strip_prefix(root)?
                        .to_string_lossy()
                        .replace('\\', "/"),
                    layout: if nuit { "nuit" } else { "fsb" }.into(),
                    properties,
                    original: original.clone(),
                    decoration: false,
                })
            }
        })();
        match result {
            Ok(layer) => {
                let key = format!("{}/{}", layer.world, layer.name);
                if seen.insert(key) {
                    layers.push(layer);
                }
            }
            Err(error) => warnings.push(format!("{original}: {error:#}")),
        }
    }
    Ok((layers, warnings))
}

pub fn prepare(root: &Path) -> Result<Vec<String>> {
    if root.join("fastpack/canonical-sky/layers.json").exists() {
        return Ok(Vec::new());
    }
    let (mut layers, warnings) = read_layers(root)?;
    if layers.is_empty() {
        return Ok(warnings);
    }
    let directory = root.join("fastpack/canonical-sky");
    fs::create_dir_all(&directory)?;
    for (index, layer) in layers.iter_mut().enumerate() {
        let source = local(root, &layer.texture)?;
        let extension = source
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        let name = format!("{index:04}.{extension}");
        fs::copy(&source, directory.join(&name))?;
        layer.texture = format!("fastpack/canonical-sky/{name}");
    }
    fs::write(
        directory.join("layers.json"),
        serde_json::to_vec_pretty(&layers)?,
    )?;
    if !warnings.is_empty() {
        fs::write(
            directory.join("warnings.json"),
            serde_json::to_vec_pretty(&warnings)?,
        )?;
    }
    Ok(warnings)
}

fn ranges(value: &str) -> Result<Vec<Value>> {
    let mut result = Vec::new();
    for token in value.split_whitespace() {
        let normalized = token.replace(['(', ')'], "");
        let sep = normalized
            .char_indices()
            .skip(1)
            .find(|(_, c)| *c == '-')
            .map(|(index, _)| index);
        let (min, max) = if let Some(index) = sep {
            (
                normalized[..index].parse::<i32>()?,
                normalized[index + 1..].parse::<i32>()?,
            )
        } else {
            let v = normalized.parse::<i32>()?;
            (v, v)
        };
        ensure!(min <= max, "范围逆序: {token}");
        result.push(json!({"min":min,"max":max}));
    }
    Ok(result)
}

fn native(layer: &Layer, nuit: bool, texture: &str, priority: i32) -> Result<Value> {
    let p = &layer.properties;
    let mut properties = json!({"sunSkyTint":false,if nuit{"layer"}else{"priority"}:priority});
    let has_fade = ["startFadeIn", "endFadeIn", "endFadeOut"]
        .iter()
        .all(|key| p.contains_key(*key));
    let fade = if has_fade { Some(fade(p)?) } else { None };
    if let Some([a, b, c, d]) = fade {
        properties["fade"] = if nuit {
            json!({"duration":24000,"keyFrames":{a.to_string():0.0,b.to_string():1.0,c.to_string():1.0,d.to_string():0.0}})
        } else {
            json!({"startFadeIn":a,"endFadeIn":b,"startFadeOut":c,"endFadeOut":d,"alwaysOn":false})
        };
    } else if !nuit {
        properties["fade"] = json!({"alwaysOn":true});
    }
    let transition = p
        .get("transition")
        .map(|value| value.parse::<f64>())
        .transpose()?
        .unwrap_or(1.0);
    ensure!(transition.is_finite() && transition >= 0.0, "天空过渡无效");
    properties["transitionInDuration"] = json!((transition * 20.0).round().max(1.0) as u32);
    properties["transitionOutDuration"] = properties["transitionInDuration"].clone();
    let rotate = p.get("rotate").map(String::as_str).unwrap_or("true") != "false";
    let speed = p
        .get("speed")
        .map(|value| value.parse::<f64>())
        .transpose()?
        .unwrap_or(1.0);
    ensure!(speed.is_finite(), "天空旋转速度无效");
    let axis = p.get("axis").map(String::as_str).unwrap_or("0 0 1");
    let axis: Vec<_> = axis
        .split_whitespace()
        .map(|number| number.parse::<f64>())
        .collect::<std::result::Result<_, _>>()?;
    ensure!(
        axis.len() == 3 && axis.iter().all(|number| number.is_finite()),
        "天空旋转轴无效"
    );
    ensure!(
        !rotate || speed == 0.0,
        "动态旋转以原生天空 Interop 保留精确行为"
    );
    let active = rotate && speed != 0.0;
    properties["rotation"] = if nuit {
        json!({"skyboxRotation":false,"speed":if active{-1.0}else{0.0},"duration":24000,"axis":{"0":[90.0,0.0,0.0]}})
    } else {
        json!({"skyboxRotation":false,"rotationSpeedZ":if active{-1.0}else{0.0},"axis":[0.0,0.0,0.0]})
    };
    let blend = p.get("blend").map(String::as_str).unwrap_or("add");
    ensure!(
        [
            "add", "subtract", "multiply", "screen", "replace", "alpha", "burn", "dodge"
        ]
        .contains(&blend),
        "目标加载器没有混合模式 {blend} 的可靠等价实现"
    );
    if nuit {
        ensure!(
            blend != "replace",
            "Nuit 的 replace 混合行为与旧版替换模式不一致，保留原配置给 Interop"
        );
        properties["blend"] = json!(blend);
    }
    let mut conditions = json!({});
    let dimension = if layer.world == "world0" {
        "minecraft:overworld"
    } else if layer.world == "world1" {
        "minecraft:the_end"
    } else {
        bail!("非标准天空维度 {}", layer.world);
    };
    let wrap = |values: Value| {
        if nuit {
            json!({"entries":values})
        } else {
            values
        }
    };
    conditions["dimensions"] = wrap(json!([dimension]));
    let mut weather = p
        .get("weather")
        .map(|value| {
            value
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec!["clear".into()]);
    if weather.iter().any(|value| value == "clear") {
        weather.push("snow".into());
    }
    conditions["weather"] = wrap(json!(weather));
    if let Some(value) = p.get("biomes") {
        let names = value
            .split_whitespace()
            .map(|name| {
                if name.contains(':') {
                    name.to_string()
                } else {
                    format!("minecraft:{name}")
                }
            })
            .collect::<Vec<_>>();
        ensure!(
            names.iter().all(|name| !name
                .rsplit(':')
                .next()
                .unwrap()
                .chars()
                .all(|c| c.is_ascii_digit())),
            "旧版数字群系 ID 需要注册表映射"
        );
        conditions["biomes"] = wrap(json!(names));
    }
    if let Some(value) = p.get("heights") {
        conditions["yRanges"] = wrap(json!(ranges(value)?));
    }
    if let Some(days) = p.get("days") {
        ensure!(
            !nuit,
            "Nuit schema 1 没有同等 daysLoop 条件，保留原配置给 Interop"
        );
        conditions["loop"] = json!({"days":p.get("daysLoop").and_then(|value|value.parse::<u32>().ok()).unwrap_or(8),"ranges":ranges(days)?});
    }
    let mut result = json!({"schemaVersion":if nuit{1}else{2},"type":if nuit{"square-textured"}else{"single-sprite-square-textured"},"texture":texture,"properties":properties,"conditions":conditions});
    if !nuit {
        result["blend"] = json!({"type":if blend=="replace"{"disable"}else{blend}});
        result["decorations"] = json!({"showSun":false,"showMoon":false,"showStars":false});
    }
    Ok(result)
}

fn restore_optifine(
    root: &Path,
    layers: &[Layer],
    format: u32,
    report: &mut CompatibilityReport,
) -> Result<()> {
    let base = if format < 4 { "mcpatcher" } else { "optifine" };
    let mut indices = BTreeMap::<String, u32>::new();
    for layer in layers {
        let index = indices.entry(layer.world.clone()).or_insert(1);
        let folder = root.join(format!("assets/minecraft/{base}/sky/{}", layer.world));
        fs::create_dir_all(&folder)?;
        let name = if layer.decoration {
            layer.name.clone()
        } else {
            while folder.join(format!("sky{index}.properties")).exists() {
                *index += 1;
            }
            let name = format!("sky{index}");
            *index += 1;
            name
        };
        let target = folder.join(format!("{name}.png"));
        if layer.decoration {
            fs::copy(local(root, &layer.texture)?, &target)?;
        } else {
            let image = image::open(local(root, &layer.texture)?)?.to_rgba8();
            let image = atlas::remap(
                &image,
                layout(&layer.layout),
                AtlasLayout::Optifine,
                layer.world == "world0",
            )?;
            image.save(&target)?;
        }
        let mut props = layer.properties.clone();
        props.insert("source".into(), format!("./{name}.png"));
        fs::write(
            folder.join(format!("{name}.properties")),
            properties::write(&props),
        )?;
        report.sky_converted.push(format!(
            "{} → {base}/sky/{}/{name}.properties（纹理引用已重写）",
            layer.original, layer.world
        ));
    }
    Ok(())
}

pub fn migrate(
    root: &Path,
    format: u32,
    runtime: SkyRuntime,
    report: &mut CompatibilityReport,
) -> Result<()> {
    let (layers, warnings) = read_layers(root)?;
    report.warnings.extend(warnings);
    report.sky_layers = layers.len();
    if layers.is_empty() {
        return Ok(());
    }
    let target = if runtime == SkyRuntime::Auto {
        if format >= 84 {
            SkyRuntime::Nuit
        } else if format >= 15 && layers.iter().all(|layer| layer.layout == "fsb") {
            SkyRuntime::FabricSkyboxes
        } else {
            SkyRuntime::Optifine
        }
    } else {
        runtime.resolved(format)
    };
    let active_path = root.join("fastpack/canonical-sky/active.json");
    let previous: Vec<String> = if active_path.exists() {
        serde_json::from_slice(&fs::read(&active_path)?)?
    } else {
        layers.iter().map(|layer| layer.original.clone()).collect()
    };
    for relative in previous {
        let path = local(root, &relative)?;
        if path.is_file() {
            let backup = root.join("fastpack/original-sky").join(&relative);
            fs::create_dir_all(backup.parent().unwrap())?;
            if !backup.exists() {
                fs::copy(&path, &backup)?;
            }
            fs::remove_file(path)?;
        }
    }
    let untouched: BTreeSet<_> = read_active_layers(root)?.into_iter().collect();
    if matches!(
        target,
        SkyRuntime::Optifine | SkyRuntime::Interop | SkyRuntime::Vanilla
    ) {
        restore_optifine(root, &layers, format, report)?;
    } else {
        let nuit = target == SkyRuntime::Nuit;
        ensure!(
            if nuit { format >= 84 } else { format >= 6 },
            "所选天空加载器不支持该游戏代际"
        );
        let mut fallback = Vec::new();
        for (index, layer) in layers.iter().enumerate() {
            let result = (|| -> Result<()> {
                ensure!(!layer.decoration, "太阳/月亮混合规则保留到原配置");
                let identifier = format!("fastpack:textures/sky/layer_{index}.png");
                let definition = native(layer, nuit, &identifier, index as i32)?;
                let texture = root.join(format!("assets/fastpack/textures/sky/layer_{index}.png"));
                fs::create_dir_all(texture.parent().unwrap())?;
                let image = image::open(local(root, &layer.texture)?)?.to_rgba8();
                let converted = atlas::remap(
                    &image,
                    layout(&layer.layout),
                    if nuit {
                        AtlasLayout::Nuit
                    } else {
                        AtlasLayout::FabricSkyboxes
                    },
                    layer.world == "world0",
                )?;
                converted.save(texture)?;
                let file = root.join(format!(
                    "assets/{}/sky/fastpack_layer_{index}.json",
                    if nuit { "nuit" } else { "fastpack" }
                ));
                fs::create_dir_all(file.parent().unwrap())?;
                fs::write(file, serde_json::to_vec_pretty(&definition)?)?;
                report.sky_converted.push(format!(
                    "{} → {} schema {}（六面朝向与淡入淡出已转换）",
                    layer.original,
                    target,
                    if nuit { 1 } else { 2 }
                ));
                Ok(())
            })();
            if let Err(error) = result {
                report.warnings.push(format!(
                    "{}：{error:#}；保留原天空供 Interop 自动读取",
                    layer.original
                ));
                fallback.push(layer.clone());
            }
        }
        if !fallback.is_empty() {
            restore_optifine(root, &fallback, format, report)?;
            report
                .requirements
                .push(SkyRuntime::Interop.requirement(format));
        }
    }
    report.requirements.push(target.requirement(format));
    report.requirements.push(
        "自定义天空的显示取决于客户端已有的渲染能力，资源包转换不能给原版客户端新增该能力".into(),
    );
    let active: Vec<_> = read_active_layers(root)?
        .into_iter()
        .filter(|path| !untouched.contains(path))
        .collect();
    fs::create_dir_all(active_path.parent().unwrap())?;
    fs::write(active_path, serde_json::to_vec_pretty(&active)?)?;
    Ok(())
}

fn read_active_layers(root: &Path) -> Result<Vec<String>> {
    let mut active = Vec::new();
    for entry in WalkDir::new(root.join("assets")).min_depth(1) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        if relative.ends_with(".properties")
            && (relative.contains("/optifine/sky/") || relative.contains("/mcpatcher/sky/"))
            || relative.ends_with(".json")
                && entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("fastpack_layer_")
        {
            active.push(relative);
        }
    }
    Ok(active)
}

pub fn preview(source: &Path) -> Result<Option<Preview>> {
    let (_work, root) = crate::converter::load_pack(source)?;
    let (layers, _) = read_layers(&root)?;
    let Some(layer) = layers.iter().find(|layer| !layer.decoration) else {
        return Ok(None);
    };
    let image = image::open(local(&root, &layer.texture)?)?.to_rgba8();
    let image = atlas::remap(
        &image,
        layout(&layer.layout),
        AtlasLayout::Optifine,
        layer.world == "world0",
    )?;
    Ok(Some(Preview {
        names: layers.iter().map(|layer| layer.name.clone()).collect(),
        image,
    }))
}
