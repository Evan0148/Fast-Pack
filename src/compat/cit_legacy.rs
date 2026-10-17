use super::*;

pub(super) fn migrate(
    root: &Path,
    target: u32,
    version: &str,
    paths: &BTreeMap<PathBuf, String>,
    report: &mut AssetReport,
) -> Result<()> {
    if target < 4 || matches!(version, "1.13" | "1.13.1" | "1.13.2") {
        if !paths.is_empty() {
            report.warnings.push("此目标版本的原版渲染没有 CustomModelData 物品属性，CIT 规则与资源已迁移，条件外观需要该版本的兼容 CIT 客户端".into());
        }
        return Ok(());
    }
    let cache = root.join("fastpack/cit-legacy-items.json");
    let previous: State = if cache.pack_is_file() {
        serde_json::from_slice(&fs::read(&cache)?).unwrap_or_default()
    } else {
        State::default()
    };
    if paths.is_empty() && previous.items.is_empty() {
        return Ok(());
    }
    let catalog: BTreeSet<_> = super::super::custom_items::items(version)
        .iter()
        .map(|item| item.id.clone())
        .collect();
    let mut rules = vec![];
    let mut notes = BTreeMap::new();
    for (path, origin) in paths {
        match rule(path, origin, &catalog) {
            Ok(Some(rule)) => {
                if let Some(reason) = &rule.unsupported {
                    notes.insert(origin.clone(), format!("条件 {reason} 需要 CIT 客户端"));
                }
                rules.push(rule);
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
    let items: BTreeSet<_> = by_item
        .keys()
        .chain(previous.items.keys())
        .cloned()
        .collect();
    let mut resources = Resources::new(root, target)?;
    let mut state = State {
        items: BTreeMap::new(),
        assets: previous.assets.clone(),
    };
    let mut outputs = BTreeMap::new();
    let mut generated = vec![];
    let mut count = 0;
    for item in items {
        let (namespace, name) = id(&item)?;
        let path = root.join(format!("assets/{namespace}/models/item/{name}.json"));
        let mut model = if path.pack_is_file() {
            match crate::model_json::parse(&fs::read(&path)?) {
                Ok(value) if value.is_object() => value,
                _ => {
                    report
                        .warnings
                        .push(format!("CIT 旧版物品适配：{item} 模型无法解析，保留原文"));
                    continue;
                }
            }
        } else {
            super::super::item_bindings::reference(&name).cloned().unwrap_or_else(||json!({"parent":"minecraft:item/generated","textures":{"layer0":format!("{namespace}:item/{name}")}}))
        };
        if let Some(saved) = previous.items.get(&item) {
            if model == saved.output {
                model = saved.base.clone();
            } else {
                report.warnings.push(format!(
                    "CIT 旧版物品适配：{item} 模型被手动修改，保留现有内容"
                ));
                state.items.insert(item, saved.clone());
                continue;
            }
        }
        let original = model.clone();
        let matching = by_item.get(&item).cloned().unwrap_or_default();
        let default = matching.iter().find(|rule| rule.cmd.is_none()).copied();
        let mut base = None;
        if let Some(rule) = default.filter(|rule| rule.unsupported.is_none()) {
            match resources.appearance(&rule.source, &rule.values, &item) {
                Ok(value) => {
                    base = value["model"].as_str().map(str::to_string);
                    count += 1;
                }
                Err(error) => {
                    notes.insert(rule.origin.clone(), format!("资源未接入：{error:#}"));
                }
            }
        }
        let mut cases = BTreeMap::new();
        let codes: BTreeSet<_> = matching.iter().filter_map(|rule| rule.cmd).collect();
        for code in codes {
            let Some(rule) = matching
                .iter()
                .find(|rule| rule.cmd.is_none() || rule.cmd == Some(code))
                .copied()
            else {
                continue;
            };
            if rule.cmd.is_none() || rule.unsupported.is_some() {
                continue;
            }
            if code == 0 {
                notes.insert(rule.origin.clone(),"此目标的原版模型无法区分缺少 CMD 与 CMD=0，保留为 CIT 条件，避免影响所有普通物品".into());
                continue;
            }
            if code.unsigned_abs() >= 16_777_216 {
                notes.insert(
                    rule.origin.clone(),
                    "此旧版模型属性无法精确区分相邻的大 CMD 整数，保留为 CIT 规则".into(),
                );
                continue;
            }
            match resources.appearance(&rule.source, &rule.values, &item) {
                Ok(value) => {
                    if let Some(reference) = value["model"].as_str() {
                        cases.insert(code, reference.to_string());
                        count += 1;
                    }
                }
                Err(error) => {
                    notes.insert(rule.origin.clone(), format!("资源未接入：{error:#}"));
                }
            }
        }
        if base.is_none() && cases.is_empty() {
            if previous.items.contains_key(&item) {
                outputs.insert(path, original);
            }
            continue;
        }
        let original_overrides = if base.is_none() {
            model["overrides"].as_array().cloned().unwrap_or_default()
        } else {
            vec![]
        };
        if original_overrides.iter().any(|entry| {
            entry["predicate"]
                .as_object()
                .is_none_or(|values| values.values().any(|value| !value.is_number()))
        }) {
            report.warnings.push(format!(
                "CIT 旧版物品适配：{item} 的原覆盖条件无法合并，保留原模型"
            ));
            continue;
        }
        if let Some(reference) = &base {
            model = json!({"parent":reference});
        }
        let mut fallback = model.clone();
        fallback.as_object_mut().unwrap().remove("overrides");
        let fallback_id = format!(
            "{namespace}:{PREFIX}{}",
            hash(format!("legacy-base:{item}").as_bytes())
        );
        resources.outputs.insert(
            model_path(root, &fallback_id),
            serde_json::to_vec(&fallback)?,
        );
        let mut overrides = original_overrides.clone();
        for (code, reference) in cases {
            overrides.push(json!({"predicate":{"custom_model_data":code},"model":reference}));
            let upper = f64::from(code) + 1.0;
            overrides.push(json!({"predicate":{"custom_model_data":upper},"model":fallback_id}));
            for entry in &original_overrides {
                let mut entry = entry.clone();
                let old = entry["predicate"]["custom_model_data"]
                    .as_f64()
                    .unwrap_or(f64::NEG_INFINITY);
                entry["predicate"]["custom_model_data"] = json!(old.max(upper));
                overrides.push(entry);
            }
        }
        if !overrides.is_empty() {
            model["overrides"] = json!(overrides);
        }
        state.items.insert(
            item,
            Saved {
                base: original,
                output: model.clone(),
            },
        );
        generated.push(model.clone());
        outputs.insert(path, model);
    }
    let mut reachable = BTreeSet::new();
    for model in &generated {
        resources.reachable(model, &mut reachable);
    }
    for path in &reachable {
        let relative = path
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        ensure!(
            !path.pack_is_file()
                || fs::read(path)? == resources.outputs[path]
                || previous.assets.contains(&relative),
            "CIT 目标已有不同内容，保留原文件：{relative}"
        );
    }
    for path in reachable {
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write_atomic(&path, &resources.outputs[&path])?;
        state.assets.insert(
            path.strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/"),
        );
    }
    for (path, value) in outputs {
        if path.pack_is_file() && crate::model_json::parse(&fs::read(&path)?)? == value {
            continue;
        }
        if path.pack_is_file() {
            backup(root, &path)?;
        }
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write_atomic(&path, &serde_json::to_vec(&value)?)?;
    }
    if !state.items.is_empty() || cache.pack_is_file() {
        fs::create_dir_all(cache.parent().unwrap())?;
        fs::write_atomic(&cache, &serde_json::to_vec(&state)?)?;
    }
    if count > 0 {
        report.converted.push(format!("CIT 旧版原版物品适配：为 {version} 接入 {count} 条规则，合并 CMD 精确区间并保留原版拉弓／损伤等覆盖分支；原 CIT 保留"));
    }
    warnings(&notes, report);
    Ok(())
}

fn model_path(root: &Path, reference: &str) -> PathBuf {
    let (namespace, name) = reference.split_once(':').unwrap();
    root.join(format!("assets/{namespace}/models/{name}.json"))
}
