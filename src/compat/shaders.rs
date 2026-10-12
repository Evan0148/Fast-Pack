use super::{CompatibilityReport, ShaderPolicy, shader_file};
use anyhow::{Result, bail, ensure};
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn generation(format: u32) -> &'static str {
    match format {
        0..=6 => "legacy-post",
        7..=54 => "classic-core",
        55..=62 => "pipeline-core",
        63..=87 => "uniform-blocks",
        _ => "glsl330-uniform-blocks",
    }
}

fn shader_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    for entry in WalkDir::new(root.join("assets")).min_depth(1) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path().to_string_lossy().replace('\\', "/");
        if (path.contains("/shaders/") || path.contains("/post_effect/")) && shader_file(&path) {
            result.push(entry.into_path());
        }
    }
    result.sort();
    Ok(result)
}

pub fn select_variant(
    root: &Path,
    target: &str,
    target_format: u32,
    report: &mut CompatibilityReport,
) -> Result<()> {
    ensure!(
        !target.is_empty()
            && !matches!(target, "." | "..")
            && target
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)),
        "版本变体名称不能包含路径"
    );
    let variant = root.join("fastpack/variants").join(target);
    if !variant.join("assets").is_dir() {
        let profile = root.join("fastpack/original-shaders/profile.json");
        if profile.exists() {
            let value: serde_json::Value = serde_json::from_slice(&fs::read(profile)?)?;
            if value
                .get("source_format")
                .and_then(serde_json::Value::as_u64)
                == Some(u64::from(target_format))
            {
                let cache = root.join("fastpack/original-shaders");
                let assets = cache.join("assets");
                if assets.is_dir() {
                    for entry in WalkDir::new(&assets).min_depth(1) {
                        let entry = entry?;
                        if entry.file_type().is_file() {
                            let path = root.join(entry.path().strip_prefix(&cache)?);
                            fs::create_dir_all(path.parent().unwrap())?;
                            fs::copy(entry.path(), path)?;
                        }
                    }
                    report.selected_variant = Some(target.into());
                    report
                        .shader_adapted
                        .push(format!("自动恢复 {target} 对应的原始材质包着色器"));
                }
            }
        }
        return Ok(());
    }
    let files: Vec<_> = WalkDir::new(variant.join("assets"))
        .min_depth(1)
        .into_iter()
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .collect();
    ensure!(!files.is_empty(), "目标版本变体为空: {target}");
    let active = if root.join("assets").exists() {
        shader_paths(root)?
    } else {
        Vec::new()
    };
    for path in active {
        let relative = path.strip_prefix(root)?;
        let backup = root.join("fastpack/original-shaders").join(relative);
        fs::create_dir_all(backup.parent().unwrap())?;
        if !backup.exists() {
            fs::copy(&path, backup)?;
        }
        fs::remove_file(path)?;
    }
    for path in files {
        let relative = path.strip_prefix(&variant)?;
        let target_path = root.join(relative);
        fs::create_dir_all(target_path.parent().unwrap())?;
        fs::copy(path, target_path)?;
    }
    report.selected_variant = Some(target.into());
    report
        .shader_adapted
        .push(format!("自动选择材质包内的 {target} 版本变体"));
    Ok(())
}

pub fn migrate(
    root: &Path,
    source: u32,
    target: u32,
    policy: ShaderPolicy,
    report: &mut CompatibilityReport,
) -> Result<()> {
    let paths = shader_paths(root)?;
    report.shader_files = paths.len();
    if paths.is_empty() {
        return Ok(());
    }
    let same = generation(source) == generation(target) || report.selected_variant.is_some();
    if !same {
        let message = format!(
            "材质包着色器跨代 {} → {}，未找到目标版本变体；不能证明任意 GLSL 与 uniform/渲染管线等价",
            generation(source),
            generation(target)
        );
        if policy == ShaderPolicy::Strict {
            bail!("{message}");
        }
        if policy == ShaderPolicy::Adaptive {
            let profile = root.join("fastpack/original-shaders/profile.json");
            if !profile.exists() {
                fs::create_dir_all(profile.parent().unwrap())?;
                fs::write(
                    profile,
                    serde_json::to_vec_pretty(&serde_json::json!({"source_format":source}))?,
                )?;
            }
            for path in paths {
                let relative = path.strip_prefix(root)?;
                let backup = root.join("fastpack/original-shaders").join(relative);
                fs::create_dir_all(backup.parent().unwrap())?;
                if !backup.exists() {
                    fs::copy(&path, backup)?;
                }
                fs::remove_file(&path)?;
                report
                    .disabled_shaders
                    .push(relative.to_string_lossy().replace('\\', "/"));
            }
            report.warnings.push(format!("{message}；已自动备份并停用旧覆盖，使用目标游戏默认渲染以避免资源加载失败。原着色器效果未保留"));
        } else {
            report
                .warnings
                .push(format!("{message}；按保留策略原样保存，效果未验证"));
        }
        return Ok(());
    }
    let imports = Regex::new(r"(?m)(#moj_import\s*<)([^>:\s]+)(>)")?;
    for path in paths {
        if !matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("vsh" | "fsh" | "glsl")
        ) {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let updated = if target >= 55 {
            imports
                .replace_all(&text, "${1}minecraft:${2}${3}")
                .into_owned()
        } else {
            text.clone()
        };
        if updated != text {
            let relative = path.strip_prefix(root)?;
            let backup = root.join("fastpack/original-shaders").join(relative);
            fs::create_dir_all(backup.parent().unwrap())?;
            if !backup.exists() {
                fs::copy(&path, backup)?;
            }
            fs::write(&path, updated)?;
            report
                .shader_adapted
                .push(format!("{}：命名空间化 import", relative.display()));
        }
    }
    report
        .warnings
        .push("材质包着色器已完成文件级检查；未在目标 Minecraft 客户端编译或验证渲染结果".into());
    Ok(())
}
