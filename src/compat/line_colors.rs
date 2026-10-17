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

pub const MIN_WIDTH: f32 = 0.5;
pub const MAX_WIDTH: f32 = 8.0;
pub const DEFAULT_WIDTH: f32 = 2.0;

pub fn parse_width(value: &str) -> std::result::Result<f32, String> {
    let value = value
        .parse::<f32>()
        .map_err(|_| "宽度须为 0.5–8 的倍数".to_string())?;
    if !value.is_finite() || !(MIN_WIDTH..=MAX_WIDTH).contains(&value) {
        return Err("宽度须为 0.5–8 的倍数，1 为原版宽度".into());
    }
    Ok(value)
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Widths {
    pub fishing: Option<f32>,
    pub outline: Option<f32>,
}

impl Widths {
    pub fn sanitized(self) -> Self {
        let valid = |value: &f32| value.is_finite() && (MIN_WIDTH..=MAX_WIDTH).contains(value);
        Self {
            fishing: self.fishing.filter(valid),
            outline: self.outline.filter(valid),
        }
    }
    fn validate(self) -> Result<()> {
        anyhow::ensure!(self == self.sanitized(), "线条宽度须为 0.5–8 的有限倍数");
        Ok(())
    }
    pub fn active(self) -> bool {
        self.fishing.is_some() || self.outline.is_some()
    }
}

/// Explicit current selections; absent selections preserve generated source-pack colors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ColorSelection {
    pub fishing: bool,
    pub outline: bool,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
struct Settings {
    fishing: Option<[u8; 3]>,
    outline: Option<[u8; 3]>,
    fishing_rainbow: bool,
    outline_rainbow: bool,
    widths: Widths,
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
    let width_function =
        Regex::new(r"(?s)\nfloat fastpack_line_width\(vec4 original\) \{.*?\n\}\n").unwrap();
    let vertex = width_function
        .replace_all(&vertex, "\n")
        .replace("(LineWidth * fastpack_line_width(Color))", "LineWidth");
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
    if settings.widths.active() {
        custom.push_str("float fastpack_line_width(vec4 original) {\n    bool blackLine = all(lessThan(abs(original.rgb), vec3(0.0001)));\n");
        if let Some(width) = settings.widths.fishing {
            custom.push_str(&format!(
                "    if (blackLine && original.a > 0.99) return {width:.3};\n"
            ));
        }
        if let Some(width) = settings.widths.outline {
            custom.push_str(&format!(
                "    if (blackLine && original.a > 0.35 && original.a < 0.45) return {width:.3};\n"
            ));
        }
        custom.push_str("    return 1.0;\n}\n\n");
    }
    let mut result = vertex.to_string();
    if settings.widths.active() {
        // Touch only the screen-space extrusion, never a custom shader's declarations.
        let extrusion = Regex::new(r"\bvec2\s+lineOffset\s*=\s*[^;]+;").unwrap();
        let mut matches = extrusion.find_iter(&result);
        let offset = matches.next()?;
        if matches.next().is_some() || offset.start() < start {
            return None;
        }
        let variable = Regex::new(r"\bLineWidth\b").unwrap();
        if !variable.is_match(offset.as_str()) {
            return None;
        }
        let replacement = variable
            .replace_all(offset.as_str(), "(LineWidth * fastpack_line_width(Color))")
            .into_owned();
        result.replace_range(offset.range(), &replacement);
    }
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
    apply_custom(
        root,
        version,
        fishing,
        outline,
        fishing_rainbow,
        outline_rainbow,
        Widths::default(),
        report,
    )
}

pub fn apply_custom(
    root: &Path,
    version: &str,
    fishing: Option<[u8; 3]>,
    outline: Option<[u8; 3]>,
    fishing_rainbow: bool,
    outline_rainbow: bool,
    widths: Widths,
    report: &mut AssetReport,
) -> Result<()> {
    apply_selected(
        root,
        version,
        fishing,
        outline,
        fishing_rainbow,
        outline_rainbow,
        widths,
        None,
        report,
    )
}

pub fn apply_selected(
    root: &Path,
    version: &str,
    fishing: Option<[u8; 3]>,
    outline: Option<[u8; 3]>,
    fishing_rainbow: bool,
    outline_rainbow: bool,
    widths: Widths,
    selection: Option<ColorSelection>,
    report: &mut AssetReport,
) -> Result<()> {
    widths.validate()?;
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
        widths: Widths {
            fishing: widths.fishing.or(original.widths.sanitized().fishing),
            outline: widths.outline.or(original.widths.sanitized().outline),
        },
    };
    let mut settings = Settings {
        fishing: settings.fishing.or(fishing_rainbow.then_some([0; 3])),
        outline: settings.outline.or(outline_rainbow.then_some([0; 3])),
        ..settings
    };
    if let Some(selection) = selection {
        if !selection.fishing {
            settings.fishing = None;
            settings.fishing_rainbow = false;
        } else {
            settings.fishing_rainbow = fishing_rainbow;
        }
        if !selection.outline {
            settings.outline = None;
            settings.outline_rainbow = false;
        } else {
            settings.outline_rainbow = outline_rainbow;
        }
    }
    // Rebuild the generated shader even when the last color was unchecked.
    // Otherwise the old helper remains active despite clearing the cached flags.
    let clearing_colors =
        selection.is_some() && (original.fishing.is_some() || original.outline.is_some());
    if settings.fishing.is_none()
        && settings.outline.is_none()
        && !settings.widths.active()
        && !clearing_colors
    {
        return Ok(());
    }
    if crate::pack_format_of(version)? < 7 {
        fs::create_dir_all(cache.parent().unwrap())?;
        fs::write_atomic(&cache, &serde_json::to_vec(&settings)?)?;
        report.warnings.push("目标版本早于 1.17，普通资源包不能自定义鱼线与方块选中边框的颜色或宽度；配置保留供后续升级使用".into());
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
    if settings.fishing.is_some() || settings.outline.is_some() {
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
    }
    if settings.widths.active() {
        let mut values = vec![];
        if let Some(width) = settings.widths.fishing {
            values.push(format!("鱼竿线 {width:.1}×"));
        }
        if let Some(width) = settings.widths.outline {
            values.push(format!("方块边框 {width:.1}×"));
        }
        report.converted.push(format!(
            "线条宽度：{}；只缩放对应线条的原版屏幕宽度，保留颜色、透明度与雾效",
            values.join("、")
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn current_color_selection_clears_cached_other_channel_without_losing_widths() {
        for (version, _) in crate::tables().pack_formats.iter().filter(|(_, f)| *f >= 7) {
            for fishing_only in [true, false] {
                let temp = tempfile::tempdir().unwrap();
                let root = temp.path();
                let mut report = AssetReport::default();
                let widths = Widths {
                    fishing: Some(2.0),
                    outline: Some(3.0),
                };
                apply_custom(
                    root,
                    version,
                    Some([12, 34, 56]),
                    Some([90, 80, 70]),
                    true,
                    true,
                    widths,
                    &mut report,
                )
                .unwrap();
                apply_selected(
                    root,
                    version,
                    fishing_only.then_some([12, 34, 56]),
                    (!fishing_only).then_some([90, 80, 70]),
                    fishing_only,
                    !fishing_only,
                    Widths::default(),
                    Some(ColorSelection {
                        fishing: fishing_only,
                        outline: !fishing_only,
                    }),
                    &mut report,
                )
                .unwrap();
                let saved: Settings = serde_json::from_slice(
                    &std::fs::read(root.join("fastpack/line-colors.json")).unwrap(),
                )
                .unwrap();
                assert_eq!(saved.fishing.is_some(), fishing_only, "{version}");
                assert_eq!(saved.outline.is_some(), !fishing_only, "{version}");
                assert_eq!(saved.fishing_rainbow, fishing_only);
                assert_eq!(saved.outline_rainbow, !fishing_only);
                assert_eq!(saved.widths, widths);
                let shader = std::fs::read_to_string(
                    root.join("assets/minecraft/shaders/core/rendertype_lines.vsh"),
                )
                .unwrap();
                let color = shader.split("float fastpack_line_width").next().unwrap();
                assert_eq!(color.contains("original.a > 0.99 ?"), fishing_only);
                assert_eq!(color.contains("original.a < 0.45 ?"), !fishing_only);
                assert_eq!(color.matches(super::super::rainbow::GLSL).count(), 1);
                apply_selected(
                    root,
                    version,
                    None,
                    None,
                    false,
                    false,
                    Widths::default(),
                    Some(ColorSelection::default()),
                    &mut report,
                )
                .unwrap();
                let shader = std::fs::read_to_string(
                    root.join("assets/minecraft/shaders/core/rendertype_lines.vsh"),
                )
                .unwrap();
                assert!(!shader.contains(super::super::rainbow::GLSL));
                assert!(shader.contains("return 2.000;") && shader.contains("return 3.000;"));
            }
        }
    }

    #[test]
    fn unchecking_the_last_color_rebuilds_identity_even_without_widths() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let native = &CATALOG.profiles[&CATALOG.versions["1.20.1"]].vertex;
        write(
            root,
            "vsh",
            native
                .replace("VIEW_SHRINK = 1.0", "VIEW_SHRINK = 0.99")
                .as_bytes(),
        )
        .unwrap();
        write(root, "fsh", b"authored fragment").unwrap();
        let mut report = AssetReport::default();
        apply_custom(
            root,
            "1.20.1",
            None,
            Some([1, 2, 3]),
            false,
            true,
            Widths::default(),
            &mut report,
        )
        .unwrap();
        apply_selected(
            root,
            "1.20.1",
            None,
            None,
            false,
            false,
            Widths::default(),
            Some(ColorSelection::default()),
            &mut report,
        )
        .unwrap();
        let shader = std::fs::read_to_string(
            root.join("assets/minecraft/shaders/core/rendertype_lines.vsh"),
        )
        .unwrap();
        assert!(!shader.contains(super::super::rainbow::GLSL));
        assert!(!shader.contains("original.a > 0.35"));
        assert!(shader.contains("VIEW_SHRINK = 0.99"));
        assert_eq!(
            std::fs::read(root.join("assets/minecraft/shaders/core/rendertype_lines.fsh")).unwrap(),
            b"authored fragment"
        );
        let saved: Settings =
            serde_json::from_slice(&std::fs::read(root.join("fastpack/line-colors.json")).unwrap())
                .unwrap();
        assert!(saved.fishing.is_none() && saved.outline.is_none());
        assert!(!saved.fishing_rainbow && !saved.outline_rainbow);
    }

    #[test]
    fn widths_work_independently_in_every_native_shader_and_do_not_accumulate() {
        for (version, _) in crate::tables().pack_formats.iter().filter(|(_, f)| *f >= 7) {
            let native = &CATALOG.profiles[&CATALOG.versions[version]].vertex;
            let mut settings = Settings {
                widths: Widths {
                    fishing: Some(2.0),
                    outline: Some(3.0),
                },
                ..Default::default()
            };
            let first = hook(native, &settings).unwrap_or_else(|| panic!("{version}"));
            assert!(compatible(&first, native), "{version}");
            assert!(first.contains("original.a > 0.99) return 2.000;"));
            assert!(first.contains("original.a < 0.45) return 3.000;"));
            assert_eq!(
                first
                    .matches("(LineWidth * fastpack_line_width(Color))")
                    .count(),
                1
            );
            assert!(first.contains("return vec4(rgb, original.a)"));
            settings.widths.fishing = Some(1.0);
            settings.widths.outline = Some(8.0);
            settings.fishing = Some([255, 0, 0]);
            settings.fishing_rainbow = true;
            let second = hook(&first, &settings).unwrap();
            assert!(compatible(&second, native), "{version}");
            assert_eq!(second.matches("float fastpack_line_width(").count(), 1);
            assert_eq!(
                second
                    .matches("(LineWidth * fastpack_line_width(Color))")
                    .count(),
                1
            );
            assert!(!second.contains("return 2.000;") && !second.contains("return 3.000;"));
            assert!(second.contains("original.a > 0.99) return 1.000;"));
            assert!(second.contains(super::super::rainbow::GLSL));
            settings.widths = Widths::default();
            let colors_only = hook(&second, &settings).unwrap();
            assert!(!colors_only.contains("fastpack_line_width"));
            assert!(compatible(&colors_only, native));
        }
    }

    #[test]
    fn authored_math_and_cached_widths_survive_repeated_conversions() {
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
        apply_custom(
            root,
            "1.20.1",
            None,
            None,
            false,
            false,
            Widths {
                fishing: Some(2.5),
                outline: None,
            },
            &mut report,
        )
        .unwrap();
        apply_custom(
            root,
            "1.20.1",
            None,
            Some([10, 20, 30]),
            false,
            true,
            Widths {
                fishing: None,
                outline: Some(3.0),
            },
            &mut report,
        )
        .unwrap();
        let file = root.join("assets/minecraft/shaders/core/rendertype_lines.vsh");
        let shader = std::fs::read_to_string(&file).unwrap();
        assert!(shader.contains("VIEW_SHRINK = 0.99"));
        assert!(shader.contains("return 2.500;") && shader.contains("return 3.000;"));
        assert_eq!(
            std::fs::read(root.join("assets/minecraft/shaders/core/rendertype_lines.fsh")).unwrap(),
            b"custom fragment"
        );
        apply_custom(
            root,
            "1.20.1",
            None,
            None,
            false,
            false,
            Widths {
                fishing: Some(1.0),
                outline: None,
            },
            &mut report,
        )
        .unwrap();
        let shader = std::fs::read_to_string(file).unwrap();
        assert!(shader.contains("return 1.000;") && !shader.contains("return 2.500;"));
        assert_eq!(shader.matches("float fastpack_line_width(").count(), 1);
        let saved: Settings =
            serde_json::from_slice(&std::fs::read(root.join("fastpack/line-colors.json")).unwrap())
                .unwrap();
        assert_eq!(
            saved.widths,
            Widths {
                fishing: Some(1.0),
                outline: Some(3.0)
            }
        );
        assert!(saved.outline_rainbow);
    }

    #[test]
    fn widths_do_not_rewrite_other_uses_of_the_native_width_variable() {
        let native = &CATALOG.profiles[&CATALOG.versions["1.20.1"]].vertex;
        let authored = native.replace(
            "void main() {",
            "void main() {\n    float savedWidth = LineWidth;",
        );
        let settings = Settings {
            widths: Widths {
                fishing: Some(4.0),
                outline: None,
            },
            ..Default::default()
        };
        let shader = hook(&authored, &settings).unwrap();
        assert!(shader.contains("float savedWidth = LineWidth;"));
        assert_eq!(shader.matches("fastpack_line_width(Color)").count(), 1);
        assert!(
            hook(
                &native.replace("vec2 lineOffset =", "vec2 unknownOffset ="),
                &settings
            )
            .is_none()
        );
    }

    #[test]
    fn invalid_widths_are_rejected_and_legacy_retains_only_configuration() {
        for width in [f32::NAN, f32::INFINITY, 0.0, -1.0, 8.1] {
            let temp = tempfile::tempdir().unwrap();
            assert!(
                apply_custom(
                    temp.path(),
                    "1.20.1",
                    None,
                    None,
                    false,
                    false,
                    Widths {
                        fishing: Some(width),
                        outline: None
                    },
                    &mut AssetReport::default()
                )
                .is_err()
            );
            assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
        }
        for input in ["NaN", "inf", "-1", "8.1", "bad"] {
            assert!(parse_width(input).is_err());
        }
        for input in ["0.5", "1", "2.5", "8"] {
            assert!(parse_width(input).is_ok());
        }
        let legacy: Settings = serde_json::from_str(r#"{"fishing":[1,2,3]}"#).unwrap();
        assert!(!legacy.widths.active());
        let temp = tempfile::tempdir().unwrap();
        let mut report = AssetReport::default();
        apply_custom(
            temp.path(),
            "1.8.9",
            None,
            None,
            false,
            false,
            Widths {
                fishing: Some(2.0),
                outline: Some(3.0),
            },
            &mut report,
        )
        .unwrap();
        assert!(!temp.path().join("assets").exists());
        assert!(temp.path().join("fastpack/line-colors.json").is_file());
        assert_eq!(report.warnings.len(), 1);
    }

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
