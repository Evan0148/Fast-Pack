use super::assets::{AssetReport, backup};
use super::properties;
use crate::packfs::{self as fs, PathExt, WalkDir};
use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

static ITEMS: LazyLock<BTreeMap<u32, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../assets/legacy-cit-items.json"))
        .expect("旧版耐久物品表无效")
});

struct Files {
    paths: BTreeMap<PathBuf, PathBuf>,
    folded: BTreeMap<String, PathBuf>,
}

impl Files {
    fn lookup(&self, path: &Path) -> Option<(&PathBuf, &PathBuf)> {
        self.paths.get_key_value(path).or_else(|| {
            self.folded
                .get(&path.to_string_lossy().to_lowercase())
                .and_then(|path| self.paths.get_key_value(path))
        })
    }
}

fn clean(name: &str) -> String {
    if name
        .bytes()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"._-".contains(&c))
    {
        return name.into();
    }
    let (stem, extension) = name.split_once('.').unwrap_or((name, ""));
    let sanitize = |text: &str| {
        text.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || "._-".contains(c) {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect::<String>()
    };
    let hash = format!("{:x}", Sha256::digest(stem.as_bytes()));
    format!(
        "{}_{}{}",
        sanitize(stem),
        &hash[..12],
        if extension.is_empty() {
            String::new()
        } else {
            format!(".{}", sanitize(extension))
        }
    )
}

fn lexical(root: &Path, path: &Path) -> Option<PathBuf> {
    let mut output = PathBuf::new();
    for component in path.strip_prefix(root).ok()?.components() {
        match component {
            Component::Normal(value) => output.push(value),
            Component::CurDir => {}
            Component::ParentDir if output.pop() => {}
            _ => return None,
        }
    }
    Some(root.join(output))
}

fn reference(
    root: &Path,
    source: &Path,
    value: &str,
    extension: &str,
    files: &Files,
    force: bool,
) -> Option<String> {
    if value.starts_with('#') || value.starts_with("builtin/") || value.is_empty() {
        return None;
    }
    let relative = source.strip_prefix(root.join("assets")).ok()?;
    let namespace = relative.components().next()?.as_os_str().to_str()?;
    let value = value.replace('\\', "/");
    let force = force || value.chars().any(|c| c.is_ascii_uppercase() || c == ' ');
    let value = if value.to_ascii_lowercase().ends_with(extension) {
        value
    } else {
        format!("{value}{extension}")
    };
    let asset = root.join("assets").join(namespace);
    let kind = if extension == ".json" {
        "models"
    } else {
        "textures"
    };
    let candidates = if let Some((namespace, name)) = value.split_once(':') {
        let base = root.join("assets").join(namespace);
        vec![base.join(name), base.join(kind).join(name)]
    } else if let Some(value) = value.strip_prefix("~/") {
        let directory = relative.components().nth(1)?.as_os_str();
        vec![asset.join(directory).join(value)]
    } else if let Some(value) = value.strip_prefix("/assets/") {
        vec![root.join("assets").join(value)]
    } else if value.starts_with("assets/") {
        vec![root.join(value)]
    } else {
        vec![
            asset.join(value.trim_start_matches('/')),
            asset.join(kind).join(&value),
            source.parent()?.join(&value),
        ]
    };
    for candidate in candidates {
        let Some(candidate) = lexical(root, &candidate) else {
            continue;
        };
        let mapped = files.lookup(&candidate);
        let destination = mapped.map_or(&candidate, |(_, destination)| destination);
        if mapped.is_none() && !destination.pack_is_file() {
            continue;
        }
        if !force && destination == &candidate {
            return None;
        }
        let name = destination
            .strip_prefix(root.join("assets"))
            .ok()?
            .to_str()?
            .replace('\\', "/");
        let (namespace, name) = name.split_once('/')?;
        return Some(format!("{namespace}:{}", name.trim_end_matches(extension)));
    }
    None
}

fn item_names(value: &str) -> Option<String> {
    let mut result = Vec::new();
    for token in value
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|token| !token.is_empty())
    {
        let numeric = token.strip_prefix("minecraft:").unwrap_or(token);
        if let Ok(id) = numeric.parse::<u32>() {
            result.push(format!("minecraft:{}", ITEMS.get(&id)?));
        } else if let Some((first, last)) = numeric.split_once('-')
            && let (Ok(first), Ok(last)) = (first.parse::<u32>(), last.parse::<u32>())
        {
            if last < first || last - first > 4096 {
                return None;
            }
            for id in first..=last {
                result.push(format!("minecraft:{}", ITEMS.get(&id)?));
            }
        } else {
            result.push(token.to_string());
        }
    }
    Some(result.join(" "))
}

fn model_refs(
    value: &mut serde_json::Value,
    root: &Path,
    source: &Path,
    files: &Files,
    force: bool,
) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, value) in object {
                if key == "textures" {
                    if let Some(textures) = value.as_object_mut() {
                        for texture in textures.values_mut() {
                            if let Some(name) = texture.as_str().and_then(|name| {
                                reference(root, source, name, ".png", files, force)
                            }) {
                                *texture = name.into();
                            }
                        }
                    }
                } else if (key == "parent" || key == "model") && value.is_string() {
                    if let Some(name) =
                        reference(root, source, value.as_str().unwrap(), ".json", files, force)
                    {
                        *value = name.into();
                    }
                } else {
                    model_refs(value, root, source, files, force);
                }
            }
        }
        serde_json::Value::Array(array) => {
            for value in array {
                model_refs(value, root, source, files, force);
            }
        }
        _ => {}
    }
}

pub(super) fn migrate(
    root: &Path,
    target: u32,
    report: &mut AssetReport,
) -> Result<BTreeMap<PathBuf, String>> {
    if target < 4 || !root.join("assets").pack_is_dir() {
        return Ok(BTreeMap::new());
    }
    let mut files = BTreeMap::new();
    for namespace in fs::read_dir(root.join("assets"))? {
        let namespace = namespace?.path();
        for runtime in ["mcpatcher", "optifine", "citresewn"] {
            let directory = namespace.join(runtime).join("cit");
            if !directory.pack_is_dir() {
                continue;
            }
            let destination = if runtime == "mcpatcher" {
                namespace.join("optifine/cit/fastpack_legacy")
            } else {
                directory.clone()
            };
            for entry in WalkDir::new(&directory).min_depth(1) {
                let entry = entry?;
                if !entry.file_type().is_file() {
                    continue;
                }
                let relative: PathBuf = entry
                    .path()
                    .strip_prefix(&directory)?
                    .components()
                    .map(|part| clean(&part.as_os_str().to_string_lossy()))
                    .collect();
                files.insert(entry.path().to_path_buf(), destination.join(relative));
            }
        }
    }
    if files.is_empty() {
        return Ok(BTreeMap::new());
    }
    let files = Files {
        folded: files
            .keys()
            .map(|path| (path.to_string_lossy().to_lowercase(), path.clone()))
            .collect(),
        paths: files,
    };
    let mut destinations = BTreeSet::new();
    for (source, destination) in &files.paths {
        ensure!(destinations.insert(destination), "CIT 资源路径发生冲突");
        ensure!(
            source == destination
                || !destination.pack_is_file()
                || fs::read(source)? == fs::read(destination)?,
            "CIT 目标已有不同内容，保留原资源：{}",
            destination.display()
        );
    }
    let mut changed = 0;
    let mut armor = 0;
    for (source, destination) in &files.paths {
        let original = fs::read(source)?;
        let mut bytes = original.clone();
        if source
            .extension()
            .is_some_and(|value| value.to_string_lossy().eq_ignore_ascii_case("properties"))
        {
            let mut values = match properties::parse(&bytes) {
                Ok(values) => values,
                Err(error) => {
                    report.warnings.push(format!(
                        "CIT 规则 {} 无法解析，保留原文：{error:#}",
                        source.strip_prefix(root)?.display()
                    ));
                    continue;
                }
            };
            let before = values.clone();
            armor += usize::from(values.get("type").is_some_and(|value| value == "armor"));
            for key in ["items", "matchItems"] {
                if let Some(value) = values.get(key).cloned() {
                    if let Some(value) = item_names(&value) {
                        values.insert(key.into(), value);
                    } else {
                        report.warnings.push(format!(
                            "CIT 规则 {} 的物品 ID 无法完整确定，保留选择条件：{value}",
                            source.strip_prefix(root)?.display()
                        ));
                    }
                }
            }
            for (key, value) in &mut values {
                if key == "texture"
                    || key.starts_with("texture.")
                    || key == "model"
                    || key.starts_with("model.")
                {
                    let extension = if key.starts_with("model") {
                        ".json"
                    } else {
                        ".png"
                    };
                    if let Some(name) = reference(
                        root,
                        source,
                        value,
                        extension,
                        &files,
                        source != destination,
                    ) {
                        *value = name;
                    } else if source != destination {
                        report.warnings.push(format!(
                            "CIT 规则 {} 的 {key}={value} 缺少可确认资源，保留原引用",
                            source.strip_prefix(root)?.display()
                        ));
                    }
                }
            }
            if values != before {
                bytes = properties::write(&values).into_bytes();
            }
            changed += usize::from(bytes != original || source != destination);
        } else if source
            .extension()
            .is_some_and(|value| value.to_string_lossy().eq_ignore_ascii_case("json"))
            && let Ok(mut value) = crate::model_json::parse(&bytes)
        {
            model_refs(&mut value, root, source, &files, source != destination);
            let normalized = serde_json::to_vec_pretty(&value)?;
            if crate::model_json::parse(&original)? != value {
                bytes = normalized;
            }
        }
        if source != destination || original != bytes {
            if source.extension().is_some_and(|value| {
                ["properties", "json"]
                    .iter()
                    .any(|ext| value.to_string_lossy().eq_ignore_ascii_case(ext))
            }) {
                backup(root, source)?;
            }
            fs::create_dir_all(destination.parent().unwrap())?;
            fs::write_atomic(destination, &bytes)?;
        }
    }
    for (source, destination) in &files.paths {
        if source != destination && destination.pack_is_file() {
            fs::remove_file(source)?;
        }
    }
    if changed > 0 {
        report.converted.push(format!("自定义耐久材质：迁移 {changed} 条 CIT 规则（穿戴盔甲 {armor} 条），转换旧数字物品 ID、非法路径和相对贴图／模型引用；保留耐久区间、颜色、权重与条件"));
    }
    if armor > 0 {
        report.warnings.push("耐久穿戴变色需要客户端启用兼容的 CIT 盔甲纹理功能（如 OptiFine 或支持 armor 的 CIT 实现）；原版客户端不会执行 CIT 规则，转换器不会修改客户端文件".into());
    }
    Ok(files
        .paths
        .into_iter()
        .filter(|(source, _)| {
            source
                .extension()
                .is_some_and(|ext| ext.to_string_lossy().eq_ignore_ascii_case("properties"))
        })
        .map(|(source, destination)| {
            let path = if destination.pack_is_file() {
                destination
            } else {
                source.clone()
            };
            (
                path,
                source
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            )
        })
        .collect())
}

#[cfg(test)]
#[path = "cit_tests.rs"]
mod tests;
