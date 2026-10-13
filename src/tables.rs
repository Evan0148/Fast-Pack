use anyhow::{Result, bail};
use regex::Regex;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::LazyLock;

pub const LATEST_VERSION: &str = "26.4";
pub const LEGACY_VERSION: &str = "1.7.10";
pub const NEW_META_MIN_FORMAT: u32 = 69;

#[derive(Clone, Debug, Deserialize)]
pub struct VersionProfile {
    pub id: String,
    pub format: [u32; 2],
    pub kind: String,
    pub reference: String,
}

static VERSIONS: LazyLock<Vec<VersionProfile>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../assets/versions.json")).expect("版本配置无效")
});

pub fn version_profiles() -> &'static [VersionProfile] {
    &VERSIONS
}

pub fn version_profile(version: &str) -> Option<&'static VersionProfile> {
    VERSIONS.iter().find(|profile| profile.id == version)
}

pub fn format_components(version: Option<&str>, major: u32) -> [u32; 2] {
    version
        .and_then(version_profile)
        .filter(|profile| profile.format[0] == major)
        .map_or([major, 0], |profile| profile.format)
}

#[derive(Debug, Deserialize)]
pub struct Tables {
    pub pack_formats: Vec<(String, u32)>,
    pub explicit_block: BTreeMap<String, String>,
    pub explicit_item: BTreeMap<String, String>,
    pub inv_block: BTreeMap<String, String>,
    pub inv_item: BTreeMap<String, String>,
    pub colors: Vec<String>,
    pub legacy_stable_block: Vec<String>,
    pub legacy_stable_item: Vec<String>,
    pub deferred_unmapped: Vec<String>,
    pub block_rules: Vec<(String, String)>,
    pub item_rules: Vec<(String, String)>,
    pub inv_block_rules: Vec<(String, String)>,
    pub inv_item_rules: Vec<(String, String)>,
}

static TABLES: LazyLock<Tables> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../assets/tables.json")).expect("内置转换表无效")
});

#[derive(Deserialize)]
pub struct LegacyItems {
    pub items: BTreeMap<String, String>,
    pub aliases: BTreeMap<String, Vec<String>>,
    pub models: BTreeMap<String, String>,
    pub overrides: BTreeMap<String, Vec<serde_json::Value>>,
}

static LEGACY_ITEMS: LazyLock<LegacyItems> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../assets/legacy-items.json")).expect("旧版物品映射无效")
});

pub fn legacy_items() -> &'static LegacyItems {
    &LEGACY_ITEMS
}

#[derive(Deserialize)]
pub struct LegacyBlocks {
    pub aliases: BTreeMap<String, String>,
    pub inverse: BTreeMap<String, String>,
    pub source_names: Vec<String>,
}

static LEGACY_BLOCKS: LazyLock<LegacyBlocks> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../assets/legacy-blocks.json"))
        .expect("旧版方块纹理映射无效")
});

pub fn legacy_blocks() -> &'static LegacyBlocks {
    &LEGACY_BLOCKS
}

type Rules = Vec<(Regex, String)>;

static BLOCK_RULES: LazyLock<Rules> = LazyLock::new(|| compile_rules(&tables().block_rules));
static ITEM_RULES: LazyLock<Rules> = LazyLock::new(|| compile_rules(&tables().item_rules));
static INV_BLOCK_RULES: LazyLock<Rules> =
    LazyLock::new(|| compile_rules(&tables().inv_block_rules));
static INV_ITEM_RULES: LazyLock<Rules> = LazyLock::new(|| compile_rules(&tables().inv_item_rules));

fn compile_rules(rules: &[(String, String)]) -> Rules {
    rules
        .iter()
        .map(|(pattern, replacement)| {
            (
                Regex::new(pattern).expect("内置名称规则无效"),
                replacement.clone(),
            )
        })
        .collect()
}

pub fn tables() -> &'static Tables {
    &TABLES
}

pub fn pack_format_of(version: &str) -> Result<u32> {
    match tables()
        .pack_formats
        .iter()
        .find(|(label, _)| label == version)
    {
        Some((_, format)) => Ok(*format),
        None => bail!("未知版本: {version}"),
    }
}

pub fn version_label_of(format: u32) -> String {
    tables()
        .pack_formats
        .iter()
        .rev()
        .find(|(_, value)| *value == format)
        .map(|(label, _)| label.clone())
        .unwrap_or_else(|| format.to_string())
}

fn pre_legacy(name: &str) -> String {
    let name = name.replace("big_oak", "dark_oak");
    if name == "silver" {
        "light_gray".into()
    } else if let Some(prefix) = name.strip_suffix("_silver") {
        format!("{prefix}_light_gray")
    } else {
        name
    }
}

fn post_legacy(name: &str) -> String {
    let name = name.replace("dark_oak", "big_oak");
    if name == "light_gray" {
        "silver".into()
    } else if let Some(prefix) = name.strip_suffix("_light_gray") {
        format!("{prefix}_silver")
    } else {
        name
    }
}

pub fn flatten_texture_name(stem: &str, kind: &str) -> String {
    let name = pre_legacy(&stem.to_lowercase());
    if kind == "block"
        && let Some(mapped) = legacy_blocks().aliases.get(&stem.to_lowercase())
    {
        return mapped.clone();
    }
    if kind == "block"
        && legacy_blocks()
            .aliases
            .values()
            .any(|mapped| mapped == &stem.to_lowercase())
    {
        return stem.into();
    }
    if kind == "item" {
        let original = stem.to_lowercase();
        if let Some(mapped) = legacy_items().items.get(&original) {
            return mapped.clone();
        }
    }
    let table = if kind == "block" {
        &tables().explicit_block
    } else {
        &tables().explicit_item
    };
    if let Some(result) = table.get(&name) {
        return result.clone();
    }
    let rules = if kind == "block" {
        &*BLOCK_RULES
    } else {
        &*ITEM_RULES
    };
    for (pattern, replacement) in rules {
        if pattern.is_match(&name) {
            return pattern
                .replace_all(&name, replacement.as_str())
                .into_owned();
        }
    }
    if name != stem.to_lowercase() {
        name
    } else {
        stem.into()
    }
}

pub fn legacy_texture_name(stem: &str, kind: &str) -> Option<String> {
    let name = stem.to_lowercase();
    if kind == "block"
        && let Some(mapped) = legacy_blocks().inverse.get(&name)
    {
        return Some(mapped.clone());
    }
    if kind == "item" {
        if let Some((old, _)) = legacy_items().items.iter().find(|(_, new)| **new == name) {
            return Some(old.clone());
        }
        if let Some((old, _)) = legacy_items()
            .aliases
            .iter()
            .find(|(_, names)| names.contains(&name))
        {
            return Some(old.clone());
        }
    }
    let table = if kind == "block" {
        &tables().inv_block
    } else {
        &tables().inv_item
    };
    if let Some(result) = table.get(&name) {
        return Some(post_legacy(result));
    }
    let rules = if kind == "block" {
        &*INV_BLOCK_RULES
    } else {
        &*INV_ITEM_RULES
    };
    for (pattern, replacement) in rules {
        if pattern.is_match(&name) {
            return Some(post_legacy(
                &pattern.replace_all(&name, replacement.as_str()),
            ));
        }
    }
    let whitelist = if kind == "block" {
        &tables().legacy_stable_block
    } else {
        &tables().legacy_stable_item
    };
    (whitelist.contains(&name) || kind == "block" && legacy_blocks().source_names.contains(&name))
        .then(|| stem.into())
}

pub fn parse_drop_data(data: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut buffer = String::new();
    let mut in_brace = false;
    let push = |result: &mut Vec<String>, buffer: &mut String| {
        let value = buffer.trim().to_owned();
        if !value.is_empty() && !result.contains(&value) {
            result.push(value);
        }
        buffer.clear();
    };
    for c in data.trim().chars() {
        match c {
            '{' => {
                in_brace = true;
                buffer.clear();
            }
            '}' => {
                in_brace = false;
                push(&mut result, &mut buffer);
            }
            ' ' if !in_brace => push(&mut result, &mut buffer),
            _ => buffer.push(c),
        }
    }
    push(&mut result, &mut buffer);
    result
}
