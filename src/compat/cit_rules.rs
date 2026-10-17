use super::properties::Properties;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

pub(super) fn destination(root: &Path, origin: &str, target: u32) -> Option<PathBuf> {
    let path = Path::new(origin);
    if path
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return None;
    }
    let parts: Vec<_> = origin.split('/').collect();
    if parts.len() < 5 || parts[0] != "assets" || parts[3] != "cit" {
        return None;
    }
    let namespace = parts[1];
    let runtime = parts[2];
    if !["mcpatcher", "optifine", "citresewn"].contains(&runtime) {
        return None;
    }
    let mut result = root.join("assets").join(namespace);
    result = result.join(if target < 4 {
        "mcpatcher/cit"
    } else {
        "optifine/cit"
    });
    let prefix = match (runtime, target < 4) {
        ("mcpatcher", false) => Some("fastpack_legacy"),
        ("optifine", true) => Some("fastpack_optifine"),
        ("citresewn", _) => Some("fastpack_citresewn"),
        _ => None,
    };
    if let Some(prefix) = prefix {
        result = result.join(prefix);
    }
    for part in &parts[4..] {
        result = result.join(if target < 3 {
            part.to_string()
        } else {
            super::cit::clean(part)
        });
    }
    Some(result)
}

pub(super) fn texture_candidates(path: &Path) -> Vec<PathBuf> {
    let value = path.to_string_lossy().replace('\\', "/");
    let Some((base, relative)) = value.split_once("/textures/") else {
        return vec![];
    };
    let Some((folder, name)) = relative.split_once('/') else {
        return vec![];
    };
    let kind = match folder {
        "item" | "items" => "item",
        "block" | "blocks" => "block",
        _ => return vec![],
    };
    let Some(stem) = name.strip_suffix(".png") else {
        return vec![];
    };
    let modern = crate::tables::flatten_texture_name(stem, kind);
    let old = crate::tables::legacy_texture_name(stem, kind).unwrap_or_else(|| stem.into());
    let mut result = vec![];
    for folder in [kind, &format!("{kind}s")] {
        for name in [stem, modern.as_str(), old.as_str()] {
            result.push(PathBuf::from(format!(
                "{base}/textures/{folder}/{name}.png"
            )));
        }
    }
    result
}

fn mapped_key(key: &str, target: u32) -> String {
    let component = key
        .strip_prefix("components.minecraft:")
        .or_else(|| key.strip_prefix("components."));
    let cmd = key == "nbt.CustomModelData"
        || component
            .is_some_and(|key| matches!(key, "custom_model_data" | "custom_model_data.floats.0"));
    if cmd {
        return if target >= 46 {
            "components.minecraft:custom_model_data.floats.0"
        } else if target >= 32 {
            "components.minecraft:custom_model_data"
        } else {
            "nbt.CustomModelData"
        }
        .into();
    }
    for (nbt, modern) in [
        ("display.Name", "custom_name"),
        ("display.Lore", "lore"),
        ("Damage", "damage"),
        ("RepairCost", "repair_cost"),
    ] {
        let suffix = key
            .strip_prefix(&format!("nbt.{nbt}"))
            .filter(|suffix| suffix.is_empty() || suffix.starts_with('.'))
            .or_else(|| {
                component
                    .and_then(|key| key.strip_prefix(modern))
                    .filter(|suffix| suffix.is_empty() || suffix.starts_with('.'))
            });
        if let Some(suffix) = suffix {
            return if target >= 32 {
                format!("components.minecraft:{modern}{suffix}")
            } else {
                format!("nbt.{nbt}{suffix}")
            };
        }
    }
    if target < 32 {
        if let Some(path) = component.and_then(|name| name.strip_prefix("custom_data.")) {
            return format!("nbt.{path}");
        }
    } else if let Some(path) = key.strip_prefix("nbt.") {
        let head = path.split('.').next().unwrap_or(path);
        if ![
            "display",
            "ench",
            "Enchantments",
            "StoredEnchantments",
            "AttributeModifiers",
            "HideFlags",
            "Unbreakable",
            "CanDestroy",
            "CanPlaceOn",
            "Potion",
            "CustomPotionEffects",
            "CustomPotionColor",
            "SkullOwner",
            "EntityTag",
            "BlockEntityTag",
            "BlockStateTag",
            "pages",
            "title",
            "author",
            "generation",
            "resolved",
            "map",
            "Decorations",
            "Fireworks",
            "Explosion",
            "Charged",
            "ChargedProjectiles",
            "Trim",
            "Instrument",
            "instrument",
            "LodestonePos",
            "LodestoneDimension",
            "LodestoneTracked",
            "BucketVariantTag",
            "Items",
        ]
        .contains(&head)
        {
            return format!("components.minecraft:custom_data.{path}");
        }
    }
    key.into()
}

pub(super) fn conditions(
    values: &mut Properties,
    source: Option<u32>,
    target: u32,
    version: Option<&str>,
) -> Vec<String> {
    let mut warnings = vec![];
    enchantments(values, target, &mut warnings);
    let original = values.clone();
    for (key, value) in &original {
        let new = mapped_key(key, target);
        if new != *key {
            if values.get(&new).is_some_and(|old| old != value) {
                warnings.push(format!("{key} 与 {new} 条件冲突，均保留"));
            } else {
                values.remove(key);
                values.insert(new, value.clone());
            }
        } else if target < 32 && key.starts_with("components.") {
            warnings.push(format!(
                "目标早于物品组件版本，{key} 无法等价还原，保留供支持该条件的客户端读取"
            ));
        }
    }
    let renames = &[
        ("grass_path", "dirt_path", 7),
        ("grass", "short_grass", 22),
        ("scute", "turtle_scute", 32),
        ("zombie_pigman_spawn_egg", "zombified_piglin_spawn_egg", 5),
    ];
    let catalog = version.map(super::custom_items::items);
    let is_legacy = source.is_some_and(|source| source < 4);
    if is_legacy && target >= 4 {
        for key in ["items", "matchItems"] {
            if let Some(value) = values.get(key).cloned() {
                values.insert(
                    key.into(),
                    value
                        .split_whitespace()
                        .map(|name| {
                            let block = super::cit::ITEMS.iter().find(|(id, old)| {
                                **id < 256 && name.strip_prefix("minecraft:") == Some(old.as_str())
                            });
                            if !VARIANTS.contains_key(name) {
                                if let Some((id, _)) = block {
                                    if let Some(modern) = BLOCKS.get(&(id * 16)) {
                                        return format!("minecraft:{modern}");
                                    }
                                }
                            }
                            name.to_string()
                        })
                        .collect::<Vec<_>>()
                        .join(" "),
                );
            }
        }
    }
    for key in ["items", "matchItems"] {
        if let Some(names) = values.get(key).cloned() {
            let result = names
                .split_whitespace()
                .map(|name| {
                    let bare = name.strip_prefix("minecraft:").unwrap_or(name);
                    if name.contains(':') && !name.starts_with("minecraft:") {
                        return name.to_string();
                    }
                    let mut mapped = bare;
                    for &(old, new, boundary) in renames {
                        let modern = catalog.as_ref().map_or(target >= boundary, |items| {
                            items
                                .iter()
                                .any(|item| item.id == format!("minecraft:{new}"))
                        });
                        if modern && mapped == old {
                            mapped = new;
                        } else if !modern && mapped == new {
                            mapped = old;
                        }
                    }
                    format!("minecraft:{mapped}")
                })
                .collect::<Vec<_>>()
                .join(" ");
            values.insert(key.into(), result);
        }
    }
    if let Some(catalog) = catalog {
        variants(values, source, target, &catalog, &mut warnings);
        if target < 4 {
            for key in ["items", "matchItems"] {
                if let Some(value) = values.get(key).cloned() {
                    values.insert(
                        key.into(),
                        value
                            .split_whitespace()
                            .map(|name| {
                                let name = if super::cit::ITEMS.values().any(|old| {
                                    name.strip_prefix("minecraft:") == Some(old.as_str())
                                }) {
                                    name.to_string()
                                } else {
                                    BLOCKS
                                        .iter()
                                        .find(|(index, modern)| {
                                            **index % 16 == 0
                                                && name.strip_prefix("minecraft:")
                                                    == Some(modern.as_str())
                                        })
                                        .and_then(|(index, _)| super::cit::ITEMS.get(&(index / 16)))
                                        .map_or_else(
                                            || name.to_string(),
                                            |old| format!("minecraft:{old}"),
                                        )
                                };
                                super::cit::ITEMS
                                    .iter()
                                    .find(|(_, item)| {
                                        name.strip_prefix("minecraft:") == Some(item.as_str())
                                    })
                                    .map_or_else(|| name.to_string(), |(id, _)| id.to_string())
                            })
                            .collect::<Vec<_>>()
                            .join(" "),
                    );
                }
            }
        }
        for key in ["items", "matchItems"] {
            for item in values
                .get(key)
                .into_iter()
                .flat_map(|value| value.split_whitespace())
            {
                if item.starts_with("minecraft:")
                    && item
                        .strip_prefix("minecraft:")
                        .is_some_and(|name| name.parse::<u32>().is_err())
                    && !catalog
                        .iter()
                        .any(|entry| entry.id.split('@').next() == Some(item))
                {
                    warnings.push(format!(
                        "目标版本没有物品 {item}，原规则与资源保留，不会任意替换为别的物品"
                    ));
                }
            }
        }
    }
    warnings
}

fn enchantments(values: &mut Properties, target: u32, warnings: &mut Vec<String>) {
    const ENCHANTMENTS: &[(u16, &str)] = &[
        (0, "protection"),
        (1, "fire_protection"),
        (2, "feather_falling"),
        (3, "blast_protection"),
        (4, "projectile_protection"),
        (5, "respiration"),
        (6, "aqua_affinity"),
        (7, "thorns"),
        (8, "depth_strider"),
        (9, "frost_walker"),
        (10, "binding_curse"),
        (16, "sharpness"),
        (17, "smite"),
        (18, "bane_of_arthropods"),
        (19, "knockback"),
        (20, "fire_aspect"),
        (21, "looting"),
        (32, "efficiency"),
        (33, "silk_touch"),
        (34, "unbreaking"),
        (35, "fortune"),
        (48, "power"),
        (49, "punch"),
        (50, "flame"),
        (51, "infinity"),
        (61, "luck_of_the_sea"),
        (62, "lure"),
        (70, "mending"),
        (71, "vanishing_curse"),
    ];
    let Some(value) = values
        .get("enchantments")
        .or_else(|| values.get("enchantmentIDs"))
        .cloned()
    else {
        return;
    };
    let mut result = vec![];
    let mut valid = true;
    for token in value.split_whitespace() {
        let bare = token.strip_prefix("minecraft:").unwrap_or(token);
        let entry = bare
            .parse::<u16>()
            .ok()
            .and_then(|id| ENCHANTMENTS.iter().find(|(known, _)| *known == id))
            .or_else(|| ENCHANTMENTS.iter().find(|(_, name)| *name == bare));
        if let Some((id, name)) = entry {
            result.push(if target < 4 {
                id.to_string()
            } else {
                format!("minecraft:{name}")
            });
        } else {
            valid = false;
            break;
        }
    }
    if valid {
        let key = if target < 4 {
            "enchantmentIDs"
        } else {
            "enchantments"
        };
        let other = if target < 4 {
            "enchantments"
        } else {
            "enchantmentIDs"
        };
        if values.contains_key(key) && values.contains_key(other) && values[key] != values[other] {
            warnings.push("两种附魔选择条件冲突，均保留".into());
            return;
        }
        values.remove(other);
        values.insert(key.into(), result.join(" "));
    } else if target < 4 {
        warnings.push(
            "附魔选择包含无法映射为旧数字 ID 的条件，原条件保留，需目标客户端支持该附魔".into(),
        );
    }
}

#[cfg(test)]
#[path = "cit_rules_tests.rs"]
mod tests;

static BLOCKS: LazyLock<BTreeMap<u32, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../assets/legacy-ctm-blocks.json")).unwrap()
});
static VARIANTS: LazyLock<BTreeMap<String, BTreeMap<u16, String>>> = LazyLock::new(|| {
    let mut result: BTreeMap<String, BTreeMap<u16, String>> = BTreeMap::new();
    for (&id, old) in super::cit::ITEMS.iter() {
        let name = format!("minecraft:{old}");
        let mut seen = BTreeSet::new();
        for meta in 0..16 {
            let modern = if id < 256 {
                BLOCKS.get(&(id * 16 + u32::from(meta))).cloned()
            } else {
                match (id, meta) {
                    (263, 0) => Some("coal".into()),
                    (263, 1) => Some("charcoal".into()),
                    (322, 0) => Some("golden_apple".into()),
                    (322, 1) => Some("enchanted_golden_apple".into()),
                    (349, index) => ["cod", "salmon", "tropical_fish", "pufferfish"]
                        .get(index as usize)
                        .map(|name| name.to_string()),
                    (350, index) => ["cooked_cod", "cooked_salmon"]
                        .get(index as usize)
                        .map(|name| name.to_string()),
                    (351, index) => [
                        "ink_sac",
                        "red_dye",
                        "green_dye",
                        "cocoa_beans",
                        "lapis_lazuli",
                        "purple_dye",
                        "cyan_dye",
                        "light_gray_dye",
                        "gray_dye",
                        "pink_dye",
                        "lime_dye",
                        "yellow_dye",
                        "light_blue_dye",
                        "magenta_dye",
                        "orange_dye",
                        "bone_meal",
                    ]
                    .get(index as usize)
                    .map(|name| name.to_string()),
                    (397, index) => [
                        "skeleton_skull",
                        "wither_skeleton_skull",
                        "zombie_head",
                        "player_head",
                        "creeper_head",
                        "dragon_head",
                    ]
                    .get(index as usize)
                    .map(|name| name.to_string()),
                    _ => None,
                }
            };
            if let Some(modern) = modern {
                let modern = match modern.as_str() {
                    "grass" => "short_grass".into(),
                    "grass_path" => "dirt_path".into(),
                    _ => modern,
                };
                if !seen.insert(modern.clone()) {
                    continue;
                }
                result
                    .entry(name.clone())
                    .or_default()
                    .insert(meta, format!("minecraft:{modern}"));
            }
        }
    }
    result.retain(|_, values| values.len() > 1);
    result
});

fn metadata(value: &str) -> Option<BTreeSet<u16>> {
    let mut result = BTreeSet::new();
    for token in value.split_whitespace() {
        let (low, high) = if let Some((low, high)) = token.split_once('-') {
            (low.parse::<u16>().ok()?, high.parse::<u16>().ok()?)
        } else {
            let value = token.parse::<u16>().ok()?;
            (value, value)
        };
        if high < low || high - low > 65535 - 1 {
            return None;
        }
        result.extend(low..=high);
    }
    Some(result)
}

fn variants(
    values: &mut Properties,
    source: Option<u32>,
    target: u32,
    catalog: &[super::custom_items::Item],
    warnings: &mut Vec<String>,
) {
    let available: BTreeSet<_> = catalog.iter().map(|item| item.id.as_str()).collect();
    for key in ["items", "matchItems"] {
        let Some(value) = values.get(key).cloned() else {
            continue;
        };
        let names: Vec<_> = value.split_whitespace().collect();
        if target >= 4 && source.is_some_and(|format| format < 4) {
            if names.iter().all(|name| VARIANTS.contains_key(*name)) {
                let damage = values.get("damage");
                let selected = damage
                    .map(|value| metadata(value))
                    .unwrap_or_else(|| Some((0..16).collect()));
                let mask = values
                    .get("damageMask")
                    .is_none_or(|value| value == "65535" || value == "15");
                let Some(selected) = selected.filter(|_| mask) else {
                    warnings
                        .push("旧数据值的百分比或掩码条件无法等价拆成物品 ID，原条件保留".into());
                    continue;
                };
                if damage.is_some()
                    && names.iter().any(|name| {
                        selected
                            .iter()
                            .any(|meta| !VARIANTS[*name].contains_key(meta))
                    })
                {
                    warnings.push(
                        "旧数据值选择含无法对应现代物品的值，完整保留条件，不丢弃其中的分支".into(),
                    );
                    continue;
                }
                let mut items = BTreeSet::new();
                for name in &names {
                    for (meta, name) in &VARIANTS[*name] {
                        if selected.contains(meta) {
                            let name = match name.as_str() {
                                "minecraft:red_dye" if !available.contains("minecraft:red_dye") => {
                                    "minecraft:rose_red"
                                }
                                "minecraft:green_dye"
                                    if !available.contains("minecraft:green_dye") =>
                                {
                                    "minecraft:cactus_green"
                                }
                                "minecraft:short_grass"
                                    if !available.contains("minecraft:short_grass") =>
                                {
                                    "minecraft:grass"
                                }
                                "minecraft:dirt_path"
                                    if !available.contains("minecraft:dirt_path") =>
                                {
                                    "minecraft:grass_path"
                                }
                                "minecraft:yellow_dye"
                                    if !available.contains("minecraft:yellow_dye") =>
                                {
                                    "minecraft:dandelion_yellow"
                                }
                                _ => name,
                            };
                            items.insert(name.to_string());
                        }
                    }
                }
                if !items.is_empty() {
                    values.insert(key.into(), items.into_iter().collect::<Vec<_>>().join(" "));
                    values.remove("damage");
                    values.remove("damageMask");
                }
            }
        } else if target < 4 && !source.is_some_and(|format| format < 4) {
            if values.get("damage").is_some_and(|value| value != "0") {
                continue;
            }
            let mut old = BTreeSet::new();
            let mut damage = BTreeSet::new();
            let mut valid = true;
            for name in &names {
                let name = match *name {
                    "minecraft:rose_red" => "minecraft:red_dye",
                    "minecraft:cactus_green" => "minecraft:green_dye",
                    "minecraft:dandelion_yellow" => "minecraft:yellow_dye",
                    "minecraft:grass" => "minecraft:short_grass",
                    "minecraft:grass_path" => "minecraft:dirt_path",
                    _ => name,
                };
                let found = VARIANTS.iter().find_map(|(old, variants)| {
                    variants
                        .iter()
                        .find(|(_, modern)| modern.as_str() == name)
                        .map(|(meta, _)| (old.clone(), *meta))
                });
                if let Some((name, meta)) = found {
                    old.insert(name);
                    damage.insert(meta);
                } else {
                    valid = false;
                    break;
                }
            }
            if valid && old.len() == 1 && !damage.is_empty() {
                values.insert(key.into(), old.into_iter().next().unwrap());
                values.insert(
                    "damage".into(),
                    damage
                        .iter()
                        .map(u16::to_string)
                        .collect::<Vec<_>>()
                        .join(" "),
                );
            } else if valid && old.len() > 1 {
                warnings.push(
                    "不同旧物品的元数据条件无法合为一条规则，保留原物品选择，不放宽匹配".into(),
                );
            }
        }
    }
}
