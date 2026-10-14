use super::assets::{AssetReport, backup};
use super::properties::{self, Properties};
use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

static BLOCKS: LazyLock<BTreeMap<u32, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../assets/legacy-ctm-blocks.json"))
        .expect("旧方块状态表无效")
});

fn clean(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._-".contains(c) {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn numbers(value: &str) -> Option<Vec<u32>> {
    let mut result = vec![];
    for item in value.split_whitespace() {
        if let Some((first, last)) = item.split_once('-') {
            let first: u32 = first.parse().ok()?;
            let last: u32 = last.parse().ok()?;
            if last < first || last - first > 4096 {
                return None;
            }
            result.extend(first..=last);
        } else {
            result.push(item.parse().ok()?);
        }
    }
    Some(result)
}

fn texture(value: &str) -> String {
    let value = value.trim_end_matches(".png");
    let (namespace, name) = value.split_once(':').unwrap_or(("minecraft", value));
    let name = name.strip_prefix("textures/").unwrap_or(name);
    let name = name
        .strip_prefix("blocks/")
        .or_else(|| name.strip_prefix("block/"))
        .unwrap_or(name);
    if namespace != "minecraft" {
        return value.into();
    }
    crate::tables::flatten_texture_name(name, "block")
}

fn matches(values: &mut Properties, stem: &str, target: u32) -> Option<()> {
    if let Some(value) = values.get("matchBlocks").cloned() {
        let metadata = values.get("metadata").and_then(|value| numbers(value));
        let mut result = BTreeSet::new();
        for token in value.split_whitespace() {
            let mut split = token.split(':');
            let first = split.next()?;
            if let Ok(block) = first.parse::<u32>() {
                let specific = split
                    .next()
                    .and_then(numbers)
                    .or_else(|| metadata.clone())
                    .unwrap_or_else(|| (0..16).collect());
                let mut found = false;
                for meta in specific {
                    if let Some(name) = BLOCKS.get(&(block.checked_mul(16)? + meta)) {
                        result.insert(format!("minecraft:{name}"));
                        found = true;
                    }
                }
                if !found {
                    return None;
                }
            } else if first == "minecraft" || !token.contains(':') {
                let name = token.strip_prefix("minecraft:").unwrap_or(token);
                let (name, properties) = name
                    .split_once(':')
                    .map(|(name, properties)| (name, format!(":{properties}")))
                    .unwrap_or((name, String::new()));
                if let Ok(block) = name.parse::<u32>() {
                    for meta in metadata.clone().unwrap_or_else(|| (0..16).collect()) {
                        if let Some(name) = BLOCKS.get(&(block.checked_mul(16)? + meta)) {
                            result.insert(format!("minecraft:{name}"));
                        }
                    }
                } else {
                    result.insert(format!(
                        "minecraft:{}{properties}",
                        crate::tables::flatten_texture_name(name, "block")
                    ));
                }
            } else {
                result.insert(token.into());
            }
        }
        if result.is_empty() {
            return None;
        }
        values.insert(
            "matchBlocks".into(),
            result
                .into_iter()
                .map(|name| {
                    if target >= 7 && name == "minecraft:grass_path" {
                        "minecraft:dirt_path".into()
                    } else if target >= 22 && name == "minecraft:grass" {
                        "minecraft:short_grass".into()
                    } else {
                        name
                    }
                })
                .collect::<Vec<_>>()
                .join(" "),
        );
        values.remove("metadata");
    } else if let Some(value) = values.get("matchTiles").cloned() {
        values.insert(
            "matchTiles".into(),
            value
                .split_whitespace()
                .map(texture)
                .collect::<Vec<_>>()
                .join(" "),
        );
        values.remove("metadata");
    } else {
        values.insert("matchTiles".into(), texture(stem));
        values.remove("metadata");
    }
    Some(())
}

fn relative(root: &Path, base: &Path, value: &str) -> Option<PathBuf> {
    let mut path = if let Some(value) = value.strip_prefix("~/") {
        root.join("assets/minecraft/mcpatcher").join(value)
    } else if let Some(value) = value.strip_prefix("/assets/") {
        root.join("assets").join(value)
    } else if let Some((namespace, path)) = value.split_once(':') {
        root.join(format!(
            "assets/{namespace}/{}",
            path.strip_prefix("textures/")
                .map(|_| path.to_string())
                .unwrap_or_else(|| format!("textures/{path}"))
        ))
    } else {
        base.join(value)
    };
    if path.extension().is_none() {
        path.set_extension("png");
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return None;
    }
    path.is_file().then_some(path)
}

pub(super) fn migrate(root: &Path, target: u32, report: &mut AssetReport) -> Result<()> {
    if target < 4 {
        return Ok(());
    }
    let directory = root.join("assets/minecraft/mcpatcher/ctm");
    if !directory.is_dir() {
        return Ok(());
    }
    let mut files = BTreeMap::new();
    let mut paths = BTreeMap::new();
    let entries = walkdir::WalkDir::new(&directory)
        .min_depth(1)
        .sort_by_file_name()
        .into_iter()
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let rule_order: BTreeMap<_, _> = entries
        .iter()
        .filter(|entry| {
            entry.file_type().is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "properties")
        })
        .map(|entry| entry.path().to_path_buf())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .enumerate()
        .map(|(index, path)| (path, index))
        .collect();
    for entry in entries {
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry.path().strip_prefix(&directory)?;
        let parent = relative.parent().unwrap_or(Path::new(""));
        let name = entry.path().file_name().unwrap().to_string_lossy();
        let digest = format!("{:x}", Sha256::digest(parent.to_string_lossy().as_bytes()));
        let folder = parent
            .file_name()
            .map(|value| clean(&value.to_string_lossy()))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "root".into());
        let mut output = root.join(format!(
            "assets/minecraft/optifine/ctm/fastpack_legacy/{folder}_{}/{}",
            &digest[..12],
            clean(&name)
        ));
        if entry
            .path()
            .extension()
            .is_some_and(|extension| extension == "properties")
        {
            let rule_order = rule_order[entry.path()];
            output = root.join(format!(
                "assets/minecraft/optifine/ctm/fastpack_legacy/{rule_order:05}_{}.properties",
                clean(&entry.path().file_stem().unwrap().to_string_lossy())
            ));
        }
        ensure!(!paths.contains_key(&output), "连接纹理规范化发生路径冲突");
        paths.insert(output.clone(), entry.path().to_path_buf());
        files.insert(entry.path().to_path_buf(), output);
    }
    let mut migrated = 0;
    let mut disabled = 0;
    for (source, destination) in &files {
        if source
            .extension()
            .is_none_or(|extension| extension != "properties")
        {
            if !destination.is_file() {
                fs::create_dir_all(destination.parent().unwrap())?;
                fs::copy(source, destination)?;
            }
            continue;
        }
        let mut values = match properties::parse(&fs::read(source)?) {
            Ok(values) => values,
            Err(error) => {
                backup(root, source)?;
                fs::remove_file(source)?;
                disabled += 1;
                report.warnings.push(format!(
                    "连接纹理 {} 无法解析，已备份并停用该规则：{error:#}",
                    source.strip_prefix(root)?.display()
                ));
                continue;
            }
        };
        let mut valid = matches(
            &mut values,
            &source.file_stem().unwrap().to_string_lossy(),
            target,
        )
        .is_some();
        if let Some(value) = values.get("tiles").cloned() {
            let mut tiles = vec![];
            for token in value.split_whitespace() {
                let expanded = numbers(token)
                    .map(|numbers| numbers.into_iter().map(|value| value.to_string()).collect())
                    .unwrap_or_else(|| vec![token.to_string()]);
                for token in expanded {
                    if ["<default>", "<skip>"].contains(&token.as_str()) {
                        tiles.push(token);
                        continue;
                    }
                    let path = relative(root, source.parent().unwrap(), &token);
                    if let Some(output) = path.as_ref().and_then(|path| files.get(path)) {
                        let name = output
                            .strip_prefix(root.join("assets/minecraft"))?
                            .to_string_lossy()
                            .replace('\\', "/");
                        tiles.push(name.trim_end_matches(".png").to_string());
                    } else {
                        valid = false;
                    }
                }
            }
            if values.get("method").is_some_and(|method| method == "ctm") {
                if tiles.len() < 47 {
                    valid = false;
                } else {
                    tiles.truncate(47);
                }
            }
            values.insert("tiles".into(), tiles.join(" "));
        }
        backup(root, source)?;
        if valid {
            fs::create_dir_all(destination.parent().unwrap())?;
            let text = properties::write(&values);
            if !destination.is_file() || fs::read(destination)? != text.as_bytes() {
                fs::write(destination, text)?;
            }
            migrated += 1;
        } else {
            disabled += 1;
            report.warnings.push(format!("连接纹理 {} 的方块选择或图块文件无法完整解析，已备份并停用该规则，使用正常基础材质",source.strip_prefix(root)?.display()));
        }
        fs::remove_file(source)?;
    }
    if migrated + disabled > 0 {
        report.converted.push(format!("连接纹理：自动迁移 {migrated} 条旧 MCPatcher 规则，规范路径并转换旧方块 / 颜色匹配；停用 {disabled} 条缺少必要资源的规则，避免覆盖为紫黑贴图"));
    }
    Ok(())
}
