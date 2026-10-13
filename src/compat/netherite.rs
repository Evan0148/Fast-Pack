use super::assets::{AssetReport, open, save};
use anyhow::Result;
use image::RgbaImage;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
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

fn ramp(palette: &[[f64; 3]], value: f64) -> [u8; 3] {
    let position = value.clamp(0.0, 1.0) * (palette.len() - 1) as f64;
    let index = (position.floor() as usize).min(palette.len() - 2);
    let fraction = position - index as f64;
    std::array::from_fn(|channel| {
        (palette[index][channel] * (1.0 - fraction) + palette[index + 1][channel] * fraction)
            .round() as u8
    })
}

fn recolor(mut image: RgbaImage, armor: bool, grip: bool) -> RgbaImage {
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
    if levels.is_empty() && !grip {
        return image;
    }
    levels.sort_by(f64::total_cmp);
    let low = levels
        .get(levels.len().saturating_sub(1) / 20)
        .copied()
        .unwrap_or(0.0);
    let high = levels
        .get(levels.len().saturating_sub(1) * 19 / 20)
        .copied()
        .unwrap_or(255.0);
    let palette = [
        [35.0, 16.0, 18.0],
        [50.0, 39.0, 39.0],
        [79.0, 60.0, 62.0],
        [81.0, 68.0, 78.0],
        [93.0, 86.0, 93.0],
        [112.0, 103.0, 112.0],
        [134.0, 123.0, 134.0],
    ];
    let handle_palette = [
        [22.0, 8.0, 20.0],
        [35.0, 16.0, 18.0],
        [47.0, 33.0, 34.0],
        [79.0, 60.0, 62.0],
        [96.0, 52.0, 50.0],
        [115.0, 69.0, 67.0],
    ];
    let original = image.clone();
    let width = image.width();
    let height = image.height();
    let step = (width / if armor { 64 } else { 16 }).max(1) as i32;
    let frame_height = if armor { height } else { width.min(height) };
    let index = |x: u32, y: u32| (y * width + x) as usize;
    let mut distances = vec![u32::MAX; width as usize * height as usize];
    let mut queue = VecDeque::new();
    for (x, y, pixel) in original.enumerate_pixels() {
        let (_, saturation, _) = color([pixel[0], pixel[1], pixel[2]]);
        if metal(pixel) && saturation >= 0.22 {
            distances[index(x, y)] = 0;
            queue.push_back((x, y));
        }
    }
    while let Some((x, y)) = queue.pop_front() {
        let distance = distances[index(x, y)] + 1;
        if distance > 3 * step as u32 {
            continue;
        }
        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            let (nx, ny) = (x as i32 + dx, y as i32 + dy);
            if nx < 0
                || nx >= width as i32
                || ny < 0
                || ny >= height as i32
                || y / frame_height != ny as u32 / frame_height
            {
                continue;
            }
            let target = index(nx as u32, ny as u32);
            if distances[target] > distance {
                distances[target] = distance;
                queue.push_back((nx as u32, ny as u32));
            }
        }
    }
    let material = |x: i32, y: i32, frame: u32| -> bool {
        if x < 0
            || x >= width as i32
            || y < 0
            || y >= height as i32
            || y as u32 / frame_height != frame
        {
            return false;
        }
        let pixel = original.get_pixel(x as u32, y as u32);
        let (_, saturation, luminance) = color([pixel[0], pixel[1], pixel[2]]);
        metal(pixel)
            && (armor
                || colored == 0
                || saturation >= 0.22
                || luminance >= 90.0
                || distances[index(x as u32, y as u32)] <= 2 * step as u32)
    };
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        if pixel[3] <= 2 {
            continue;
        }
        let (hue, saturation, luminance) = color([pixel[0], pixel[1], pixel[2]]);
        let brightness = if high - low >= 8.0 {
            ((luminance - low) / (high - low)).clamp(0.0, 1.0)
        } else {
            (luminance / 255.0).clamp(0.0, 1.0)
        };
        let frame = y / frame_height;
        let px = x as i32;
        let py = y as i32;
        let is_metal = material(px, py, frame) && !grip;
        let upper = !material(px - step, py, frame) || !material(px, py - step, frame);
        let lower = !material(px + step, py, frame) || !material(px, py + step, frame);
        let highlight =
            !upper && (!material(px - 2 * step, py, frame) || !material(px, py - 2 * step, frame));
        let mixed = |left: [u8; 3], right: [u8; 3], weight: f64| {
            std::array::from_fn(|channel| {
                (f64::from(left[channel]) * (1.0 - weight) + f64::from(right[channel]) * weight)
                    .round() as u8
            })
        };
        let rgb = if is_metal {
            let base = ramp(&palette, 0.12 + brightness * 0.83);
            if upper && !lower {
                mixed(base, [74, 41, 64], 0.78)
            } else if lower && !upper {
                mixed(base, [35, 16, 18], 0.7)
            } else if highlight {
                mixed(base, [134, 123, 134], 0.48)
            } else {
                let band = ((x / step as u32 + (y % frame_height) / step as u32) % 5) as f64 / 4.0;
                mixed(
                    base,
                    ramp(&[[74.0, 41.0, 64.0], [79.0, 60.0, 62.0]], band),
                    0.13,
                )
            }
        } else if !armor && (grip || warm(hue, saturation) || saturation < 0.22) {
            if luminance < 20.0
                && (material(px + step, py, frame) || material(px, py + step, frame))
            {
                [59, 30, 50]
            } else if luminance < 12.0 && !grip {
                [pixel[0], pixel[1], pixel[2]]
            } else {
                let band = (x / step as u32 + (y % frame_height) / step as u32) % 4;
                let detail = if band == 0 { 0.22 } else { 0.0 };
                ramp(
                    &handle_palette,
                    (luminance / 210.0 * 0.68 + detail).clamp(0.0, 0.95),
                )
            }
        } else {
            [pixel[0], pixel[1], pixel[2]]
        };
        for channel in 0..3 {
            pixel[channel] = rgb[channel];
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
        let grip = reference != self.primary
            && ["handle", "grip", "hilt", "wood"].iter().any(|word| {
                label.contains(word) || reference.rsplit('/').next().unwrap_or("").contains(word)
            });
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
            save(self.root, &output, &recolor(open(&input)?, false, grip))?;
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
            save(root, &output, &recolor(open(&source)?, true, false))?;
            metadata(&source, &output)?;
            armor += 1;
        }
    }
    if items + armor > 0 {
        report.generated.push(format!("下界合金套装：自动补齐 {items} 件武器、工具和装备图标及 {armor} 张穿戴纹理，已有下界合金资源优先保留"));
    }
    Ok(())
}
