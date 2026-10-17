use super::assets::{AssetReport, backup};
use crate::packfs::{self as fs, PathExt};
use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

#[derive(Deserialize)]
struct Profile {
    vertex: String,
    fragment: String,
    program: Option<Value>,
}
#[derive(Deserialize)]
struct Catalog {
    versions: BTreeMap<String, String>,
    profiles: BTreeMap<String, Profile>,
}
static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../assets/line-colors.json")).expect("线条着色器参考无效")
});

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
struct Settings {
    fishing: Option<[u8; 3]>,
    outline: Option<[u8; 3]>,
    fishing_rainbow: bool,
    outline_rainbow: bool,
}

fn hook(vertex: &str, settings: &Settings) -> Option<String> {
    let declaration = Regex::new(r"\bin\s+vec4\s+Color\s*;").unwrap();
    if !declaration.is_match(vertex) {
        return None;
    }
    let function =
        Regex::new(r"(?s)\nvec4 fastpack_line_color\(vec4 original\) \{.*?\n\}\n").unwrap();
    let vertex = function
        .replace_all(vertex, "\n")
        .replace("fastpack_line_color(Color)", "Color");
    let animated = settings.fishing_rainbow || settings.outline_rainbow;
    let vertex = if animated
        && !vertex.contains("globals.glsl")
        && !Regex::new(r"\buniform\s+float\s+GameTime\s*;")
            .unwrap()
            .is_match(&vertex)
    {
        let position = vertex.find('\n')? + 1;
        let mut vertex = vertex.to_string();
        vertex.insert_str(position, "\nuniform float GameTime;\n");
        vertex
    } else {
        vertex
    };
    let assignment = Regex::new(r"\bvertexColor\s*=\s*Color\s*;").unwrap();
    if assignment.find_iter(&vertex).count() != 1 {
        return None;
    }
    let main = Regex::new(r"\bvoid\s+main\s*\(\s*\)\s*\{").unwrap();
    let start = main.find(&vertex)?.start();
    let vector = |color: [u8; 3]| {
        format!(
            "vec3({:.8}, {:.8}, {:.8})",
            color[0] as f64 / 255.0,
            color[1] as f64 / 255.0,
            color[2] as f64 / 255.0
        )
    };
    let mut custom = String::from(
        "\nvec4 fastpack_line_color(vec4 original) {\n    bool blackLine = all(lessThan(abs(original.rgb), vec3(0.0001)));\n    vec3 rgb = original.rgb;\n",
    );
    if let Some(color) = settings.fishing {
        custom.push_str(&format!(
            "    rgb = blackLine && original.a > 0.99 ? {} : rgb;\n",
            if settings.fishing_rainbow {
                super::rainbow::GLSL.into()
            } else {
                vector(color)
            }
        ));
    }
    if let Some(color) = settings.outline {
        custom.push_str(&format!(
            "    rgb = blackLine && original.a > 0.35 && original.a < 0.45 ? {} : rgb;\n",
            if settings.outline_rainbow {
                super::rainbow::GLSL.into()
            } else {
                vector(color)
            }
        ));
    }
    custom.push_str("    return vec4(rgb, original.a);\n}\n\n");
    let mut result = vertex.to_string();
    result.insert_str(start, &custom);
    Some(
        assignment
            .replace(&result, "vertexColor = fastpack_line_color(Color);")
            .into_owned(),
    )
}

fn compatible(vertex: &str, native: &str) -> bool {
    let compact = |text: &str| {
        text.chars()
            .filter(|value| !value.is_whitespace())
            .collect::<String>()
    };
    let provided = compact(vertex);
    let declarations=Regex::new(r"(?m)^\s*(#version[^\r\n]+|#(?:moj_import|include)\s*<[^>]+>|(?:layout\([^\r\n]+?\)\s*)?(?:in|out|uniform)\s+[^\r\n;]+;)").unwrap();
    if declarations
        .captures_iter(native)
        .any(|value| !provided.contains(&compact(&value[1])))
    {
        return false;
    }
    let fog = Regex::new(
        r"\b(?:fog_distance|fog_spherical_distance|fog_cylindrical_distance)\([^;\r\n]+?\)",
    )
    .unwrap();
    if fog
        .find_iter(native)
        .any(|value| !provided.contains(&compact(value.as_str())))
    {
        return false;
    }
    ["gl_VertexID", "gl_VertexIndex"]
        .iter()
        .all(|name| !native.contains(name) || vertex.contains(name))
}

fn write(root: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    let path = root.join(format!(
        "assets/minecraft/shaders/core/rendertype_lines.{name}"
    ));
    if path.pack_is_file() {
        backup(root, &path)?;
    }
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write_atomic(&path, bytes)?;
    Ok(())
}

pub fn apply(
    root: &Path,
    version: &str,
    fishing: Option<[u8; 3]>,
    outline: Option<[u8; 3]>,
    report: &mut AssetReport,
) -> Result<()> {
    apply_animated(root, version, fishing, outline, false, false, report)
}

pub fn apply_animated(
    root: &Path,
    version: &str,
    fishing: Option<[u8; 3]>,
    outline: Option<[u8; 3]>,
    fishing_rainbow: bool,
    outline_rainbow: bool,
    report: &mut AssetReport,
) -> Result<()> {
    let cache = root.join("fastpack/line-colors.json");
    let original: Settings = if cache.pack_is_file() {
        serde_json::from_slice(&fs::read(&cache)?).unwrap_or_default()
    } else {
        Settings::default()
    };
    let settings = Settings {
        fishing: fishing.or(original.fishing),
        outline: outline.or(original.outline),
        fishing_rainbow: if fishing.is_some() || fishing_rainbow {
            fishing_rainbow
        } else {
            original.fishing_rainbow
        },
        outline_rainbow: if outline.is_some() || outline_rainbow {
            outline_rainbow
        } else {
            original.outline_rainbow
        },
    };
    let settings = Settings {
        fishing: settings.fishing.or(fishing_rainbow.then_some([0; 3])),
        outline: settings.outline.or(outline_rainbow.then_some([0; 3])),
        ..settings
    };
    if settings.fishing.is_none() && settings.outline.is_none() {
        return Ok(());
    }
    if crate::pack_format_of(version)? < 7 {
        fs::create_dir_all(cache.parent().unwrap())?;
        fs::write_atomic(&cache, &serde_json::to_vec(&settings)?)?;
        report.warnings.push("目标版本早于 1.17，普通资源包不能自定义鱼线与方块选中边框颜色；颜色配置保留供后续升级使用".into());
        return Ok(());
    }
    let profile = CATALOG
        .profiles
        .get(
            CATALOG
                .versions
                .get(version)
                .context("缺少目标版本线条着色器索引")?,
        )
        .context("缺少目标版本线条着色器")?;
    let path = root.join("assets/minecraft/shaders/core/rendertype_lines.vsh");
    let existing = path
        .pack_is_file()
        .then(|| fs::read_to_string(&path))
        .transpose()?;
    let program = root.join("assets/minecraft/shaders/core/rendertype_lines.json");
    let standard_program = if program.pack_is_file() {
        serde_json::from_slice::<Value>(&fs::read(&program)?)
            .ok()
            .is_some_and(|value| {
                ["vertex", "fragment"].iter().all(|key| {
                    value[*key].as_str().is_some_and(|value| {
                        matches!(
                            value,
                            "rendertype_lines"
                                | "minecraft:rendertype_lines"
                                | "minecraft:core/rendertype_lines"
                        )
                    })
                })
            })
    } else {
        true
    };
    let own = existing
        .as_ref()
        .is_some_and(|text| text.contains("fastpack_line_color"));
    let adapted = existing
        .as_ref()
        .filter(|vertex| standard_program && compatible(vertex, &profile.vertex))
        .and_then(|vertex| hook(vertex, &settings));
    let shader = if let Some(vertex) = adapted {
        vertex
    } else {
        if existing.is_some() && !own {
            report.warnings.push(
                "线条颜色：原包线条着色器无法安全注入，已备份并使用该目标版本的原版线条程序".into(),
            );
        }
        write(root, "fsh", profile.fragment.as_bytes())?;
        if let Some(program) = &profile.program {
            write(root, "json", &serde_json::to_vec_pretty(program)?)?;
        }
        hook(&profile.vertex, &settings).context("目标版本线条颜色注入失败")?
    };
    write(root, "vsh", shader.as_bytes())?;
    if settings.fishing_rainbow || settings.outline_rainbow {
        if let Some(native) = &profile.program {
            let mut value = if program.pack_is_file() {
                serde_json::from_slice::<Value>(&fs::read(&program)?)?
            } else {
                native.clone()
            };
            let uniforms = value["uniforms"]
                .as_array_mut()
                .context("线条程序缺少 uniforms")?;
            if !uniforms.iter().any(|value| value["name"] == "GameTime") {
                uniforms.push(
                    serde_json::json!({"name":"GameTime","type":"float","count":1,"values":[0.0]}),
                );
                write(root, "json", &serde_json::to_vec_pretty(&value)?)?;
            }
        }
        report.converted.push(
            "线条动态彩色：按游戏时间六秒循环，鱼线与方块边框独立控制，保留透明度与雾效".into(),
        );
    }
    fs::create_dir_all(cache.parent().unwrap())?;
    fs::write_atomic(&cache, &serde_json::to_vec(&settings)?)?;
    report.converted.push(format!(
        "线条颜色：按 {version} 原生管线{}{}，保留线条透明度与雾效",
        if settings.fishing.is_some() {
            "自定义鱼竿线"
        } else {
            ""
        },
        if settings.outline.is_some() {
            " 自定义瞄准方块边框"
        } else {
            ""
        }
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_supported_shader_accepts_both_colors_and_repeated_edits() {
        for (version, _) in crate::tables()
            .pack_formats
            .iter()
            .filter(|(_, format)| *format >= 7)
        {
            let profile = &CATALOG.profiles[&CATALOG.versions[version]];
            assert!(
                profile.fragment.starts_with("#version") && profile.fragment.contains("void main"),
                "{version}"
            );
            let temp = tempfile::tempdir().unwrap();
            let mut report = AssetReport::default();
            apply(
                temp.path(),
                version,
                Some([255, 0, 0]),
                Some([0, 255, 0]),
                &mut report,
            )
            .unwrap();
            let file = temp
                .path()
                .join("assets/minecraft/shaders/core/rendertype_lines.vsh");
            let first = std::fs::read_to_string(&file).unwrap();
            assert!(
                first.contains("original.a > 0.99") && first.contains("original.a < 0.45"),
                "{version}"
            );
            assert!(first.contains("return vec4(rgb, original.a)"));
            apply(temp.path(), version, Some([0, 0, 255]), None, &mut report).unwrap();
            let second = std::fs::read_to_string(file).unwrap();
            assert_eq!(
                second.matches("vec4 fastpack_line_color(").count(),
                1,
                "{version}"
            );
            assert!(second.contains("vec3(0.00000000, 0.00000000, 1.00000000)"));
            assert!(!second.contains("vec3(1.00000000, 0.00000000, 0.00000000)"));
        }
    }
    #[test]
    fn disabled_options_leave_the_pack_untouched_and_legacy_is_explicit() {
        let temp = tempfile::tempdir().unwrap();
        let mut report = AssetReport::default();
        apply(temp.path(), "1.20.1", None, None, &mut report).unwrap();
        assert!(!temp.path().join("assets").exists());
        apply(temp.path(), "1.8.9", Some([255; 3]), None, &mut report).unwrap();
        assert!(!temp.path().join("assets").exists());
        assert_eq!(report.warnings.len(), 1);
    }
    #[test]
    fn compatible_custom_lines_keep_original_math_and_fragment_shader() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let source = &CATALOG.profiles[&CATALOG.versions["1.20.1"]].vertex;
        write(
            root,
            "vsh",
            source
                .replace("VIEW_SHRINK = 1.0", "VIEW_SHRINK = 0.99")
                .as_bytes(),
        )
        .unwrap();
        write(root, "fsh", b"custom fragment").unwrap();
        let mut report = AssetReport::default();
        apply(root, "1.20.1", Some([128; 3]), None, &mut report).unwrap();
        assert!(
            std::fs::read_to_string(
                root.join("assets/minecraft/shaders/core/rendertype_lines.vsh")
            )
            .unwrap()
            .contains("VIEW_SHRINK = 0.99")
        );
        assert_eq!(
            std::fs::read(root.join("assets/minecraft/shaders/core/rendertype_lines.fsh")).unwrap(),
            b"custom fragment"
        );
        apply(root, "1.20.1", Some([64; 3]), None, &mut report).unwrap();
        assert_eq!(
            std::fs::read(root.join("assets/minecraft/shaders/core/rendertype_lines.fsh")).unwrap(),
            b"custom fragment"
        );
    }
}
