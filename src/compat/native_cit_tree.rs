use super::*;

#[derive(Clone, Debug)]
pub(super) enum Atom {
    Component(String, Value),
    Damage {
        min: u32,
        max: Option<u32>,
        percent: bool,
    },
    Trim(String),
    Cmd {
        min: u32,
        max: Option<u32>,
    },
}
#[derive(Clone, Debug, Default)]
pub(super) struct Case {
    pub tests: Vec<Atom>,
    pub runtime: BTreeMap<String, f64>,
    pub parts: Vec<Value>,
}

pub(super) fn compile(node: &Value) -> Result<Vec<Case>> {
    expand(node, 0)
}
fn kind<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key]
        .as_str()
        .unwrap_or("")
        .trim_start_matches("minecraft:")
}

fn expand(node: &Value, depth: usize) -> Result<Vec<Case>> {
    ensure!(depth < 128, "模型树嵌套超过安全深度");
    let cases = match kind(node, "type") {
        "model" => vec![Case {
            parts: vec![node.clone()],
            ..Default::default()
        }],
        "empty" => vec![Case::default()],
        "composite" => {
            let mut result = vec![Case::default()];
            for child in node["models"].as_array().context("叠加模型缺少 models")? {
                let children = expand(child, depth + 1)?;
                ensure!(
                    result.len().saturating_mul(children.len()) <= 8192,
                    "组合模型条件过多"
                );
                let mut product = vec![];
                for base in &result {
                    for child in &children {
                        let mut merged = base.clone();
                        merged.tests.extend(child.tests.clone());
                        merged.parts.extend(child.parts.clone());
                        let mut compatible = true;
                        for (key, value) in &child.runtime {
                            if merged.runtime.get(key).is_some_and(|old| old != value) {
                                compatible = false;
                                break;
                            }
                            merged.runtime.insert(key.clone(), *value);
                        }
                        if compatible {
                            product.push(merged);
                        }
                    }
                }
                result = product;
            }
            result
        }
        "condition" => {
            let mut yes = expand(&node["on_true"], depth + 1)?;
            let no = expand(&node["on_false"], depth + 1)?;
            match kind(node, "property") {
                "component" => {
                    let predicate = node["predicate"]
                        .as_str()
                        .context("组件条件缺少 predicate")?;
                    ensure!(
                        matches!(
                            predicate,
                            "minecraft:enchantments"
                                | "minecraft:stored_enchantments"
                                | "minecraft:potion_contents"
                                | "minecraft:damage"
                        ),
                        "旧版 CIT 无法等价表达组件谓词 {predicate}"
                    );
                    for case in &mut yes {
                        case.tests
                            .push(Atom::Component(predicate.into(), node["value"].clone()));
                    }
                }
                "using_item" => {
                    for case in &mut yes {
                        case.runtime.insert("__using_item".into(), 1.0);
                    }
                }
                other => bail!("旧版 CIT 无法等价表达布尔属性 {other}"),
            }
            yes.into_iter().chain(no).collect()
        }
        "select" => {
            let entries = node["cases"].as_array().context("选择模型缺少 cases")?;
            match kind(node, "property") {
                "display_context" => {
                    let gui = entries.iter().find(|entry| {
                        entry["when"] == "gui"
                            || entry["when"]
                                .as_array()
                                .is_some_and(|values| values.iter().any(|value| value == "gui"))
                    });
                    expand(
                        gui.map_or(&node["fallback"], |entry| &entry["model"]),
                        depth + 1,
                    )?
                }
                "trim_material" => {
                    let mut result = vec![];
                    for entry in entries {
                        let alternatives = entry["when"]
                            .as_array()
                            .cloned()
                            .unwrap_or_else(|| vec![entry["when"].clone()]);
                        for value in alternatives {
                            let value = value.as_str().context("纹饰材料不是字符串")?;
                            resource(value)?;
                            for mut case in expand(&entry["model"], depth + 1)? {
                                case.tests.push(Atom::Trim(value.into()));
                                result.push(case);
                            }
                        }
                    }
                    result.extend(expand(&node["fallback"], depth + 1)?);
                    result
                }
                other => bail!("旧版 CIT 无法等价表达选择属性 {other}"),
            }
        }
        "range_dispatch" => {
            ensure!(
                node.get("index").is_none_or(|index| index == 0),
                "非首个 CMD 通道无法等价转换为旧版数字 CMD"
            );
            ensure!(
                node.get("remaining")
                    .is_none_or(|remaining| remaining == false),
                "剩余使用时长与旧版拉弓进度语义不同"
            );
            let mut entries = node["entries"]
                .as_array()
                .context("数值模型缺少 entries")?
                .clone();
            entries.sort_by(|a, b| {
                a["threshold"]
                    .as_f64()
                    .unwrap_or(0.0)
                    .total_cmp(&b["threshold"].as_f64().unwrap_or(0.0))
            });
            let scale = node["scale"].as_f64().unwrap_or(1.0);
            ensure!(
                scale.is_finite() && scale > 0.0,
                "数值模型 scale 必须为正数"
            );
            let property = kind(node, "property");
            let normalize = node["normalize"].as_bool().unwrap_or(true);
            let mut result = vec![];
            for index in (0..entries.len()).rev() {
                let lower = entries[index]["threshold"]
                    .as_f64()
                    .context("threshold 不是数字")?
                    / scale;
                let upper = entries
                    .get(index + 1)
                    .and_then(|entry| entry["threshold"].as_f64())
                    .map(|value| value / scale);
                let branches = expand(&entries[index]["model"], depth + 1)?;
                result.extend(interval(branches, property, lower, upper, normalize)?);
            }
            let upper = entries
                .first()
                .and_then(|entry| entry["threshold"].as_f64())
                .map(|value| value / scale);
            if upper.is_none_or(|upper| upper > 0.0) {
                result.extend(interval(
                    expand(&node["fallback"], depth + 1)?,
                    property,
                    0.0,
                    upper,
                    normalize,
                )?);
            }
            result
        }
        other => bail!("不支持的原版模型节点 {other}"),
    };
    ensure!(cases.len() <= 8192, "展开后的模型条件过多");
    // Empty composites still provide a valid transparent appearance.
    ensure!(
        cases.iter().all(|case| case.parts.len() <= 8),
        "叠加模型图层过多，保留原始定义"
    );
    Ok(cases)
}

fn interval(
    mut cases: Vec<Case>,
    property: &str,
    lower: f64,
    upper: Option<f64>,
    normalize: bool,
) -> Result<Vec<Case>> {
    ensure!(
        lower.is_finite()
            && lower >= 0.0
            && upper.is_none_or(|upper| upper.is_finite() && upper >= lower),
        "数值区间无效"
    );
    match property {
        "damage" | "custom_model_data" => {
            let factor = if property == "damage" && normalize {
                100.0
            } else {
                1.0
            };
            ensure!(
                (lower * factor - (lower * factor).round()).abs() < 1e-6
                    && upper.is_none_or(
                        |upper| (upper * factor - (upper * factor).round()).abs() < 1e-6
                    ),
                "旧版数值区间无法无损表示此小数边界"
            );
            let min = (lower * factor).round() as u32;
            let max = upper.map(|upper| ((upper * factor).round() as u32).saturating_sub(1));
            for case in &mut cases {
                case.tests.push(if property == "damage" {
                    Atom::Damage {
                        min,
                        max,
                        percent: normalize,
                    }
                } else {
                    Atom::Cmd { min, max }
                });
            }
        }
        "use_duration" => {
            // The native property is ticks; pre-1.21.4 bow pull is duration / 20.
            for case in &mut cases {
                case.runtime.insert("pull".into(), lower / 20.0);
            }
        }
        other => bail!("旧版模型无法表达数值属性 {other}"),
    }
    Ok(cases)
}

fn range(value: &Value) -> Result<String> {
    if let Some(value) = value.as_u64() {
        return Ok(value.to_string());
    }
    ensure!(value.is_object(), "数值条件不是整数或区间");
    ensure!(
        value
            .as_object()
            .unwrap()
            .iter()
            .all(|(key, value)| matches!(key.as_str(), "min" | "max") && value.as_u64().is_some()),
        "数值条件含不支持的边界字段"
    );
    let min = value["min"].as_u64().unwrap_or(0);
    if let Some(max) = value["max"].as_u64() {
        ensure!(max >= min, "条件区间倒置");
        if max == min {
            return Ok(min.to_string());
        }
        Ok(format!("{min}-{max}"))
    } else {
        Ok(format!("{min}-"))
    }
}

pub(super) fn properties(tests: &[Atom]) -> Result<Properties> {
    let mut props = Properties::new();
    for test in tests {
        match test {
            Atom::Component(predicate, value) => match predicate.as_str() {
                "minecraft:enchantments" | "minecraft:stored_enchantments" => {
                    let entries = value.as_array().context("附魔谓词必须是列表")?;
                    ensure!(
                        entries.len() == 1,
                        "多个附魔的联合条件无法写为单个旧版附魔过滤器"
                    );
                    let entry = &entries[0];
                    ensure!(
                        entry.as_object().is_some_and(|map| map
                            .keys()
                            .all(|key| matches!(key.as_str(), "enchantments" | "levels"))),
                        "附魔条件含未知字段"
                    );
                    let ids = if let Some(id) = entry["enchantments"].as_str() {
                        id.to_string()
                    } else {
                        entry["enchantments"]
                            .as_array()
                            .context("未指定附魔 ID，无法用旧版附魔总和语义代替")?
                            .iter()
                            .map(|value| {
                                value
                                    .as_str()
                                    .map(str::to_owned)
                                    .context("附魔标识不是字符串")
                            })
                            .collect::<Result<Vec<_>>>()?
                            .join(" ")
                    };
                    ensure!(!ids.is_empty(), "附魔 ID 列表为空");
                    for id in ids.split_whitespace() {
                        resource(id)?;
                    }
                    ensure!(
                        props.get("enchantments").is_none_or(|old| *old == ids),
                        "多个不同附魔联合条件不能放宽为或关系"
                    );
                    put(&mut props, "enchantments".into(), ids)?;
                    if entry.get("levels").is_some() {
                        put(
                            &mut props,
                            "enchantmentLevels".into(),
                            range(&entry["levels"])?,
                        )?;
                    }
                    if predicate == "minecraft:stored_enchantments" {
                        bail!("附魔书的存储附魔与普通附魔不能直接互换");
                    }
                }
                "minecraft:potion_contents" => {
                    let potions = value.as_array().context("药水谓词必须是注册名列表")?;
                    let values = potions
                        .iter()
                        .map(|value| value.as_str().context("药水标识不是字符串"))
                        .collect::<Result<Vec<_>>>()?;
                    ensure!(!values.is_empty(), "药水条件为空");
                    for id in &values {
                        resource(id)?;
                    }
                    put(
                        &mut props,
                        "nbt.Potion".into(),
                        if values.len() == 1 {
                            values[0].into()
                        } else {
                            format!(
                                "regex:(?:{})",
                                values
                                    .iter()
                                    .map(|value| regex::escape(value))
                                    .collect::<Vec<_>>()
                                    .join("|")
                            )
                        },
                    )?;
                }
                "minecraft:damage" => {
                    ensure!(
                        value
                            .as_object()
                            .is_some_and(|map| map.keys().all(|key| key == "damage")),
                        "剩余耐久谓词需要物品最大耐久，原定义保留"
                    );
                    put(&mut props, "damage".into(), range(&value["damage"])?)?;
                }
                _ => bail!("不支持的组件谓词 {predicate}"),
            },
            Atom::Damage { min, max, percent } => {
                let suffix = if *percent { "%" } else { "" };
                put(
                    &mut props,
                    "damage".into(),
                    if *max == Some(*min) {
                        format!("{min}{suffix}")
                    } else {
                        format!(
                            "{min}-{}{suffix}",
                            max.map(|max| max.to_string()).unwrap_or_default()
                        )
                    },
                )?;
            }
            Atom::Trim(material) => {
                put(&mut props, "nbt.Trim.material".into(), material.clone())?;
            }
            Atom::Cmd { min, max } => {
                put(
                    &mut props,
                    "nbt.CustomModelData".into(),
                    if *max == Some(*min) {
                        min.to_string()
                    } else {
                        format!(
                            "range:{min}-{}",
                            max.map(|max| max.to_string()).unwrap_or_default()
                        )
                    },
                )?;
            }
        }
    }
    Ok(props)
}

fn put(props: &mut Properties, key: String, value: String) -> Result<()> {
    ensure!(
        props.get(&key).is_none_or(|old| *old == value),
        "联合条件 {key} 不能在旧版中安全合并，原模型树保留"
    );
    props.insert(key, value);
    Ok(())
}
