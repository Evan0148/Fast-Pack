use super::*;

fn binding(textures: &serde_json::Map<String, Value>, reference: &str) -> Result<String> {
    let mut current = reference;
    let mut seen = BTreeSet::new();
    while let Some(slot) = current.strip_prefix('#') {
        ensure!(seen.insert(slot) && seen.len() < 32, "贴图槽循环");
        current = textures
            .get(slot)
            .and_then(Value::as_str)
            .context("模型缺少贴图槽")?;
    }
    resource(current)?;
    Ok(current.into())
}

fn model(root: &Path, target: u32, reference: &str) -> Result<Value> {
    super::super::custom_items::cit_model(root, target, reference).or_else(|error| {
        if reference.starts_with("minecraft:item/") && reference.ends_with("_trim") {
            let base = reference.strip_suffix("_trim").unwrap();
            super::super::custom_items::cit_model(root, target, &format!("{base}_darker_trim"))
                .or(Err(error))
        } else {
            Err(error)
        }
    })
}

pub(super) fn compose(
    root: &Path,
    target: u32,
    item: &str,
    parts: &[Value],
    outputs: &mut BTreeMap<String, Vec<u8>>,
) -> Result<Value> {
    for (index, part) in parts.iter().enumerate() {
        if let Some(tints) = part["tints"].as_array() {
            ensure!(
                (tints.is_empty() || index == 0 && tints.len() == 1)
                    && tints.iter().all(|tint| {
                        (tint["type"] == "minecraft:dye" && item.starts_with("minecraft:leather_"))
                            || (tint["type"] == "minecraft:potion"
                                && matches!(
                                    item,
                                    "minecraft:potion"
                                        | "minecraft:splash_potion"
                                        | "minecraft:lingering_potion"
                                        | "minecraft:tipped_arrow"
                                ))
                    }),
                "此原版染色来源无法用旧版物品染色等价表达，保留原定义"
            );
        }
    }
    if parts.len() == 1
        && !matches!(
            item,
            "minecraft:potion"
                | "minecraft:splash_potion"
                | "minecraft:lingering_potion"
                | "minecraft:tipped_arrow"
        )
    {
        let reference = parts[0]["model"].as_str().context("模型节点缺少 model")?;
        let model = model(root, target, reference)?;
        if model.get("elements").is_some() {
            if let Some(textures) = model["textures"].as_object() {
                for value in textures.values().filter_map(Value::as_str) {
                    texture(root, target, textures, value)?;
                }
            }
            return Ok(model);
        }
    }
    let mut textures = serde_json::Map::new();
    let mut display = json!({});
    let mut parent = "minecraft:item/generated".to_string();
    let mut layer = 0;
    for (part_index, part) in parts.iter().enumerate() {
        let reference = part["model"].as_str().context("模型节点缺少 model")?;
        resource(reference)?;
        let model = model(root, target, reference)?;
        ensure!(
            model.get("elements").is_none() && model.get("loader").is_none(),
            "多个三维几何模型不能无损拼接为单个旧版物品模型"
        );
        if part_index == 0 {
            display = model["display"].clone();
            if let Some(value) = model["parent"].as_str() {
                parent = value.into();
            }
        }
        let source = model["textures"].as_object().context("缺少平面物品贴图")?;
        // A native model without tint sources must stay untinted when rendered by
        // old potion/tipped-arrow item color providers (which color layer 0).
        let untinted_potion = part_index == 0
            && matches!(
                item,
                "minecraft:potion"
                    | "minecraft:splash_potion"
                    | "minecraft:lingering_potion"
                    | "minecraft:tipped_arrow"
            )
            && part["tints"].as_array().is_none_or(Vec::is_empty);
        if untinted_potion {
            textures.insert("layer0".into(), json!(clear(outputs)?));
            layer = 1;
        }
        let mut found = false;
        for index in 0..5 {
            if let Some(value) = source.get(&format!("layer{index}")).and_then(Value::as_str) {
                ensure!(layer < 5, "叠加图层超过旧版的五层上限");
                textures.insert(
                    format!("layer{layer}"),
                    json!(texture(root, target, source, value)?),
                );
                layer += 1;
                found = true;
            }
        }
        ensure!(found, "模型没有 generated 图层：{reference}");
    }
    if parts.is_empty() {
        textures.insert("layer0".into(), json!(clear(outputs)?));
    }
    Ok(json!({"parent":parent,"textures":textures,"display":display}))
}

fn texture(
    root: &Path,
    target: u32,
    textures: &serde_json::Map<String, Value>,
    value: &str,
) -> Result<String> {
    let reference = binding(textures, value)?;
    let (namespace, name) = resource(&reference)?;
    ensure!(
        root.join(format!("assets/{namespace}/textures/{name}.png"))
            .pack_is_file()
            || super::super::model_integrity::native_texture(target, &reference),
        "目标版本缺少贴图 {reference}"
    );
    Ok(reference)
}

fn clear(outputs: &mut BTreeMap<String, Vec<u8>>) -> Result<&'static str> {
    const PATH: &str = "assets/minecraft/textures/item/fastpack_native_cit/transparent.png";
    if !outputs.contains_key(PATH) {
        // One 1x1 sprite reduces the mip level of the entire block/item atlas.
        let image = image::RgbaImage::from_pixel(16, 16, image::Rgba([0; 4]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png)?;
        outputs.insert(PATH.into(), bytes.into_inner());
    }
    Ok("minecraft:item/fastpack_native_cit/transparent")
}
