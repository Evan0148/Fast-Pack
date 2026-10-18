//! Optional, locally bundled overlays. Nothing is loaded or installed at startup.
use crate::compat::{
    assets::{AssetReport, backup},
    native_cit,
};
use crate::packfs::{self as fs, PathExt};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Feature {
    Sharpness,
    GoldenDurability,
    BowEnchantments,
    ArmorProtection,
    HeypixelPack,
    HighlightNames,
    ClearTooltips,
    RainbowOutline,
}
impl Feature {
    pub const ALL: [Self; 8] = [
        Self::Sharpness,
        Self::GoldenDurability,
        Self::BowEnchantments,
        Self::ArmorProtection,
        Self::HeypixelPack,
        Self::HighlightNames,
        Self::ClearTooltips,
        Self::RainbowOutline,
    ];
    pub fn title(self) -> &'static str {
        match self {
            Self::Sharpness => "显示剑的锋利等级",
            Self::GoldenDurability => "显示金剑剩余耐久",
            Self::BowEnchantments => "显示弓的附魔等级",
            Self::ArmorProtection => "显示护甲保护等级",
            Self::HeypixelPack => "布吉岛综合外观",
            Self::HighlightNames => "高亮玩家昵称",
            Self::ClearTooltips => "透明物品提示框",
            Self::RainbowOutline => "彩色方块边框",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::Sharpness => "背包数字 I–V，沿用当前剑的外观",
            Self::GoldenDurability => "背包显示 1–32 剩余耐久",
            Self::BowEnchantments => "力量 I–V、火矢与冲击，保留拉弓状态",
            Self::ArmorProtection => "背包显示保护 I–IV，保留染色与纹饰",
            Self::HeypixelPack => "内含物品、方块、GUI、字体、粒子、音效、CIT 与布吉岛饰品模型",
            Self::HighlightNames => "粉色昵称弱透明文字；需要 1.17+ 核心着色器",
            Self::ClearTooltips => "移除原包提示框底色；需要 1.17+ 核心着色器",
            Self::RainbowOutline => "使用现有边框动态彩色，单独调色设置优先",
        }
    }
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    pub features: Vec<Feature>,
}
impl Settings {
    pub fn enabled(&self, feature: Feature) -> bool {
        self.features.contains(&feature)
    }
    pub fn set(&mut self, feature: Feature, enabled: bool) {
        self.features.retain(|value| *value != feature);
        if enabled {
            self.features.push(feature);
        }
    }
    pub fn active(&self) -> bool {
        !self.features.is_empty()
    }
    pub fn count(&self) -> usize {
        Feature::ALL
            .into_iter()
            .filter(|feature| self.enabled(*feature))
            .count()
    }
}
type Files = BTreeMap<String, Vec<u8>>;
const STATE: &str = "fastpack/builtin-overlays/settings.json";
#[derive(Default, Deserialize, Serialize)]
struct Provenance {
    #[serde(flatten)]
    settings: Settings,
    #[serde(default)]
    generated: BTreeMap<String, String>,
}
fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}
fn provenance(root: &Path) -> Result<Provenance> {
    if !root.join(STATE).pack_is_file() {
        return Ok(Provenance::default());
    }
    Ok(serde_json::from_slice(&fs::read(root.join(STATE))?).unwrap_or_default())
}
pub(crate) fn effective(root: &Path, requested: &Settings) -> Result<Settings> {
    Ok(if requested.active() {
        requested.clone()
    } else {
        provenance(root)?.settings
    })
}
pub(crate) fn prepare(root: &Path) -> Result<Vec<String>> {
    let state = provenance(root)?;
    let mut notes = vec![];
    // Preserve the complete generated graph when any node has been edited.
    // Deleting just its dependencies would leave an authored model broken.
    for (relative, expected) in &state.generated {
        let allowed = relative.starts_with("assets/minecraft/items/")
            || relative.starts_with("assets/minecraft/optifine/cit/fastpack_builtin_hud/")
            || relative.starts_with("assets/minecraft/models/item/fastpack_builtin_hud_cit/")
            || relative.starts_with("assets/minecraft/textures/item/fastpack_builtin_hud_cit/");
        ensure!(
            allowed
                && !relative.contains('\\')
                && !relative
                    .split('/')
                    .any(|part| matches!(part, "" | "." | "..")),
            "内置覆盖缓存路径无效"
        );
        if fs::read(root.join(relative)).is_ok_and(|bytes| digest(&bytes) != *expected) {
            write(root, "fastpack/builtin-overlays/edited.json", b"true")?;
            return Ok(vec![
                "内置等级资源已被编辑，保留整组模型、规则及其依赖，不自动重建数字层".into(),
            ]);
        }
    }
    for (relative, expected) in state.generated {
        let safe = relative.starts_with("assets/minecraft/items/")
            || relative.starts_with("assets/minecraft/optifine/cit/fastpack_builtin_hud/")
            || relative.starts_with("assets/minecraft/models/item/fastpack_builtin_hud_cit/")
            || relative.starts_with("assets/minecraft/textures/item/fastpack_builtin_hud_cit/");
        ensure!(
            safe && !relative.contains('\\')
                && !relative.split('/').any(|p| matches!(p, "" | "." | "..")),
            "内置覆盖缓存路径无效"
        );
        let path = root.join(&relative);
        if !path.pack_is_file() {
            continue;
        }
        let bytes = fs::read(&path)?;
        if digest(&bytes) != expected {
            notes.push(format!("内置覆盖资源 {relative} 已修改，保留编辑结果"));
            continue;
        }
        if relative.starts_with("assets/minecraft/items/") {
            let value: Value = serde_json::from_slice(&bytes)?;
            if let Some(base) = value.get("fastpack_builtin_hud") {
                write(root, &relative, &serde_json::to_vec(base)?)?;
            }
        } else {
            fs::remove_file(path)?;
        }
    }
    Ok(notes)
}
fn archive(bytes: &[u8]) -> Result<Files> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))?;
    let mut files = BTreeMap::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        let path = entry.enclosed_name().context("内置资源路径无效")?;
        let name = path.to_string_lossy().replace('\\', "/");
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        files.insert(name, data);
    }
    Ok(files)
}
static BOW: LazyLock<Files> =
    LazyLock::new(|| archive(include_bytes!("../assets/builtin-overlays/bow.zip")).unwrap());
static ARMOR: LazyLock<Files> =
    LazyLock::new(|| archive(include_bytes!("../assets/builtin-overlays/armor.zip")).unwrap());
static SWORD: LazyLock<Files> =
    LazyLock::new(|| archive(include_bytes!("../assets/builtin-overlays/sword.zip")).unwrap());

fn write(root: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    let path = root.join(name);
    if path.pack_is_file() {
        if fs::read(&path)? == bytes {
            return Ok(());
        }
        backup(root, &path)?;
    }
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write_atomic(&path, bytes)?;
    Ok(())
}

type Cached = Arc<OnceLock<std::result::Result<Arc<Files>, String>>>;
static CONVERTED: LazyLock<Mutex<BTreeMap<String, Cached>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));
fn converted_server(version: &str) -> Result<Arc<Files>> {
    let cache = CONVERTED
        .lock()
        .unwrap()
        .entry(version.into())
        .or_default()
        .clone();
    let value = cache.get_or_init(|| {
        (|| -> Result<Arc<Files>> {
            let temp = tempfile::tempdir()?;
            let source = temp.path().join("source.zip");
            let output = temp.path().join("converted.zip");
            // The source retains all supplied assets; shader effects are installed
            // separately using the target's own shader profile.
            std::fs::write(
                &source,
                include_bytes!("../assets/builtin-overlays/heypixel.zip"),
            )?;
            crate::convert_pack(
                &source,
                &output,
                &crate::ConversionOptions {
                    tgt_format: crate::pack_format_of(version)?,
                    target_version: Some(version.into()),
                    heypixel_compat: false,
                    auto_client_setup: false,
                    auto_netherite: false,
                    auto_netherite_sword: false,
                    ..Default::default()
                },
                |_, _| {},
            )?;
            let mut files = archive(&std::fs::read(output)?)?;
            files.retain(|name, _| name.starts_with("assets/") && !name.contains("/shaders/"));
            Ok(Arc::new(files))
        })()
        .map_err(|error| format!("{error:#}"))
    });
    value.clone().map_err(anyhow::Error::msg)
}

fn gui(node: &Value) -> Option<Value> {
    node["cases"]
        .as_array()?
        .iter()
        .find(|case| case["when"] == "gui")
        .map(|case| case["model"].clone())
}
fn markers(node: &Value) -> Value {
    if node["type"] == "minecraft:model"
        && node["model"]
            .as_str()
            .is_some_and(|model| model.starts_with("minecraft:"))
    {
        return json!({"type":"minecraft:empty"});
    }
    // Every original composite has the ordinary item as its first child.
    if node["type"] == "minecraft:composite" {
        let layers = node["models"].as_array().unwrap();
        return if layers.len() == 2 {
            layers[1].clone()
        } else {
            json!({"type":"minecraft:composite","models":layers.iter().skip(1).cloned().collect::<Vec<_>>()})
        };
    }
    match node {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    (
                        key.clone(),
                        if matches!(key.as_str(), "model" | "on_true" | "on_false" | "fallback")
                            && value.is_object()
                        {
                            markers(value)
                        } else if key == "entries" {
                            Value::Array(
                                value
                                    .as_array()
                                    .unwrap()
                                    .iter()
                                    .map(|entry| {
                                        let mut entry = entry.clone();
                                        entry["model"] = markers(&entry["model"]);
                                        entry
                                    })
                                    .collect(),
                            )
                        } else {
                            value.clone()
                        },
                    )
                })
                .collect(),
        ),
        _ => node.clone(),
    }
}
fn template_branch_available(root: &Path, format: u32, value: &Value) -> bool {
    if value["type"] == "minecraft:model" {
        return value["model"].as_str().is_some_and(|reference| {
            crate::compat::custom_items::cit_model(root, format, reference).is_ok()
                || reference.strip_suffix("_trim").is_some_and(|base| {
                    crate::compat::custom_items::cit_model(
                        root,
                        format,
                        &format!("{base}_darker_trim"),
                    )
                    .is_ok()
                })
        });
    }
    match value {
        Value::Object(map) => map
            .values()
            .all(|child| template_branch_available(root, format, child)),
        Value::Array(array) => array
            .iter()
            .all(|child| template_branch_available(root, format, child)),
        _ => true,
    }
}

/// Only filter the bundled fallback template. Authored item definitions retain
/// their conditions and the regular converter's diagnostics.
fn target_trim_template(root: &Path, format: u32, value: &mut Value) {
    if value["type"] == "minecraft:select" && value["property"] == "minecraft:trim_material" {
        if let Some(cases) = value["cases"].as_array_mut() {
            cases.retain(|case| template_branch_available(root, format, &case["model"]));
        }
        if value["cases"].as_array().is_some_and(Vec::is_empty) {
            *value = value["fallback"].clone();
        }
    }
    match value {
        Value::Object(map) => {
            for child in map.values_mut() {
                target_trim_template(root, format, child);
            }
        }
        Value::Array(array) => {
            for child in array {
                target_trim_template(root, format, child);
            }
        }
        _ => {}
    }
}

fn install_hud(
    root: &Path,
    version: &str,
    settings: &Settings,
    report: &mut AssetReport,
) -> Result<()> {
    if root
        .join("fastpack/builtin-overlays/edited.json")
        .pack_is_file()
    {
        report.warnings.push(
            "内置等级资源含用户编辑，继续保留该组资源；从原始包重新转换可以应用内置选项".into(),
        );
        return Ok(());
    }
    let format = crate::pack_format_of(version)?;
    let packs = [
        (
            settings.enabled(Feature::Sharpness) || settings.enabled(Feature::GoldenDurability),
            &*SWORD,
        ),
        (settings.enabled(Feature::BowEnchantments), &*BOW),
        (settings.enabled(Feature::ArmorProtection), &*ARMOR),
    ];
    if !packs.iter().any(|(on, _)| *on) {
        return Ok(());
    }
    let catalog = crate::compat::custom_items::items(version)
        .iter()
        .map(|item| item.id.clone())
        .collect::<BTreeSet<_>>();
    let mut definitions = BTreeMap::new();
    for (enabled, files) in packs {
        if !enabled {
            continue;
        }
        for (name, bytes) in files {
            if !name.starts_with("assets/") {
                continue;
            }
            if (name.contains("swordhud/")
                && name.contains("/dur/")
                && !settings.enabled(Feature::GoldenDurability))
                || (name.contains("swordhud/")
                    && name.contains("/sharp/")
                    && !settings.enabled(Feature::Sharpness))
            {
                continue;
            }
            if !name.starts_with("assets/minecraft/items/") {
                // Both original gold-sword counters occupy the top-left corner.
                // Move durability below the blade when both counters are selected.
                if settings.enabled(Feature::Sharpness)
                    && settings.enabled(Feature::GoldenDurability)
                    && name.starts_with("assets/swordhud/textures/item/dur/")
                    && name.ends_with(".png")
                {
                    let image = image::load_from_memory(bytes)?.to_rgba8();
                    let mut shifted = image::RgbaImage::new(image.width(), image.height());
                    image::imageops::replace(&mut shifted, &image, 0, 11);
                    let mut png = Cursor::new(Vec::new());
                    shifted.write_to(&mut png, image::ImageFormat::Png)?;
                    write(root, name, png.get_ref())?;
                } else {
                    write(root, name, bytes)?;
                }
                continue;
            }
            let item = name
                .strip_prefix("assets/minecraft/items/")
                .unwrap()
                .trim_end_matches(".json");
            if !catalog.contains(&format!("minecraft:{item}")) {
                continue;
            }
            let sword = item.ends_with("_sword");
            if sword && item != "golden_sword" && !settings.enabled(Feature::Sharpness) {
                continue;
            }
            let source: Value = serde_json::from_slice(bytes)?;
            let Some(original_gui) = gui(&source["model"]) else {
                continue;
            };
            let mut layer = markers(&original_gui);
            if item == "golden_sword" {
                let sharp: Value =
                    serde_json::from_slice(&SWORD["assets/minecraft/items/diamond_sword.json"])?;
                let sharp = markers(&gui(&sharp["model"]).unwrap());
                layer = match (
                    settings.enabled(Feature::Sharpness),
                    settings.enabled(Feature::GoldenDurability),
                ) {
                    (true, true) => json!({"type":"minecraft:composite","models":[layer,sharp]}),
                    (true, false) => sharp,
                    (false, true) => layer,
                    _ => continue,
                };
            }
            let path = root.join(name);
            let authored_definition = format >= 46 && path.pack_is_file();
            let mut base = if format >= 46 && path.pack_is_file() {
                crate::model_json::parse(&fs::read(&path)?)?
            } else if format < 15 && item != "bow" {
                // Armor trims do not exist before 1.20. Do not expand the
                // source pack's future trim selector into old CIT variants.
                json!({"model":{"type":"minecraft:model","model":format!("minecraft:item/{item}")}})
            } else {
                crate::compat::modern_items::definition_reference(format, item)
                    .unwrap_or_else(|| json!({"model":source["model"]["fallback"]}))
            };
            if let Some(original) = base.get("fastpack_builtin_hud").cloned() {
                base = original;
            }
            if !authored_definition {
                target_trim_template(root, format, &mut base);
            }
            let mut output = base.clone();
            output["fastpack_builtin_hud"] = base.clone();
            output["model"] = json!({"type":"minecraft:select","property":"minecraft:display_context","cases":[{"when":"gui","model":{"type":"minecraft:composite","models":[base["model"],layer]}}],"fallback":base["model"]});
            if format >= 55 {
                write(root, name, &serde_json::to_vec(&output)?)?;
            } else {
                definitions.insert(format!("minecraft:{item}"), output);
            }
        }
    }
    if format < 55 {
        native_cit::install_builtin_hud(root, format, &definitions, report)?;
    }
    report
        .converted
        .push("内置等级覆盖：合成背包专用数字，保留基础物品、染色、纹饰与手持／拉弓状态".into());
    Ok(())
}

#[derive(Deserialize)]
struct Shaders {
    versions: BTreeMap<String, String>,
    profiles: BTreeMap<String, BTreeMap<String, String>>,
}
static SHADERS: LazyLock<Shaders> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../assets/builtin-overlays/shaders.json"))
        .expect("内置效果着色器参考无效")
});
fn shaders(
    root: &Path,
    version: &str,
    settings: &Settings,
    report: &mut AssetReport,
) -> Result<()> {
    let names = settings.enabled(Feature::HighlightNames);
    let tooltips = settings.enabled(Feature::ClearTooltips);
    if !names && !tooltips {
        return Ok(());
    }
    if crate::pack_format_of(version)? < 7 {
        report
            .warnings
            .push("昵称高亮与透明提示框需要 1.17+ 原版核心着色器，此目标保留原效果".into());
        return Ok(());
    }
    let profile = &SHADERS.profiles[&SHADERS.versions[version]];
    let mut installed = 0;
    for (name, native) in profile {
        let stem = name.rsplit_once('.').unwrap().0;
        let text = stem == "text" || stem == "rendertype_text_see_through";
        let tooltip = matches!(stem, "gui" | "rendertype_gui" | "position_color");
        if !(names && text || tooltips && tooltip) {
            continue;
        }
        let path = format!("assets/minecraft/shaders/core/{name}");
        let mut shader = native.clone();
        if names && text && name.ends_with(".fsh") {
            let pattern = regex::Regex::new(
                r"vec4\s+color\s*=\s*(?:texture\(Sampler0,\s*texCoord0\)|texColor)\s*\*\s*vertexColor\s*(?:\*\s*ColorModulator\s*)?;",
            )?;
            ensure!(
                pattern.is_match(&shader),
                "目标文字着色器缺少可验证的颜色入口"
            );
            let guard = if stem == "text" {
                "#ifdef IS_SEE_THROUGH\n"
            } else {
                ""
            };
            let end = if stem == "text" { "#endif\n" } else { "" };
            shader=pattern.replace_all(&shader,|captures:&regex::Captures| {
                let original=&captures[0];
                let multiplier=if original.contains("ColorModulator") {" * ColorModulator.rgb"}else{""};
                format!("{original}\n// Fast Pack: supplied name highlight; retain transparent glyph discard.\n{guard}if (vertexColor.a > 0.0 && vertexColor.a < 0.5) {{ color.rgb = vec3(1.0, 0.6, 0.8){multiplier}; }}\n{end}")
            }).into_owned();
        }
        if tooltips && tooltip && name.ends_with(".vsh") {
            ensure!(
                shader.contains("vertexColor = Color;"),
                "目标 GUI 着色器缺少可验证的颜色入口"
            );
            shader=shader.replace("vertexColor = Color;","vertexColor = Color;\n    // Fast Pack: supplied transparent tooltip background.\n    if (abs(Color.r - 0.0627451) < 0.00001 && Color.a >= 0.7 && Color.a < 0.9) { vertexColor.a = 0.0; }");
        }
        write(root, &path, shader.as_bytes())?;
        installed += 1;
    }
    report.converted.push(format!("内置文字与提示框效果：按 {version} 原生管线写入 {installed} 个资源，保留字体透明度和目标接口"));
    Ok(())
}

pub(crate) fn apply(
    root: &Path,
    version: &str,
    settings: &Settings,
    report: &mut AssetReport,
) -> Result<()> {
    if !settings.active() {
        return Ok(());
    }
    if settings.enabled(Feature::HeypixelPack)
        && !provenance(root)?.settings.enabled(Feature::HeypixelPack)
    {
        for (name, bytes) in converted_server(version)?.iter() {
            if name.contains("/lang/") && name.ends_with(".json") && root.join(name).pack_is_file()
            {
                let existing = crate::model_json::parse(&fs::read(root.join(name))?);
                let incoming = serde_json::from_slice::<Value>(bytes)?;
                if let Ok(Value::Object(mut map)) = existing {
                    if let Value::Object(add) = incoming {
                        map.extend(add);
                        write(root, name, &serde_json::to_vec(&map)?)?;
                        continue;
                    }
                }
            }
            write(root, name, bytes)?;
        }
        report.converted.push("内置布吉岛综合外观：应用全部物品、方块、GUI、字体、粒子、音效、CIT 与饰品模型；原包文件已备份".into());
    }
    install_hud(root, version, settings, report)?;
    shaders(root, version, settings, report)?;
    write(
        root,
        "fastpack/builtin-overlays/NOTICE.txt",
        include_bytes!("../assets/builtin-overlays/NOTICE.txt"),
    )?;
    write(
        root,
        "fastpack/builtin-overlays/CC-BY-NC-SA-4.0.txt",
        include_bytes!("../assets/builtin-overlays/CC-BY-NC-SA-4.0.txt"),
    )?;
    write(
        root,
        "fastpack/builtin-overlays/Credits-heypixel.txt",
        include_bytes!("../assets/builtin-overlays/Credits-heypixel.txt"),
    )?;
    let mut generated = BTreeMap::new();
    for entry in crate::packfs::WalkDir::new(root.join("assets/minecraft")) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry
            .path()
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        let own = name.starts_with("assets/minecraft/optifine/cit/fastpack_builtin_hud/")
            || name.starts_with("assets/minecraft/models/item/fastpack_builtin_hud_cit/")
            || name.starts_with("assets/minecraft/textures/item/fastpack_builtin_hud_cit/")
            || name.starts_with("assets/minecraft/items/")
                && fs::read(entry.path())
                    .ok()
                    .and_then(|data| serde_json::from_slice::<Value>(&data).ok())
                    .is_some_and(|v| v.get("fastpack_builtin_hud").is_some());
        if own {
            generated.insert(name, digest(&fs::read(entry.path())?));
        }
    }
    write(
        root,
        STATE,
        &serde_json::to_vec(&Provenance {
            settings: settings.clone(),
            generated,
        })?,
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "builtin_overlays_tests.rs"]
mod tests;
