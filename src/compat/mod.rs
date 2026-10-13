pub mod assets;
pub mod atlas;
mod beds;
mod effect_panels;
mod equipment;
pub mod glint;
mod item_bindings;
mod item_display;
mod item_transparency;
mod legacy_blocks;
pub(crate) mod legacy_defaults;
mod legacy_effect_icons;
mod legacy_gui;
mod modern_gui;
mod modern_items;
mod netherite;
mod players;
pub mod properties;
mod recent_resources;
pub mod shaders;
pub mod sky;
mod texture_formats;

pub(crate) fn migrate_versioned_assets(
    root: &std::path::Path,
    target: u32,
    version: &str,
    report: &mut assets::AssetReport,
) -> anyhow::Result<()> {
    beds::migrate(root, target, version, report)
}

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;
use walkdir::WalkDir;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum SkyRuntime {
    #[default]
    Auto,
    Optifine,
    Interop,
    FabricSkyboxes,
    Nuit,
    Vanilla,
}

impl SkyRuntime {
    pub const ALL: [Self; 6] = [
        Self::Auto,
        Self::Optifine,
        Self::Interop,
        Self::FabricSkyboxes,
        Self::Nuit,
        Self::Vanilla,
    ];
    pub fn resolved(self, format: u32) -> Self {
        if self == Self::Auto {
            if format >= 84 {
                Self::Nuit
            } else {
                Self::Optifine
            }
        } else {
            self
        }
    }
    pub fn requirement(self, format: u32) -> String {
        match self.resolved(format) {
            Self::Optifine => {
                "输出 OptiFine/MCPatcher 天空格式，适用于已有兼容天空渲染能力的客户端".into()
            }
            Self::Interop if format >= 84 => {
                "复杂天空规则以原配置保留，适用于已有 Nuit Interop 能力的客户端".into()
            }
            Self::Interop => "复杂天空规则以原配置保留，适用于已有 FSB Interop 能力的客户端".into(),
            Self::FabricSkyboxes => {
                "输出 FabricSkyBoxes schema 2，适用于已有对应天空渲染能力的客户端".into()
            }
            Self::Nuit => "输出 Nuit schema 1，适用于已有对应天空渲染能力的客户端".into(),
            Self::Vanilla => {
                "原版客户端本身不加载自定义天空；资源包文件无法新增客户端渲染功能".into()
            }
            Self::Auto => unreachable!(),
        }
    }
}

impl fmt::Display for SkyRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Auto => "自动选择",
            Self::Optifine => "OptiFine",
            Self::Interop => "天空 Interop",
            Self::FabricSkyboxes => "FabricSkyBoxes 原生",
            Self::Nuit => "Nuit 原生",
            Self::Vanilla => "原版客户端",
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum ShaderPolicy {
    #[default]
    Adaptive,
    Preserve,
    Strict,
}

impl ShaderPolicy {
    pub const ALL: [Self; 3] = [Self::Adaptive, Self::Preserve, Self::Strict];
}

impl fmt::Display for ShaderPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Adaptive => "自适应（不兼容项备份停用）",
            Self::Preserve => "保留原着色器（可能不兼容）",
            Self::Strict => "严格检查（不兼容则停止）",
        })
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompatibilityReport {
    pub sky_layers: usize,
    pub sky_converted: Vec<String>,
    pub shader_files: usize,
    pub shader_adapted: Vec<String>,
    pub disabled_shaders: Vec<String>,
    pub requirements: Vec<String>,
    pub warnings: Vec<String>,
    pub selected_variant: Option<String>,
}

impl CompatibilityReport {
    pub fn is_empty(&self) -> bool {
        self.sky_layers == 0
            && self.shader_files == 0
            && self.warnings.is_empty()
            && self.selected_variant.is_none()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackFeatures {
    pub optifine_sky: usize,
    pub fabric_sky: usize,
    pub nuit_sky: usize,
    pub core_shaders: usize,
    pub post_shaders: usize,
    pub version_variants: Vec<String>,
}

impl PackFeatures {
    pub fn sky_count(&self) -> usize {
        self.optifine_sky + self.fabric_sky + self.nuit_sky
    }
    pub fn has_graphics(&self) -> bool {
        self.sky_count() + self.core_shaders + self.post_shaders > 0
    }
}

pub fn inspect(root: &Path) -> Result<PackFeatures> {
    let mut features = PackFeatures::default();
    for entry in WalkDir::new(root).min_depth(1) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry
            .path()
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        if path.starts_with("fastpack/") {
            continue;
        }
        if path.ends_with(".properties")
            && (path.contains("/mcpatcher/sky/") || path.contains("/optifine/sky/"))
        {
            features.optifine_sky += 1;
        }
        if path.ends_with(".json") && path.contains("/sky/") && path.starts_with("assets/") {
            if path.starts_with("assets/nuit/") {
                features.nuit_sky += 1;
            } else if !path.contains("/optifine/") && !path.contains("/mcpatcher/") {
                features.fabric_sky += 1;
            }
        }
        if path.contains("/shaders/core/") && shader_file(&path) {
            features.core_shaders += 1;
        }
        if (path.contains("/shaders/post/")
            || path.contains("/shaders/program/")
            || path.contains("/post_effect/"))
            && shader_file(&path)
        {
            features.post_shaders += 1;
        }
    }
    let variants = root.join("fastpack/variants");
    if variants.is_dir() {
        for entry in std::fs::read_dir(variants)? {
            let entry = entry?;
            if entry.path().is_dir() {
                features
                    .version_variants
                    .push(entry.file_name().to_string_lossy().into());
            }
        }
        features.version_variants.sort();
    }
    Ok(features)
}

pub fn shader_file(path: &str) -> bool {
    matches!(
        Path::new(path).extension().and_then(|ext| ext.to_str()),
        Some("fsh" | "vsh" | "gsh" | "csh" | "glsl" | "json" | "properties")
    )
}

pub fn migrate(
    root: &Path,
    source_format: u32,
    target_format: u32,
    target_version: &str,
    runtime: SkyRuntime,
    policy: ShaderPolicy,
) -> Result<CompatibilityReport> {
    let mut report = CompatibilityReport::default();
    shaders::select_variant(root, target_version, target_format, &mut report)?;
    let features = inspect(root)?;
    if features.sky_count() > 0 {
        sky::migrate(root, target_format, runtime, &mut report)?;
    }
    if features.core_shaders + features.post_shaders > 0 {
        shaders::migrate(root, source_format, target_format, policy, &mut report)?;
    }
    if !report.is_empty() {
        std::fs::create_dir_all(root.join("fastpack"))?;
        std::fs::write(
            root.join("fastpack/compatibility.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
    }
    Ok(report)
}
