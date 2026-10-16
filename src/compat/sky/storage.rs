use super::{AtlasLayout, CompatibilityReport, Layer, atlas, layout, local, properties, resolve};
use crate::packfs::PathExt;
use crate::packfs::WalkDir;
use crate::packfs::{self as fs, File};
use anyhow::{Context, Result};
use image::ImageEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufReader, Read};
use std::path::Path;

#[derive(Clone)]
pub(super) struct Texture {
    pub path: String,
    pub resized: Option<((u32, u32), (u32, u32))>,
}

pub(super) struct Storage {
    canonical: BTreeSet<String>,
    owned: BTreeSet<String>,
    outputs: BTreeMap<String, Texture>,
}

fn hash(path: &Path) -> Result<String> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let length = reader.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        digest.update(&buffer[..length]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub(super) fn identifier(path: &str) -> Result<String> {
    let (namespace, name) = path
        .strip_prefix("assets/")
        .and_then(|path| path.split_once('/'))
        .context("天空纹理缺少资源命名空间")?;
    Ok(format!("{namespace}:{name}"))
}

impl Storage {
    pub fn prepare(&mut self, root: &Path, layers: &[Layer], target: AtlasLayout) -> Result<()> {
        let mut unique = BTreeMap::new();
        for layer in layers {
            let key = format!(
                "{}:{target:?}:{}:{}:{}",
                layer.texture,
                layer.layout,
                layer.world == "world0",
                layer.decoration
            );
            unique.entry(key).or_insert_with(|| layer.clone());
        }
        let jobs: Vec<_> = unique.into_iter().collect();
        let outputs = crate::parallel::map(&jobs, |(key, layer)| {
            let mut temporary = Self {
                canonical: BTreeSet::new(),
                owned: BTreeSet::new(),
                outputs: BTreeMap::new(),
            };
            Ok(temporary
                .materialize(root, layer, target)
                .ok()
                .map(|texture| (key.clone(), texture, temporary.owned)))
        })?;
        for (key, texture, owned) in outputs.into_iter().flatten() {
            self.outputs.insert(key, texture);
            self.owned.extend(owned);
        }
        Ok(())
    }

    pub fn new(root: &Path, layers: &mut [Layer], previous: &[String]) -> Result<Self> {
        let mut owned = BTreeSet::new();
        for relative in previous {
            let file = local(root, relative)?;
            if !file.pack_is_file() {
                continue;
            }
            let stem = file.file_stem().unwrap_or_default().to_string_lossy();
            let texture = if relative.ends_with(".properties") {
                let values = properties::parse(&fs::read(&file)?)?;
                let source = values.get("source");
                if source.is_some_and(|source| *source == format!("./{stem}.png")) {
                    Some(file.with_extension("png"))
                } else if source
                    .is_some_and(|source| source.starts_with("fastpack:textures/sky/converted_"))
                {
                    resolve(root, &file, source.map(String::as_str), "", None).ok()
                } else {
                    None
                }
            } else if stem.starts_with("fastpack_layer_") {
                let value: serde_json::Value = serde_json::from_slice(&fs::read(&file)?)?;
                value["texture"]
                    .as_str()
                    .filter(|texture| texture.starts_with("fastpack:textures/sky/"))
                    .and_then(|texture| resolve(root, &file, Some(texture), "", None).ok())
            } else {
                None
            };
            if let Some(texture) = texture.filter(|path| path.pack_is_file()) {
                owned.insert(
                    texture
                        .strip_prefix(root)?
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
        let mut candidates = BTreeSet::new();
        for entry in WalkDir::new(root.join("assets")).min_depth(1) {
            let entry = entry?;
            let relative = entry
                .path()
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            if entry.file_type().is_file()
                && relative.contains("/sky/")
                && matches!(
                    entry.path().extension().and_then(|s| s.to_str()),
                    Some("png" | "jpg" | "jpeg")
                )
            {
                candidates.insert(relative);
            }
        }
        for layer in layers.iter() {
            if layer.texture.starts_with("assets/") {
                candidates.insert(layer.texture.clone());
            }
            let original = local(root, &layer.original)?;
            let backup = root.join("fastpack/original-sky").join(&layer.original);
            let file = if original.pack_is_file() {
                original.clone()
            } else {
                backup
            };
            if let Ok(bytes) = fs::read(file) {
                let source = if layer.original.ends_with(".properties") {
                    properties::parse(&bytes)
                        .ok()
                        .and_then(|p| p.get("source").cloned())
                } else {
                    serde_json::from_slice::<serde_json::Value>(&bytes)
                        .ok()
                        .and_then(|v| v["texture"].as_str().map(str::to_string))
                };
                if let Ok(path) = resolve(
                    root,
                    &original,
                    source.as_deref(),
                    if layer.original.contains("/mcpatcher/") {
                        "mcpatcher"
                    } else {
                        "optifine"
                    },
                    None,
                ) {
                    candidates.insert(
                        path.strip_prefix(root)?
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                }
            }
        }
        let mut indexed = BTreeMap::new();
        for relative in candidates {
            if local(root, &relative)?.pack_is_file() {
                let digest = hash(&local(root, &relative)?)?;
                let preference = (owned.contains(&relative), relative.len(), relative.clone());
                let current = indexed.entry(digest).or_insert(preference.clone());
                if preference < *current {
                    *current = preference;
                }
            }
        }
        let mut canonical = BTreeSet::new();
        for layer in layers {
            let source = local(root, &layer.texture)?;
            let digest = hash(&source)?;
            if layer.texture.starts_with("fastpack/canonical-sky/") {
                canonical.insert(layer.texture.clone());
            }
            if let Some((_, _, relative)) = indexed.get(&digest) {
                layer.texture = relative.clone();
            } else if !layer.texture.starts_with("assets/") {
                let extension = source.extension().and_then(|s| s.to_str()).unwrap_or("png");
                let relative = format!("assets/fastpack/textures/sky/source_{digest}.{extension}");
                let destination = local(root, &relative)?;
                fs::create_dir_all(destination.parent().unwrap())?;
                if !destination.pack_is_file() {
                    fs::copy(&source, &destination)?;
                }
                indexed.insert(digest, (false, relative.len(), relative.clone()));
                layer.texture = relative;
            }
        }
        Ok(Self {
            canonical,
            owned,
            outputs: BTreeMap::new(),
        })
    }

    pub fn materialize(
        &mut self,
        root: &Path,
        layer: &Layer,
        target: AtlasLayout,
    ) -> Result<Texture> {
        let key = format!(
            "{}:{target:?}:{}:{}:{}",
            layer.texture,
            layer.layout,
            layer.world == "world0",
            layer.decoration
        );
        if let Some(texture) = self.outputs.get(&key) {
            return Ok(texture.clone());
        }
        let source = local(root, &layer.texture)?;
        let reader = crate::packfs::image_reader(&source)?.with_guessed_format()?;
        let png = reader.format() == Some(image::ImageFormat::Png);
        let dimensions = reader.into_dimensions()?;
        let exact = if layer.decoration {
            true
        } else {
            let size = atlas::face_size(dimensions.0, dimensions.1)?;
            png && layout(&layer.layout) == target && dimensions == (size * 3, size * 2)
        };
        let texture = if exact {
            Texture {
                path: layer.texture.clone(),
                resized: None,
            }
        } else {
            let image = crate::compat::assets::open(&source)?;
            let converted = atlas::remap(
                &image,
                layout(&layer.layout),
                target,
                layer.world == "world0",
            )?;
            let relative = format!(
                "assets/fastpack/textures/sky/converted_{}_{:?}_{target:?}_{}.png",
                hash(&source)?,
                layout(&layer.layout),
                layer.world == "world0"
            )
            .to_lowercase();
            let destination = local(root, &relative)?;
            fs::create_dir_all(destination.parent().unwrap())?;
            PngEncoder::new_with_quality(
                File::create(destination)?,
                CompressionType::Default,
                FilterType::Adaptive,
            )
            .write_image(
                converted.as_raw(),
                converted.width(),
                converted.height(),
                image::ExtendedColorType::Rgba8,
            )?;
            self.owned.insert(relative.clone());
            Texture {
                path: relative,
                resized: (dimensions != converted.dimensions())
                    .then_some((dimensions, converted.dimensions())),
            }
        };
        self.outputs.insert(key, texture.clone());
        Ok(texture)
    }

    pub fn finish(
        self,
        root: &Path,
        layers: &[Layer],
        report: &mut CompatibilityReport,
    ) -> Result<()> {
        fs::write(
            root.join("fastpack/canonical-sky/layers.json"),
            serde_json::to_vec_pretty(layers)?,
        )?;
        let retained: BTreeSet<_> = layers.iter().map(|layer| layer.texture.as_str()).collect();
        let mut text = Vec::new();
        for entry in WalkDir::new(root.join("assets")).min_depth(1) {
            let entry = entry?;
            if entry.file_type().is_file()
                && matches!(
                    entry.path().extension().and_then(|s| s.to_str()),
                    Some("json" | "properties" | "mcmeta" | "fsh" | "vsh" | "glsl")
                )
                && let Ok(bytes) = fs::read(entry.path())
                && let Ok(value) = String::from_utf8(bytes)
            {
                text.push((entry.into_path(), value));
            }
        }
        let mut removed = 0;
        let mut bytes = 0;
        for relative in self.canonical.into_iter().chain(self.owned) {
            if retained.contains(relative.as_str()) {
                continue;
            }
            let path = local(root, &relative)?;
            if !path.pack_is_file() {
                continue;
            }
            if relative.starts_with("assets/") {
                let resource = identifier(&relative)?;
                let name = resource.split_once(':').unwrap().1.trim_end_matches(".png");
                let filename = path.file_name().unwrap().to_string_lossy();
                if text.iter().any(|(file, value)| {
                    value.contains(name)
                        || file.parent() == path.parent()
                            && (value.contains(&format!("\"{filename}\""))
                                || value.contains(&format!("\"./{filename}\""))
                                || value.lines().any(|line| {
                                    line.split_once('=').is_some_and(|(_, value)| {
                                        let value = value.trim();
                                        value == filename.as_ref()
                                            || value == format!("./{filename}")
                                    })
                                }))
                }) {
                    continue;
                }
            }
            bytes += fs::metadata(&path)?.len();
            fs::remove_file(path)?;
            removed += 1;
        }
        report.sky_converted.push(format!(
            "天空纹理共用：{} 组输出引用，清理 {removed} 个冗余副本，减少 {:.2} MiB",
            self.outputs.len(),
            bytes as f64 / 1_048_576.0
        ));
        Ok(())
    }
}
