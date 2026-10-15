use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

type Files = BTreeMap<String, Vec<u8>>;

fn jar(path: &Path) -> Result<Files> {
    let mut archive = ZipArchive::new(File::open(path)?)?;
    let mut files = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.is_file() && entry.name().starts_with("assets/") {
            let name = entry.name().to_string();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            files.insert(name, bytes);
        }
    }
    Ok(files)
}

fn folder(path: &Path) -> Result<Files> {
    let mut files = BTreeMap::new();
    for entry in walkdir::WalkDir::new(path.join("assets")).min_depth(1) {
        let entry = entry?;
        if entry.file_type().is_file() {
            files.insert(
                entry
                    .path()
                    .strip_prefix(path)?
                    .to_string_lossy()
                    .replace('\\', "/"),
                fs::read(entry.path())?,
            );
        }
    }
    Ok(files)
}

fn id(name: &str) -> String {
    if name.contains(':') {
        name.into()
    } else {
        format!("minecraft:{name}")
    }
}

fn models(files: &Files) -> BTreeMap<String, Value> {
    files
        .iter()
        .filter_map(|(name, bytes)| {
            let (namespace, model) = name.strip_prefix("assets/")?.split_once("/models/")?;
            if namespace.contains('/') {
                return None;
            }
            Some((
                format!("{namespace}:{}", model.strip_suffix(".json")?),
                serde_json::from_slice(bytes).ok()?,
            ))
        })
        .collect()
}

fn canonical(value: Value) -> Value {
    match value {
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .map(|(key, value)| (key, canonical(value)))
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(canonical).collect()),
        Value::Number(number) if number.as_f64().is_some_and(|n| n.fract() == 0.0) => {
            json!(number.as_f64().unwrap() as i64)
        }
        value => value,
    }
}

fn resolve(name: &str, values: &BTreeMap<String, Value>, seen: &mut BTreeSet<String>) -> Value {
    if !seen.insert(name.into()) || seen.len() > 64 {
        return json!({});
    }
    let Some(value) = values.get(name) else {
        return json!({});
    };
    let mut out = value["parent"]
        .as_str()
        .map(|parent| resolve(&id(parent), values, seen))
        .unwrap_or_else(|| json!({}));
    for field in ["textures", "display"] {
        if let Some(fields) = value[field].as_object() {
            if !out[field].is_object() {
                out[field] = json!({});
            }
            out[field].as_object_mut().unwrap().extend(fields.clone());
        }
    }
    for field in ["elements", "ambientocclusion"] {
        if let Some(field_value) = value.get(field) {
            out[field] = field_value.clone();
        }
    }
    seen.remove(name);
    out
}

fn transform(value: &Value) -> Value {
    json!({"rotation":value.get("rotation").cloned().unwrap_or_else(|| json!([0,0,0])),"translation":value.get("translation").cloned().unwrap_or_else(|| json!([0,0,0])),"scale":value.get("scale").cloned().unwrap_or_else(|| json!([1,1,1]))})
}

fn geometry(value: &Value) -> Value {
    let mut elements = value["elements"].clone();
    if let Some(elements) = elements.as_array_mut() {
        for element in elements {
            if let Some(faces) = element["faces"].as_object_mut() {
                for face in faces.values_mut() {
                    face.as_object_mut().unwrap().remove("texture");
                }
            }
        }
    }
    canonical(elements)
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 5,
        "audit_legacy_assets <legacy.jar> <modern.jar> <source folder> <converted folder> <report.json>"
    );
    let legacy_files = jar(Path::new(&args[0]))?;
    let modern_files = jar(Path::new(&args[1]))?;
    let source_files = folder(Path::new(&args[2]))?;
    let output_files = folder(Path::new(&args[3]))?;
    let legacy = models(&legacy_files);
    let modern = models(&modern_files);
    let source = models(&source_files);
    let mut output = legacy.clone();
    output.extend(models(&output_files));
    let mut combined = modern.clone();
    combined.extend(source.clone());
    let catalog: Value = serde_json::from_slice(include_bytes!("../assets/legacy-models.json"))?;
    let mut stock = Vec::new();
    let mut custom = Vec::new();
    for (name, value) in &source {
        if modern
            .get(name)
            .is_some_and(|reference| canonical(reference.clone()) == canonical(value.clone()))
        {
            stock.push(name.clone());
        } else {
            custom.push(json!({"model":name,"provided":value,"reference":modern.get(name)}));
        }
    }
    let mut poses = Vec::new();
    let mut shapes = Vec::new();
    let mut missing = Vec::new();
    for name in legacy.keys() {
        let expected = resolve(name, &legacy, &mut BTreeSet::new());
        let actual = resolve(name, &output, &mut BTreeSet::new());
        let alias = catalog["aliases"][name].as_str().unwrap_or(name);
        let input = resolve(alias, &combined, &mut BTreeSet::new());
        let baseline = resolve(alias, &modern, &mut BTreeSet::new());
        let unchanged = canonical(input.clone()) == canonical(baseline);
        if name.starts_with("minecraft:item/") {
            for view in ["gui", "firstperson", "thirdperson"] {
                let a = canonical(transform(&actual["display"][view]));
                let b = canonical(transform(&expected["display"][view]));
                if a != b {
                    poses.push(json!({"model":name,"view":view,"actual":a,"expected":b,"source_stock":unchanged}));
                }
            }
        }
        if geometry(&expected) != geometry(&actual) {
            shapes.push(json!({"model":name,"source_stock":unchanged,"actual_elements":actual["elements"].as_array().map_or(0, Vec::len),"expected_elements":expected["elements"].as_array().map_or(0, Vec::len)}));
        }
    }
    for (name, _) in &legacy_files {
        let Some(texture) = name
            .strip_prefix("assets/minecraft/textures/")
            .and_then(|n| n.strip_suffix(".png"))
        else {
            continue;
        };
        let Some((kind, short)) = texture.split_once('/') else {
            continue;
        };
        let modern_kind = match kind {
            "blocks" => "block",
            "items" => "item",
            _ => kind,
        };
        let modern_name = fast_pack::flatten_texture_name(short, modern_kind);
        let reference = format!("assets/minecraft/textures/{modern_kind}/{modern_name}.png");
        if source_files.contains_key(&reference) && !output_files.contains_key(name) {
            missing.push(json!({"legacy":name,"source":reference}));
        }
    }
    let stock_pose_issues = poses.iter().filter(|p| p["source_stock"] == true).count();
    let stock_shape_issues = shapes.iter().filter(|p| p["source_stock"] == true).count();
    let mut image_errors = Vec::new();
    let mut images_checked = 0;
    let mut metadata_errors = Vec::new();
    for (name, bytes) in &output_files {
        if name.ends_with(".png") {
            images_checked += 1;
            if let Err(error) = image::load_from_memory(bytes) {
                image_errors.push(json!({"file":name,"error":error.to_string()}));
            }
        } else if name.ends_with(".png.mcmeta") {
            if let Err(error) = serde_json::from_slice::<Value>(bytes) {
                metadata_errors.push(json!({"file":name,"error":error.to_string()}));
            }
        }
    }
    let report = json!({"source_models":source.len(),"stock_models":stock.len(),"custom_models":custom,"legacy_models":legacy.len(),"pose_issues":poses,"shape_issues":shapes,"stock_pose_issues":stock_pose_issues,"stock_shape_issues":stock_shape_issues,"missing_mapped_textures":missing,"images_checked":images_checked,"image_errors":image_errors,"metadata_errors":metadata_errors});
    fs::write(&args[4], serde_json::to_vec_pretty(&report)?)?;
    println!(
        "stock={}, custom={}, poses={}, shapes={}, missing_mapped_textures={}",
        stock.len(),
        report["custom_models"].as_array().unwrap().len(),
        stock_pose_issues,
        stock_shape_issues,
        report["missing_mapped_textures"].as_array().unwrap().len()
    );
    Ok(())
}
