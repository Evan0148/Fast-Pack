//! Conditions expressible by the native 1.21.5+ item model predicate API.
use super::properties::Properties;
use anyhow::{Result, bail, ensure};
use serde_json::{Value, json};
use std::collections::BTreeSet;
#[cfg(test)]
#[path = "cit_conditions_tests.rs"]
mod tests;

#[derive(Clone, Debug)]
pub(super) enum Test {
    Any(Vec<Test>),
    Component {
        predicate: String,
        value: Value,
    },
    Range {
        property: String,
        ranges: Vec<(f64, Option<f64>)>,
        normalize: bool,
    },
    Cmd(i32),
}

impl Test {
    pub fn wrap(&self, yes: Value, no: Value) -> Value {
        match self {
            Self::Any(tests) => tests
                .iter()
                .rev()
                .fold(no, |fallback, test| test.wrap(yes.clone(), fallback)),
            Self::Component { predicate, value } => {
                json!({"type":"minecraft:condition","property":"minecraft:component","predicate":predicate,"value":value,"on_true":yes,"on_false":no})
            }
            Self::Range {
                property,
                ranges,
                normalize,
            } => {
                let mut entries = Vec::new();
                for (lower, upper) in ranges {
                    entries.push(json!({"threshold":lower,"model":yes}));
                    if let Some(upper) = upper {
                        entries.push(json!({"threshold":upper,"model":no}));
                    }
                }
                entries.sort_by(|a, b| {
                    a["threshold"]
                        .as_f64()
                        .unwrap()
                        .total_cmp(&b["threshold"].as_f64().unwrap())
                });
                json!({"type":"minecraft:range_dispatch","property":property,"normalize":normalize,"entries":entries,"fallback":no})
            }
            Self::Cmd(code) => {
                super::cit_items::dispatch(&no, &[(*code, yes)].into_iter().collect())
            }
        }
    }
}

impl Test {
    pub fn budget_factor(&self) -> usize {
        match self {
            Self::Any(tests) => tests.iter().map(Self::budget_factor).sum::<usize>().max(1),
            Self::Range { ranges, .. } => ranges.len() + 1,
            Self::Cmd(_) => 4,
            _ => 1,
        }
    }
}

pub(super) fn integer_ranges(value: &str) -> Result<Vec<(u32, Option<u32>)>> {
    let mut output = vec![];
    for token in value.split_whitespace() {
        let (min, max) = if let Some((a, b)) = token.split_once('-') {
            (
                if a.is_empty() { 0 } else { a.parse()? },
                if b.is_empty() { None } else { Some(b.parse()?) },
            )
        } else {
            let value = token.parse()?;
            (value, Some(value))
        };
        ensure!(max.is_none_or(|max| max >= min), "数值区间倒置");
        ensure!(
            min <= 65535 && max.is_none_or(|max| max <= 65535),
            "CIT 数值区间超过 0–65535 的可表达范围"
        );
        output.push((min, max));
    }
    ensure!(
        !output.is_empty() && output.len() <= 64,
        "数值条件为空或过多"
    );
    output.sort_unstable();
    let mut merged: Vec<(u32, Option<u32>)> = vec![];
    for (min, max) in output {
        if let Some((_, old)) = merged
            .last_mut()
            .filter(|(_, end)| end.is_none_or(|end| min <= end.saturating_add(1)))
        {
            *old = match (*old, max) {
                (Some(a), Some(b)) => Some(a.max(b)),
                _ => None,
            };
        } else {
            merged.push((min, max));
        }
    }
    Ok(merged)
}

fn numeric(value: &str) -> Result<Value> {
    let ranges = integer_ranges(value)?;
    ensure!(ranges.len() == 1, "组件谓词需要单个连续区间");
    let (min, max) = ranges[0];
    if max == Some(min) {
        Ok(json!(min))
    } else {
        let mut value = json!({"min":min});
        if let Some(max) = max {
            value["max"] = json!(max);
        }
        Ok(value)
    }
}

pub(super) fn compile(values: &Properties) -> Result<Vec<Test>> {
    let mut used: BTreeSet<&str> = ["type", "items", "matchItems", "model", "texture", "weight"]
        .into_iter()
        .collect();
    let mut tests = vec![];
    let cmd = super::cit_items::CMD_KEYS
        .iter()
        .find_map(|key| values.get(*key));
    if let Some(value) = cmd {
        let code = value.parse::<i32>()?;
        ensure!(
            f64::from(code as f32) == f64::from(code),
            "CMD 无法精确表示"
        );
        tests.push(Test::Cmd(code));
        used.extend(super::cit_items::CMD_KEYS.iter().copied());
    }
    if let Some(ids) = values
        .get("enchantments")
        .or_else(|| values.get("enchantmentIDs"))
    {
        let ids: Vec<_> = ids
            .split_whitespace()
            .map(|id| {
                if id.contains(':') {
                    id.to_string()
                } else {
                    format!("minecraft:{id}")
                }
            })
            .collect();
        ensure!(
            !ids.is_empty()
                && ids
                    .iter()
                    .all(|id| !id.starts_with('!') && super::cit_items::id(id).is_ok()),
            "附魔标识无效"
        );
        let ranges = values
            .get("enchantmentLevels")
            .map(|value| integer_ranges(value))
            .transpose()?
            .unwrap_or_else(|| vec![(1, None)]);
        let mut alternatives = vec![];
        for (min, max) in ranges {
            let mut value = json!({"enchantments":if ids.len()==1{json!(ids[0])}else{json!(ids)}});
            value["levels"] = if max == Some(min) {
                json!(min)
            } else {
                let mut v = json!({"min":min});
                if let Some(max) = max {
                    v["max"] = json!(max);
                }
                v
            };
            alternatives.push(Test::Component {
                predicate: "minecraft:enchantments".into(),
                value: json!([value]),
            });
        }
        if alternatives.len() == 1 {
            tests.extend(alternatives);
        } else {
            tests.push(Test::Any(alternatives));
        }
        used.extend(["enchantments", "enchantmentIDs", "enchantmentLevels"]);
    } else if values.contains_key("enchantmentLevels") {
        bail!("未指定附魔 ID 的等级条件表示所有附魔等级总和，原版模型无法等价计算该总和");
    }
    if let Some(value) = values.get("damage") {
        ensure!(
            !values.contains_key("damageMask"),
            "耐久位掩码无法直接映射原版谓词"
        );
        let percent = value.contains('%');
        let ranges = integer_ranges(&value.replace('%', ""))?
            .into_iter()
            .map(|(min, max)| {
                let factor = if percent { 100.0 } else { 1.0 };
                (
                    f64::from(min) / factor,
                    max.map(|max| f64::from(max + 1) / factor),
                )
            })
            .collect();
        tests.push(Test::Range {
            property: "minecraft:damage".into(),
            ranges,
            normalize: percent,
        });
        used.insert("damage");
    }
    if let Some(value) = values.get("stackSize") {
        let ranges = integer_ranges(value)?
            .into_iter()
            .map(|(min, max)| (f64::from(min), max.map(|max| f64::from(max + 1))))
            .collect();
        tests.push(Test::Range {
            property: "minecraft:count".into(),
            ranges,
            normalize: false,
        });
        used.insert("stackSize");
    }
    let mut fireworks = json!({});
    for (key, value) in values {
        if matches!(
            key.as_str(),
            "nbt.Fireworks.Flight"
                | "components.minecraft:fireworks.flight_duration"
                | "components.fireworks.flight_duration"
        ) {
            fireworks["flight_duration"] = numeric(value)?;
            used.insert(key);
        } else if matches!(
            key.as_str(),
            "nbt.Fireworks.Explosions.0.Type"
                | "components.minecraft:fireworks.explosions.0.shape"
                | "components.fireworks.explosions.0.shape"
        ) {
            ensure!(
                matches!(value.as_str(), "regex:[0-9]+" | "exists:true"),
                "只能等价迁移烟花爆炸列表非空条件"
            );
            fireworks["explosions"] = json!({"contains":[{}]});
            used.insert(key);
        } else if matches!(
            key.as_str(),
            "nbt.Potion"
                | "components.minecraft:potion_contents.potion"
                | "components.potion_contents.potion"
        ) {
            super::cit_items::id(value)?;
            tests.push(Test::Component {
                predicate: "minecraft:potion_contents".into(),
                value: json!([value]),
            });
            used.insert(key);
        }
    }
    if fireworks.as_object().unwrap().len() > 0 {
        tests.push(Test::Component {
            predicate: "minecraft:fireworks".into(),
            value: fireworks,
        });
    }
    for key in values.keys() {
        ensure!(
            used.contains(key.as_str()) || key.starts_with("texture.") || key.starts_with("model."),
            "无法等价迁移条件 {key}"
        );
    }
    Ok(tests)
}

pub(super) fn wrap_all(tests: &[Test], mut yes: Value, no: &Value) -> Value {
    for test in tests.iter().rev() {
        yes = test.wrap(yes, no.clone());
    }
    yes
}
