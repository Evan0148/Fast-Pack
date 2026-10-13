use super::assets::{AssetReport, open, save};
use anyhow::Result;
use image::RgbaImage;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

fn id(reference: &str) -> Option<String> {
    let (namespace, path) = reference
        .split_once(':')
        .unwrap_or(("minecraft", reference));
    if namespace.is_empty()
        || path.is_empty()
        || !namespace
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c))
        || !path
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-/".contains(c))
        || path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return None;
    }
    Some(format!("{namespace}:{path}"))
}

fn path(root: &Path, reference: &str, folder: &str, extension: &str) -> Option<PathBuf> {
    let reference = id(reference)?;
    let (namespace, name) = reference.split_once(':')?;
    Some(root.join(format!("assets/{namespace}/{folder}/{name}.{extension}")))
}

fn model(root: &Path, reference: &str, target: u32) -> Option<Value> {
    let reference = id(reference)?;
    let location = path(root, &reference, "models", "json")?;
    if location.is_file() {
        return serde_json::from_slice(&fs::read(location).ok()?).ok();
    }
    super::modern_items::model_reference(target, &reference).or_else(|| {
        reference
            .strip_prefix("minecraft:item/")
            .and_then(super::item_bindings::reference)
            .cloned()
    })
}

fn textures(
    root: &Path,
    reference: &str,
    target: u32,
    visited: &mut BTreeSet<String>,
) -> BTreeMap<String, String> {
    let Some(reference) = id(reference) else {
        return BTreeMap::new();
    };
    if visited.len() >= 64 || !visited.insert(reference.clone()) {
        return BTreeMap::new();
    }
    let Some(value) = model(root, &reference, target) else {
        return BTreeMap::new();
    };
    let mut result = value
        .get("parent")
        .and_then(Value::as_str)
        .map(|parent| textures(root, parent, target, visited))
        .unwrap_or_default();
    if let Some(values) = value.get("textures").and_then(Value::as_object) {
        for (key, value) in values {
            if let Some(value) = value.as_str() {
                result.insert(key.clone(), value.into());
            }
        }
    }
    result
}

fn main_texture(root: &Path, reference: &str, name: &str, target: u32) -> Option<String> {
    let values = textures(root, reference, target, &mut BTreeSet::new());
    for key in ["layer0", "blade", "head", "body", "all", "particle"]
        .into_iter()
        .chain(values.keys().map(String::as_str))
    {
        let Some(mut value) = values.get(key).map(String::as_str) else {
            continue;
        };
        let mut seen = BTreeSet::new();
        while let Some(key) = value.strip_prefix('#') {
            if !seen.insert(key) {
                break;
            }
            let Some(next) = values.get(key) else {
                break;
            };
            value = next;
        }
        if let Some(reference) = id(value)
            && !reference.contains(":trims/")
            && path(root, &reference, "textures", "png").is_some_and(|path| path.is_file())
        {
            return Some(reference);
        }
    }
    let reference = format!("minecraft:item/{name}");
    path(root, &reference, "textures", "png")
        .filter(|path| path.is_file())
        .map(|_| reference)
}

fn color([r, g, b]: [u8; 3]) -> (f64, f64, f64) {
    let [r, g, b] = [r, g, b].map(f64::from);
    let maximum = r.max(g).max(b);
    let minimum = r.min(g).min(b);
    let delta = maximum - minimum;
    let hue = if delta == 0.0 {
        0.0
    } else if maximum == r {
        60.0 * ((g - b) / delta).rem_euclid(6.0)
    } else if maximum == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    (
        hue,
        if maximum == 0.0 { 0.0 } else { delta / maximum },
        0.2126 * r + 0.7152 * g + 0.0722 * b,
    )
}

fn warm(hue: f64, saturation: f64) -> bool {
    (15.0..=68.0).contains(&hue) && saturation > 0.22
}

fn recolor(mut image: RgbaImage) -> RgbaImage {
    let mut histogram = [0u64; 24];
    let mut colored = 0;
    let mut neutral = 0;
    let mut warm_count = 0;
    for pixel in image.pixels().filter(|pixel| pixel[3] >= 128) {
        let (hue, saturation, luminance) = color([pixel[0], pixel[1], pixel[2]]);
        if luminance < 20.0 {
            continue;
        }
        if saturation < 0.22 {
            neutral += 1;
        } else if warm(hue, saturation) {
            warm_count += 1;
        } else {
            histogram[(hue / 15.0) as usize % 24] += 1;
            colored += 1;
        }
    }
    let dominant = (0..24)
        .max_by_key(|index| {
            histogram[*index] + histogram[(*index + 1) % 24] + histogram[(*index + 23) % 24]
        })
        .unwrap();
    let dominant = dominant as f64 * 15.0 + 7.5;
    let keep_warm = colored + neutral > warm_count / 3 && colored + neutral > 0;
    let metal = |pixel: &image::Rgba<u8>| {
        let (hue, saturation, luminance) = color([pixel[0], pixel[1], pixel[2]]);
        let distance = (hue - dominant).abs().min(360.0 - (hue - dominant).abs());
        pixel[3] > 2
            && luminance >= 20.0
            && (!keep_warm || !warm(hue, saturation))
            && (saturation < 0.22 || distance <= 48.0 || colored == 0)
    };
    let mut levels: Vec<_> = image
        .pixels()
        .filter(|pixel| pixel[3] >= 128 && metal(pixel))
        .map(|pixel| color([pixel[0], pixel[1], pixel[2]]).2)
        .collect();
    if levels.is_empty() {
        return image;
    }
    levels.sort_by(f64::total_cmp);
    let low = levels[(levels.len() - 1) / 20];
    let high = levels[(levels.len() - 1) * 19 / 20];
    let palette: [[f64; 3]; 7] = [
        [18.0, 14.0, 15.0],
        [28.0, 24.0, 26.0],
        [40.0, 34.0, 37.0],
        [54.0, 46.0, 51.0],
        [61.0, 55.0, 60.0],
        [75.0, 66.0, 75.0],
        [96.0, 84.0, 96.0],
    ];
    for pixel in image.pixels_mut() {
        if !metal(pixel) {
            continue;
        }
        let luminance = color([pixel[0], pixel[1], pixel[2]]).2;
        let brightness = if high - low >= 8.0 {
            ((luminance - low) / (high - low)).clamp(0.0, 1.0)
        } else {
            (luminance / 255.0).clamp(0.0, 1.0)
        };
        let position = brightness * (palette.len() - 1) as f64;
        let index = (position.floor() as usize).min(palette.len() - 2);
        let fraction = position - index as f64;
        for channel in 0..3 {
            pixel[channel] = (palette[index][channel] * (1.0 - fraction)
                + palette[index + 1][channel] * fraction)
                .round() as u8;
        }
    }
    image
}

fn metadata(input: &Path, output: &Path) -> Result<()> {
    let input = input.with_extension("png.mcmeta");
    let output = output.with_extension("png.mcmeta");
    if input.is_file() && !output.is_file() {
        fs::copy(input, output)?;
    }
    Ok(())
}

struct ClonePack<'a> {
    root: &'a Path,
    target: u32,
    kind: &'a str,
    primary: String,
    texture: String,
    models: BTreeMap<String, String>,
    images: BTreeMap<String, String>,
}

impl ClonePack<'_> {
    fn texture(&mut self, reference: &str, label: &str) -> Result<String> {
        let Some(reference) = id(reference) else {
            return Ok(reference.into());
        };
        if reference.contains(":trims/items/") {
            let reference = reference.replace("_darker", "");
            return Ok(if reference.ends_with("_netherite") {
                format!("{reference}_darker")
            } else {
                reference
            });
        }
        if reference != self.primary
            && ["handle", "grip", "hilt", "wood"].iter().any(|word| {
                label.contains(word) || reference.rsplit('/').next().unwrap_or("").contains(word)
            })
        {
            return Ok(reference);
        }
        if let Some(mapped) = self.images.get(&reference) {
            return Ok(mapped.clone());
        }
        let input = path(self.root, &reference, "textures", "png").unwrap();
        if !input.is_file() {
            return Ok(reference);
        }
        let destination = if reference == self.primary {
            self.texture.clone()
        } else {
            let (namespace, name) = reference.split_once(':').unwrap();
            format!(
                "minecraft:item/fastpack/netherite/{}/{namespace}/{name}",
                self.kind
            )
        };
        let output = path(self.root, &destination, "textures", "png").unwrap();
        if !output.is_file() {
            save(self.root, &output, &recolor(open(&input)?))?;
            metadata(&input, &output)?;
        }
        self.images.insert(reference, destination.clone());
        Ok(destination)
    }

    fn rewrite(&mut self, value: &mut Value) -> Result<()> {
        match value {
            Value::Object(values) => {
                for (key, value) in values {
                    if key == "tints"
                        && let Some(tints) = value.as_array_mut()
                    {
                        tints.retain(|tint| {
                            !matches!(
                                tint.get("type").and_then(Value::as_str),
                                Some("minecraft:dyed_color" | "dyed_color")
                            )
                        });
                    } else if key == "textures"
                        && let Some(textures) = value.as_object_mut()
                    {
                        for (label, value) in textures {
                            if let Some(reference) = value.as_str() {
                                *value = json!(self.texture(reference, label)?);
                            }
                        }
                    } else if key == "texture"
                        && let Some(reference) = value.as_str()
                    {
                        *value = json!(self.texture(reference, "face")?);
                    } else if matches!(key.as_str(), "parent" | "model" | "base")
                        && let Some(reference) = value.as_str()
                    {
                        if let Some(mapped) = self.model(reference, None)? {
                            *value = json!(mapped);
                        }
                    } else {
                        self.rewrite(value)?;
                    }
                }
            }
            Value::Array(values) => {
                for value in values {
                    self.rewrite(value)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn model(&mut self, reference: &str, destination: Option<String>) -> Result<Option<String>> {
        let Some(reference) = id(reference) else {
            return Ok(None);
        };
        if let Some(mapped) = self.models.get(&reference) {
            return Ok(Some(mapped.clone()));
        }
        if self.models.len() >= 256 {
            return Ok(None);
        }
        let Some(mut value) = model(self.root, &reference, self.target) else {
            return Ok(None);
        };
        let destination = destination.unwrap_or_else(|| {
            let (namespace, name) = reference.split_once(':').unwrap();
            format!(
                "minecraft:fastpack/netherite/{}/{namespace}/{name}",
                self.kind
            )
        });
        self.models.insert(reference, destination.clone());
        if self.target < 15
            && let Some(overrides) = value.get_mut("overrides").and_then(Value::as_array_mut)
        {
            overrides.retain(|entry| {
                entry
                    .get("predicate")
                    .and_then(|predicate| predicate.get("trim_type"))
                    .is_none()
            });
        }
        self.rewrite(&mut value)?;
        let output = path(self.root, &destination, "models", "json").unwrap();
        if !output.is_file() {
            fs::create_dir_all(output.parent().unwrap())?;
            fs::write(output, serde_json::to_vec_pretty(&value)?)?;
        }
        Ok(Some(destination))
    }
}

pub(super) fn migrate(root: &Path, target: u32, report: &mut AssetReport) -> Result<()> {
    if target < 6 {
        return Ok(());
    }
    let mut items = 0;
    let mut armor = 0;
    for kind in [
        "sword",
        "axe",
        "pickaxe",
        "shovel",
        "hoe",
        "helmet",
        "chestplate",
        "leggings",
        "boots",
    ] {
        let destination = format!("netherite_{kind}");
        let output = root.join(format!("assets/minecraft/textures/item/{destination}.png"));
        let output_model = root.join(format!("assets/minecraft/models/item/{destination}.json"));
        let output_definition = root.join(format!("assets/minecraft/items/{destination}.json"));
        if output.is_file() || output_model.is_file() || output_definition.is_file() {
            continue;
        }
        let armor_item = matches!(kind, "helmet" | "chestplate" | "leggings" | "boots");
        let materials = if armor_item {
            ["diamond", "iron", "golden", "chainmail", "leather"]
        } else {
            ["diamond", "iron", "golden", "stone", "wooden"]
        };
        let candidate = materials.into_iter().find_map(|material| {
            let name = format!("{material}_{kind}");
            let definition = root.join(format!("assets/minecraft/items/{name}.json"));
            let provided = fs::read(definition)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
            let reference = provided
                .as_ref()
                .and_then(|value| value.get("model"))
                .and_then(super::item_display::definition_model)
                .map(str::to_string)
                .unwrap_or_else(|| format!("minecraft:item/{name}"));
            main_texture(root, &reference, &name, target)
                .map(|texture| (name, reference, texture, provided))
        });
        let Some((name, reference, texture, provided)) = candidate else {
            continue;
        };
        let mut pack = ClonePack {
            root,
            target,
            kind,
            primary: texture.clone(),
            texture: format!("minecraft:item/{destination}"),
            models: BTreeMap::new(),
            images: BTreeMap::new(),
        };
        pack.texture(&texture, "layer0")?;
        if pack
            .model(&reference, Some(format!("minecraft:item/{destination}")))?
            .is_none()
        {
            fs::create_dir_all(output_model.parent().unwrap())?;
            fs::write(
                &output_model,
                serde_json::to_vec_pretty(
                    &json!({"parent":if armor_item { "minecraft:item/generated" } else { "minecraft:item/handheld" },"textures":{"layer0":format!("minecraft:item/{destination}")}}),
                )?,
            )?;
            pack.models.insert(
                id(&reference).unwrap(),
                format!("minecraft:item/{destination}"),
            );
        }
        if target >= 46 {
            let mut definition = provided
                .or_else(|| super::modern_items::definition_reference(target, &name))
                .unwrap_or_else(|| json!({"model":{"type":"minecraft:model","model":reference}}));
            pack.rewrite(&mut definition)?;
            fs::create_dir_all(output_definition.parent().unwrap())?;
            fs::write(output_definition, serde_json::to_vec_pretty(&definition)?)?;
        }
        report.generated.push(format!(
            "下界合金{kind}：从 {name} 自动配色，保留原图轮廓、动画与模型姿态"
        ));
        items += 1;
    }
    for (layer, directory) in [(1, "humanoid"), (2, "humanoid_leggings")] {
        let output = root.join(if target >= 42 {
            format!("assets/minecraft/textures/entity/equipment/{directory}/netherite.png")
        } else {
            format!("assets/minecraft/textures/models/armor/netherite_layer_{layer}.png")
        });
        if output.is_file() {
            continue;
        }
        let source = ["diamond", "iron", "gold", "chainmail", "leather"]
            .into_iter()
            .flat_map(|material| {
                [
                    root.join(format!(
                        "assets/minecraft/textures/entity/equipment/{directory}/{material}.png"
                    )),
                    root.join(format!(
                        "assets/minecraft/textures/models/armor/{material}_layer_{layer}.png"
                    )),
                ]
            })
            .find(|path| path.is_file());
        if let Some(source) = source {
            save(root, &output, &recolor(open(&source)?))?;
            metadata(&source, &output)?;
            armor += 1;
        }
    }
    if items + armor > 0 {
        report.generated.push(format!("下界合金套装：自动补齐 {items} 件武器、工具和装备图标及 {armor} 张穿戴纹理，已有下界合金资源优先保留"));
    }
    Ok(())
}
