use super::assets::{AssetReport, backup};
use super::properties::{self, Properties};
use crate::packfs::{self as fs, PathExt, WalkDir};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

const PREFIX: &str = "item/fastpack_cit/";

#[path = "cit_legacy.rs"]
mod legacy;
#[path = "cit_native_rules.rs"]
mod native;

pub(super) fn id(value: &str) -> Result<(String, String)> {
    let (namespace, name) = value.split_once(':').unwrap_or(("minecraft", value));
    ensure!(
        !namespace.is_empty()
            && !name.is_empty()
            && namespace
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"._-".contains(&c))
            && name
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"._-/".contains(&c))
            && !name.split('/').any(|part| matches!(part, "" | "." | "..")),
        "资源标识无效：{value}"
    );
    Ok((namespace.into(), name.into()))
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))[..24].into()
}

fn key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_lowercase()
}

fn lexical(root: &Path, value: &Path) -> Option<PathBuf> {
    let mut path = PathBuf::new();
    for part in value.strip_prefix(root).ok()?.components() {
        match part {
            Component::Normal(part) => path.push(part),
            Component::CurDir => {}
            Component::ParentDir if path.pop() => {}
            _ => return None,
        }
    }
    let path = root.join(path);
    path.starts_with(root.join("assets")).then_some(path)
}

struct Resources<'a> {
    root: &'a Path,
    target: u32,
    index: BTreeMap<String, PathBuf>,
    outputs: BTreeMap<PathBuf, Vec<u8>>,
    textures: BTreeMap<PathBuf, String>,
    models: BTreeMap<PathBuf, String>,
    visiting: BTreeSet<PathBuf>,
}

impl<'a> Resources<'a> {
    fn new(root: &'a Path, target: u32) -> Result<Self> {
        let mut index = BTreeMap::new();
        for entry in WalkDir::new(root.join("assets")).min_depth(1) {
            let entry = entry?;
            if entry.file_type().is_file() {
                index.insert(key(entry.path()), entry.into_path());
            }
        }
        Ok(Self {
            root,
            target,
            index,
            outputs: BTreeMap::new(),
            textures: BTreeMap::new(),
            models: BTreeMap::new(),
            visiting: BTreeSet::new(),
        })
    }
    fn resolve(&self, source: &Path, value: &str, extension: &str) -> Result<PathBuf> {
        let relative = source.strip_prefix(self.root.join("assets"))?;
        let namespace = relative
            .components()
            .next()
            .context("资源缺少命名空间")?
            .as_os_str();
        let value = value.replace('\\', "/");
        let value = if value.to_ascii_lowercase().ends_with(extension) {
            value
        } else {
            format!("{value}{extension}")
        };
        let base = self.root.join("assets").join(namespace);
        let kind = if extension == ".json" {
            "models"
        } else {
            "textures"
        };
        let mut candidates = if let Some((namespace, name)) = value.split_once(':') {
            let base = self.root.join("assets").join(namespace);
            vec![base.join(name), base.join(kind).join(name)]
        } else if let Some(value) = value
            .strip_prefix("/assets/")
            .or_else(|| value.strip_prefix("assets/"))
        {
            vec![self.root.join("assets").join(value)]
        } else if let Some(value) = value.strip_prefix("~/") {
            let runtime = relative
                .components()
                .nth(1)
                .context("CIT 路径无效")?
                .as_os_str();
            vec![base.join(runtime).join(value)]
        } else if value.starts_with("./") || value.starts_with("../") {
            vec![source.parent().unwrap().join(&value)]
        } else {
            vec![
                base.join(&value),
                base.join(kind).join(&value),
                source.parent().unwrap().join(&value),
            ]
        };
        for candidate in candidates.clone() {
            let text = candidate.to_string_lossy();
            for (old, new) in [
                ("/textures/items/", "/textures/item/"),
                ("/textures/item/", "/textures/items/"),
                ("/textures/blocks/", "/textures/block/"),
            ] {
                if text.contains(old) {
                    candidates.push(PathBuf::from(text.replace(old, new)));
                }
            }
        }
        for candidate in candidates {
            if let Some(path) =
                lexical(self.root, &candidate).and_then(|path| self.index.get(&key(&path)))
            {
                return Ok(path.clone());
            }
        }
        bail!("找不到 CIT 资源：{value}")
    }
    fn texture(&mut self, source: &Path, value: &str) -> Result<String> {
        let path = self.resolve(source, value, ".png")?;
        if let Some(reference) = self.textures.get(&path) {
            return Ok(reference.clone());
        }
        let relative = path
            .strip_prefix(self.root.join("assets"))?
            .to_string_lossy()
            .replace('\\', "/");
        let (namespace, name) = relative.split_once('/').context("贴图路径无效")?;
        let standard = name
            .strip_prefix("textures/")
            .map(|name| format!("{namespace}:{}", name.trim_end_matches(".png")))
            .filter(|value| id(value).is_ok());
        let reference = if let Some(value) = standard {
            value
        } else {
            let name = format!("{PREFIX}{}", hash(relative.as_bytes()));
            let output = self
                .root
                .join(format!("assets/{namespace}/textures/{name}.png"));
            self.outputs.insert(output.clone(), fs::read(&path)?);
            let metadata = path.with_extension("png.mcmeta");
            if metadata.pack_is_file() {
                self.outputs
                    .insert(output.with_extension("png.mcmeta"), fs::read(metadata)?);
            }
            format!("{namespace}:{name}")
        };
        self.textures.insert(path, reference.clone());
        Ok(reference)
    }
    fn native_model(&self, value: &str) -> Option<String> {
        let (namespace, name) = id(value).ok()?;
        (namespace == "minecraft"
            && (matches!(
                name.as_str(),
                "block/cube_all"
                    | "block/cube"
                    | "block/block"
                    | "block/cube_bottom_top"
                    | "block/orientable"
                    | "block/cross"
                    | "builtin/generated"
                    | "builtin/entity"
                    | "item/generated"
                    | "item/handheld"
                    | "item/handheld_rod"
            ) || super::modern_items::model_reference(
                self.target,
                &format!("minecraft:{name}"),
            )
            .is_some()))
        .then(|| format!("{namespace}:{name}"))
    }
    fn model(&mut self, source: &Path, value: &str) -> Result<String> {
        let path = match self.resolve(source, value, ".json") {
            Ok(path) => path,
            Err(error) => return self.native_model(value).ok_or(error),
        };
        if let Some(reference) = self.models.get(&path) {
            return Ok(reference.clone());
        }
        ensure!(
            self.visiting.len() < 64 && self.visiting.insert(path.clone()),
            "CIT 模型存在循环或继承过深"
        );
        let result = (|| -> Result<String> {
            let mut value = crate::model_json::parse(&fs::read(&path)?)?;
            ensure!(value.is_object(), "CIT 模型不是对象");
            ensure!(
                value.get("loader").is_none(),
                "CIT 模型使用模组加载器，保留给对应客户端处理"
            );
            ensure!(
                value
                    .get("overrides")
                    .and_then(Value::as_array)
                    .is_none_or(Vec::is_empty),
                "CIT 模型含旧式状态 overrides，保留给 CIT 客户端处理"
            );
            if let Some(parent) = value["parent"].as_str() {
                value["parent"] = json!(self.model(&path, parent)?);
            }
            if let Some(textures) = value.get_mut("textures").and_then(Value::as_object_mut) {
                for texture in textures.values_mut() {
                    if let Some(reference) = texture.as_str().filter(|name| !name.starts_with('#'))
                    {
                        *texture = json!(self.texture(&path, reference)?);
                    }
                }
            }
            let relative = path
                .strip_prefix(self.root.join("assets"))?
                .to_string_lossy()
                .replace('\\', "/");
            let (namespace, _) = relative.split_once('/').context("CIT 模型路径无效")?;
            let name = format!("{PREFIX}{}", hash(relative.as_bytes()));
            let reference = format!("{namespace}:{name}");
            self.outputs.insert(
                self.root
                    .join(format!("assets/{namespace}/models/{name}.json")),
                serde_json::to_vec(&value)?,
            );
            self.models.insert(path.clone(), reference.clone());
            Ok(reference)
        })();
        self.visiting.remove(&path);
        result
    }
    fn settings(&self, source: &Path, model: &Value, visiting: &mut BTreeSet<PathBuf>) -> Value {
        let mut result = json!({"display":{}});
        if visiting.len() < 64 {
            if let Some(path) = model["parent"]
                .as_str()
                .and_then(|name| self.resolve(source, name, ".json").ok())
            {
                if visiting.insert(path.clone()) {
                    if let Ok(parent) = fs::read(&path)
                        .map_err(anyhow::Error::from)
                        .and_then(|bytes| crate::model_json::parse(&bytes))
                    {
                        result = self.settings(&path, &parent, visiting);
                    }
                    visiting.remove(&path);
                }
            }
        }
        if let Some(display) = model["display"].as_object() {
            for (key, value) in display {
                result["display"][key] = value.clone();
            }
        }
        for key in ["gui_light", "ambientocclusion"] {
            if let Some(value) = model.get(key) {
                result[key] = value.clone();
            }
        }
        result
    }
    fn appearance(&mut self, source: &Path, properties: &Properties, item: &str) -> Result<Value> {
        if let Some(model) = properties.get("model") {
            return Ok(json!({"type":"minecraft:model","model":self.model(source,model)?}));
        }
        let default = source.file_stem().unwrap().to_string_lossy();
        let texture = self.texture(
            source,
            properties
                .get("texture")
                .map_or(default.as_ref(), String::as_str),
        )?;
        let (namespace, item_name) = id(item)?;
        let handheld = ["_sword", "_pickaxe", "_axe", "_shovel", "_hoe"]
            .iter()
            .any(|suffix| item_name.ends_with(suffix));
        let mut model = json!({"parent":if item_name=="fishing_rod" {"minecraft:item/handheld_rod"}else if handheld {"minecraft:item/handheld"}else{"minecraft:item/generated"},"textures":{"layer0":texture}});
        let base = self
            .root
            .join(format!("assets/{namespace}/models/item/{item_name}.json"));
        let reference = if base.pack_is_file() {
            crate::model_json::parse(&fs::read(&base)?).ok()
        } else {
            super::item_bindings::reference(&item_name).cloned()
        };
        if let Some(reference) = reference {
            let reference = self.settings(&base, &reference, &mut BTreeSet::new());
            for key in ["display", "gui_light", "ambientocclusion"] {
                if let Some(value) = reference.get(key) {
                    model[key] = value.clone();
                }
            }
        }
        let identity = format!(
            "{}:{item}",
            source
                .strip_prefix(self.root)?
                .to_string_lossy()
                .replace('\\', "/")
        );
        let name = format!("{PREFIX}{}", hash(identity.as_bytes()));
        self.outputs.insert(
            self.root
                .join(format!("assets/{namespace}/models/{name}.json")),
            serde_json::to_vec(&model)?,
        );
        Ok(json!({"type":"minecraft:model","model":format!("{namespace}:{name}")}))
    }
    fn reachable(&self, value: &Value, output: &mut BTreeSet<PathBuf>) {
        match value {
            Value::Object(values) => {
                for value in values.values() {
                    self.reachable(value, output);
                }
            }
            Value::Array(values) => {
                for value in values {
                    self.reachable(value, output);
                }
            }
            Value::String(reference) => {
                if let Ok((namespace, name)) = id(reference) {
                    for kind in ["models", "textures"] {
                        let path = self.root.join(format!(
                            "assets/{namespace}/{kind}/{name}.{}",
                            if kind == "models" { "json" } else { "png" }
                        ));
                        if let Some(bytes) = self.outputs.get(&path) {
                            if output.insert(path.clone()) && kind == "models" {
                                if let Ok(value) = serde_json::from_slice::<Value>(bytes) {
                                    self.reachable(&value, output);
                                }
                            }
                            let metadata = path.with_extension("png.mcmeta");
                            if self.outputs.contains_key(&metadata) {
                                output.insert(metadata);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

struct Rule {
    source: PathBuf,
    origin: String,
    values: Properties,
    items: BTreeSet<String>,
    cmd: Option<i32>,
    weight: i64,
    unsupported: Option<String>,
}

pub(super) const CMD_KEYS: &[&str] = &[
    "nbt.CustomModelData",
    "components.custom_model_data",
    "components.minecraft:custom_model_data",
    "components.custom_model_data.floats.0",
    "components.minecraft:custom_model_data.floats.0",
];

fn rule(source: &Path, origin: &str, catalog: &BTreeSet<String>) -> Result<Option<Rule>> {
    let values = properties::parse(&fs::read(source)?)?;
    if values.get("type").is_some_and(|value| value != "item") {
        return Ok(None);
    }
    let inferred = source.file_stem().unwrap().to_string_lossy();
    let names = values
        .get("items")
        .or_else(|| values.get("matchItems"))
        .map_or(inferred.as_ref(), String::as_str);
    let items = names
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|name| !name.is_empty())
        .map(|name| id(name).map(|(namespace, name)| format!("{namespace}:{name}")))
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(!items.is_empty(), "CIT 物品列表为空");
    for item in &items {
        if item.starts_with("minecraft:") {
            ensure!(catalog.contains(item), "目标版本没有物品 {item}");
        }
    }
    let mut unsupported = Vec::new();
    let mut cmd = None;
    for (key, value) in &values {
        if CMD_KEYS.contains(&key.as_str()) {
            match value.parse::<i32>() {
                Ok(number) if f64::from(number as f32) == f64::from(number) => {
                    if cmd.is_some_and(|old| old != number) {
                        unsupported.push("多个 CMD 条件不一致".into());
                    }
                    cmd = Some(number);
                }
                _ => unsupported.push(format!("{key}={value}")),
            }
        } else if !matches!(
            key.as_str(),
            "type" | "items" | "matchItems" | "model" | "texture" | "weight"
        ) {
            unsupported.push(key.clone());
        }
    }
    Ok(Some(Rule {
        source: source.into(),
        origin: origin.into(),
        cmd,
        weight: values
            .get("weight")
            .map(|value| value.parse())
            .transpose()?
            .unwrap_or(0),
        values,
        items,
        unsupported: (!unsupported.is_empty()).then(|| unsupported.join(", ")),
    }))
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct State {
    items: BTreeMap<String, Saved>,
    assets: BTreeSet<String>,
}
#[derive(Clone, Deserialize, Serialize)]
struct Saved {
    base: Value,
    output: Value,
}

fn warnings(notes: &BTreeMap<String, String>, report: &mut AssetReport) {
    for (path, note) in notes.iter().take(12) {
        report
            .warnings
            .push(format!("CIT 原版适配保留 {path}：{note}"));
    }
    if notes.len() > 12 {
        report.warnings.push(format!(
            "另有 {} 条 CIT 规则需要客户端支持，原规则与资源均保留",
            notes.len() - 12
        ));
    }
}

fn upper(value: i32) -> f64 {
    let value = value as f32;
    f64::from(if value == 0.0 {
        f32::from_bits(1)
    } else if value > 0.0 {
        f32::from_bits(value.to_bits() + 1)
    } else {
        f32::from_bits(value.to_bits() - 1)
    })
}

pub(super) fn dispatch(base: &Value, cases: &BTreeMap<i32, Value>) -> Value {
    let mut entries: BTreeMap<u32, (f64, Value)> = BTreeMap::new();
    for (&number, model) in cases {
        let order = |value: f32| {
            if value < 0.0 {
                !value.to_bits()
            } else {
                value.to_bits() ^ 0x80000000
            }
        };
        entries.insert(order(upper(number) as f32), (upper(number), base.clone()));
        entries.insert(order(number as f32), (f64::from(number), model.clone()));
    }
    let entries: Vec<_> = entries
        .into_values()
        .map(|(threshold, model)| json!({"threshold":threshold,"model":model}))
        .collect();
    json!({"type":"minecraft:condition","property":"minecraft:has_component","component":"minecraft:custom_model_data",
        "on_true":{"type":"minecraft:range_dispatch","property":"minecraft:custom_model_data","index":0,"entries":entries,"fallback":base},"on_false":base})
}

pub(super) fn migrate(
    root: &Path,
    target: u32,
    version: &str,
    paths: &BTreeMap<PathBuf, String>,
    report: &mut AssetReport,
) -> Result<()> {
    if target < 46 {
        return legacy::migrate(root, target, version, paths, report);
    }
    if target >= 55 && native::needed(paths)? {
        return native::migrate(root, target, version, paths, report);
    }
    let cache = root.join("fastpack/cit-items.json");
    if target < 46 || paths.is_empty() && !cache.pack_is_file() {
        return Ok(());
    }
    let previous: State = if cache.pack_is_file() {
        serde_json::from_slice(&fs::read(&cache)?).unwrap_or_default()
    } else {
        State::default()
    };
    let mut rules = Vec::new();
    let mut notes = BTreeMap::new();
    let catalog: BTreeSet<_> = super::custom_items::items(version)
        .iter()
        .map(|item| item.id.clone())
        .collect();
    for (source, origin) in paths {
        match rule(source, origin, &catalog) {
            Ok(Some(value)) => {
                if let Some(unsupported) = &value.unsupported {
                    notes.insert(
                        value.origin.clone(),
                        format!("条件 {unsupported} 需要 CIT 客户端"),
                    );
                }
                rules.push(value);
            }
            Ok(None) => {}
            Err(error) => {
                notes.insert(origin.clone(), format!("{error:#}"));
            }
        }
    }
    rules.sort_by(|a, b| {
        b.weight
            .cmp(&a.weight)
            .then_with(|| a.origin.cmp(&b.origin))
    });
    if previous.items.is_empty() && rules.iter().all(|rule| rule.unsupported.is_some()) {
        warnings(&notes, report);
        return Ok(());
    }
    let mut by_item: BTreeMap<String, Vec<&Rule>> = BTreeMap::new();
    for rule in &rules {
        for item in &rule.items {
            by_item.entry(item.clone()).or_default().push(rule);
        }
    }
    let all_items: BTreeSet<_> = by_item
        .keys()
        .cloned()
        .chain(previous.items.keys().cloned())
        .collect();
    let mut resources = Resources::new(root, target)?;
    let mut state = State {
        items: BTreeMap::new(),
        assets: previous.assets.clone(),
    };
    let mut definitions = BTreeMap::new();
    let mut appearances = Vec::new();
    let mut count = 0;
    for item in all_items {
        let (namespace, name) = id(&item)?;
        let path = root.join(format!("assets/{namespace}/items/{name}.json"));
        let definition = if path.pack_is_file() {
            crate::model_json::parse(&fs::read(&path)?)
        } else {
            Ok(super::modern_items::definition_reference(target,&name).filter(|_|namespace=="minecraft")
                .unwrap_or_else(||json!({"model":{"type":"minecraft:model","model":format!("{namespace}:item/{name}")}})))
        };
        let mut definition = match definition {
            Ok(value) if value["model"].is_object() => value,
            _ => {
                report.warnings.push(format!(
                    "CIT 原版适配：{item} 的已有物品定义无法解析，保留原文与 CIT 规则"
                ));
                continue;
            }
        };
        if let Some(saved) = previous.items.get(&item) {
            if definition["model"] == saved.output {
                definition["model"] = saved.base.clone();
            } else {
                report.warnings.push(format!(
                    "CIT 原版适配：{item} 的已生成定义被修改，保留现有模型树以免覆盖自定义内容"
                ));
                state.items.insert(item, saved.to_owned());
                continue;
            }
        }
        let original = definition["model"].clone();
        let matches = by_item.get(&item).cloned().unwrap_or_default();
        let codes: BTreeSet<_> = matches.iter().filter_map(|rule| rule.cmd).collect();
        let default = matches.iter().find(|rule| rule.cmd.is_none()).copied();
        let mut base = original.clone();
        if let Some(rule) = default {
            if let Some(unsupported) = &rule.unsupported {
                notes.insert(
                    rule.origin.clone(),
                    format!("条件 {unsupported} 需要 CIT 客户端"),
                );
            } else {
                match resources.appearance(&rule.source, &rule.values, &item) {
                    Ok(model) => {
                        base = model;
                        count += 1;
                    }
                    Err(error) => {
                        notes.insert(rule.origin.clone(), format!("资源未接入：{error:#}"));
                    }
                }
            }
        }
        let mut cases = BTreeMap::new();
        for code in codes {
            let Some(rule) = matches
                .iter()
                .find(|rule| rule.cmd.is_none() || rule.cmd == Some(code))
                .copied()
            else {
                continue;
            };
            if rule.cmd.is_none() {
                continue;
            }
            if let Some(unsupported) = &rule.unsupported {
                notes.insert(
                    rule.origin.clone(),
                    format!("条件 {unsupported} 需要 CIT 客户端"),
                );
                continue;
            }
            match resources.appearance(&rule.source, &rule.values, &item) {
                Ok(model) => {
                    cases.insert(code, model);
                    count += 1;
                }
                Err(error) => {
                    notes.insert(rule.origin.clone(), format!("资源未接入：{error:#}"));
                }
            }
        }
        definition["model"] = if cases.is_empty() {
            base
        } else {
            dispatch(&base, &cases)
        };
        if definition["model"] != original {
            state.items.insert(
                item,
                Saved {
                    base: original,
                    output: definition["model"].clone(),
                },
            );
            appearances.push(definition["model"].clone());
            definitions.insert(path, definition);
        } else if previous.items.contains_key(&item) {
            definitions.insert(path, definition);
        }
    }
    let mut reachable = BTreeSet::new();
    for value in &appearances {
        resources.reachable(value, &mut reachable);
    }
    for path in &reachable {
        let bytes = &resources.outputs[path];
        let relative = path
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        ensure!(
            !path.pack_is_file()
                || fs::read(path)? == *bytes
                || previous.assets.contains(&relative),
            "CIT 输出已有作者资源，拒绝覆盖：{relative}"
        );
    }
    for path in &reachable {
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write_atomic(path, &resources.outputs[path])?;
        state.assets.insert(
            path.strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/"),
        );
    }
    let mut changed = 0;
    for (path, value) in definitions {
        let bytes = serde_json::to_vec(&value)?;
        if path.pack_is_file() && crate::model_json::parse(&fs::read(&path)?)? == value {
            continue;
        }
        if path.pack_is_file() {
            backup(root, &path)?;
        }
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write_atomic(&path, &bytes)?;
        changed += 1;
    }
    if !state.items.is_empty() || cache.pack_is_file() {
        fs::create_dir_all(cache.parent().unwrap())?;
        fs::write_atomic(&cache, &serde_json::to_vec(&state)?)?;
    }
    if count > 0 {
        report.converted.push(format!("CIT 原版物品适配：为 {version} 接入 {count} 条物品规则，更新 {changed} 个定义；数字 CMD 精确匹配、未命中保留完整模型树，贴图动画与自定义姿态一并保留"));
    }
    warnings(&notes, report);
    Ok(())
}

#[cfg(test)]
#[path = "cit_items_tests.rs"]
mod tests;
