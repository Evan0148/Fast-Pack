use super::assets::{AssetReport, backup};
use anyhow::Result;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, OnceLock};

#[derive(Deserialize)]
struct Profile {
    version: String,
    models: BTreeMap<String, Value>,
    roots: BTreeSet<String>,
    textures: BTreeSet<String>,
    sprites: BTreeSet<String>,
}

struct ProfileSource {
    data: &'static str,
    parsed: OnceLock<Profile>,
}

static PROFILES: LazyLock<BTreeMap<u32, ProfileSource>> = LazyLock::new(|| {
    include_str!("../../assets/model-integrity.jsonl")
        .lines()
        .filter_map(|line| {
            let number = line
                .strip_prefix("{\"format\":")?
                .split(',')
                .next()?
                .parse()
                .ok()?;
            Some((
                number,
                ProfileSource {
                    data: line,
                    parsed: OnceLock::new(),
                },
            ))
        })
        .collect()
});

fn id(value: &str) -> Option<String> {
    let value = value.replace('\\', "/");
    let (namespace, path) = value.split_once(':').unwrap_or(("minecraft", &value));
    if namespace.is_empty()
        || path.is_empty()
        || !namespace
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c))
        || !path
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-/".contains(c))
        || path.split('/').any(|part| matches!(part, "" | "." | ".."))
    {
        return None;
    }
    Some(format!("{namespace}:{path}"))
}

fn model_path(root: &Path, name: &str) -> PathBuf {
    let (namespace, path) = name.split_once(':').unwrap();
    root.join(format!("assets/{namespace}/models/{path}.json"))
}

fn write(root: &Path, path: &Path, value: &Value) -> Result<bool> {
    let bytes = serde_json::to_vec_pretty(value)?;
    if path.is_file() {
        if fs::read(path)? == bytes {
            return Ok(false);
        }
        backup(root, path)?;
    }
    fs::create_dir_all(path.parent().unwrap())?;
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(true)
}

fn references(value: &Value, names: &mut BTreeSet<String>) {
    match value {
        Value::Object(values) => {
            for (key, value) in values {
                if key == "model" {
                    if let Some(name) = value.as_str().and_then(id) {
                        names.insert(name);
                    }
                } else {
                    references(value, names);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                references(value, names);
            }
        }
        _ => {}
    }
}

fn slots(value: &Value) -> Option<Vec<String>> {
    value.get("elements")?.as_array().map(|elements| {
        elements
            .iter()
            .flat_map(|element| {
                element["faces"]
                    .as_object()
                    .into_iter()
                    .flat_map(|faces| faces.values())
            })
            .filter_map(|face| face["texture"].as_str().map(str::to_string))
            .collect()
    })
}

#[derive(Clone, Default)]
struct Resolved {
    textures: Map<String, Value>,
    slots: Vec<String>,
    parents: Vec<String>,
    missing: BTreeSet<String>,
}

struct Graph<'a> {
    root: &'a Path,
    profile: &'a Profile,
    models: BTreeMap<String, Value>,
    roots: BTreeSet<String>,
    textures: BTreeSet<String>,
    cache: [RefCell<BTreeMap<String, Arc<Resolved>>>; 2],
}

impl Graph<'_> {
    fn resolve(&self, name: &str, native: bool, seen: &mut BTreeSet<String>) -> Arc<Resolved> {
        if name.starts_with("minecraft:builtin/") {
            return Arc::default();
        }
        if seen.len() >= 64 || !seen.insert(name.into()) {
            return Arc::new(Resolved {
                missing: BTreeSet::from([name.into()]),
                ..Default::default()
            });
        }
        let cache = &self.cache[usize::from(native)];
        if let Some(result) = cache.borrow().get(name).filter(|result| {
            seen.len() - 1 + result.parents.len() <= 64
                && result
                    .parents
                    .iter()
                    .filter(|parent| parent.as_str() != name)
                    .all(|parent| !seen.contains(parent))
        }) {
            seen.remove(name);
            return Arc::clone(result);
        }
        let provided = (!native).then(|| self.models.get(name)).flatten();
        let Some(value) = provided.or_else(|| self.profile.models.get(name)) else {
            seen.remove(name);
            return Arc::new(Resolved {
                missing: BTreeSet::from([name.into()]),
                ..Default::default()
            });
        };
        let mut result = value["parent"]
            .as_str()
            .and_then(id)
            .map(|parent| self.resolve(&parent, native, seen).as_ref().clone())
            .unwrap_or_default();
        if let Some(values) = value["textures"].as_object() {
            result.textures.extend(values.clone());
        }
        if let Some(slots) = slots(value) {
            result.slots = slots;
        } else if provided.is_none() {
            if let Some(slots) = value["slots"].as_array() {
                result.slots = slots
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect();
            }
        }
        result.parents.push(name.into());
        seen.remove(name);
        let result = Arc::new(result);
        if result.missing.is_empty() {
            cache.borrow_mut().insert(name.into(), Arc::clone(&result));
        }
        result
    }

    fn binding<'a>(&self, value: &'a str, resolved: &'a Resolved) -> Option<String> {
        let mut texture = value;
        let mut seen = BTreeSet::new();
        while let Some(key) = texture.strip_prefix('#') {
            if !seen.insert(key) {
                return None;
            }
            texture = resolved.textures.get(key)?.as_str()?;
        }
        id(texture.trim_end_matches(".png"))
    }

    fn texture_exists(&self, name: &str) -> bool {
        self.textures.contains(name)
    }

    fn used(&self, resolved: &Resolved) -> Vec<String> {
        let mut values = resolved.slots.clone();
        if !resolved.slots.is_empty()
            && resolved
                .parents
                .last()
                .is_some_and(|name| self.profile.models.contains_key(name))
        {
            values.push("#particle".into());
        }
        values.extend(
            resolved
                .textures
                .keys()
                .filter(|key| key.as_str() == "particle" || key.starts_with("layer"))
                .map(|key| format!("#{key}")),
        );
        values
    }
}

fn geometry_valid(value: &Value) -> bool {
    if value
        .get("parent")
        .is_some_and(|parent| !parent.is_string())
    {
        return false;
    }
    if value.get("textures").is_some_and(|textures| {
        !textures
            .as_object()
            .is_some_and(|textures| textures.values().all(Value::is_string))
    }) {
        return false;
    }
    let Some(elements) = value.get("elements") else {
        return true;
    };
    let Some(elements) = elements.as_array() else {
        return false;
    };
    for element in elements {
        for field in ["from", "to"] {
            let Some(values) = element[field].as_array() else {
                return false;
            };
            if values.len() != 3
                || values.iter().any(|value| {
                    value
                        .as_f64()
                        .is_none_or(|value| !value.is_finite() || !(-16.0..=32.0).contains(&value))
                })
            {
                return false;
            }
        }
        if let Some(rotation) = element.get("rotation") {
            if !matches!(rotation["axis"].as_str(), Some("x" | "y" | "z"))
                || !rotation["angle"]
                    .as_f64()
                    .is_some_and(|value| [-45.0, -22.5, 0.0, 22.5, 45.0].contains(&value))
            {
                return false;
            }
        }
        if let Some(faces) = element["faces"].as_object() {
            for face in faces.values() {
                if let Some(uv) = face.get("uv") {
                    if !uv.as_array().is_some_and(|uv| {
                        uv.len() == 4
                            && uv
                                .iter()
                                .all(|value| value.as_f64().is_some_and(f64::is_finite))
                    }) {
                        return false;
                    }
                }
                if let Some(rotation) = face.get("rotation") {
                    if !rotation
                        .as_f64()
                        .is_some_and(|value| [0.0, 90.0, 180.0, 270.0].contains(&value))
                    {
                        return false;
                    }
                }
            }
        }
    }
    true
}

fn load<'a>(root: &'a Path, profile: &'a Profile, report: &mut AssetReport) -> Result<Graph<'a>> {
    let mut graph = Graph {
        root,
        profile,
        models: BTreeMap::new(),
        roots: profile.roots.clone(),
        textures: profile.textures.clone(),
        cache: Default::default(),
    };
    let mut pending = vec![];
    let assets = root.join("assets");
    for entry in walkdir::WalkDir::new(&assets).min_depth(1) {
        let entry = entry?;
        if !entry.file_type().is_file()
            || entry
                .path()
                .extension()
                .is_none_or(|extension| extension != "json" && extension != "png")
        {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(&assets)?
            .to_string_lossy()
            .replace('\\', "/");
        if entry
            .path()
            .extension()
            .is_some_and(|extension| extension == "png")
        {
            if let Some((namespace, name)) = relative.split_once("/textures/") {
                if let Some(name) = name
                    .strip_suffix(".png")
                    .and_then(|name| id(&format!("{namespace}:{name}")))
                {
                    graph.textures.insert(name);
                }
            }
            continue;
        }
        if let Some((namespace, name)) = relative.split_once("/models/") {
            if namespace.contains('/') {
                continue;
            }
            let Some(name) = name
                .strip_suffix(".json")
                .and_then(|name| id(&format!("{namespace}:{name}")))
            else {
                continue;
            };
            let bytes = fs::read(entry.path())?;
            let parsed = serde_json::from_slice::<Value>(
                bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes),
            );
            if let Some(value) = parsed.ok().filter(|value| value.is_object()) {
                if !geometry_valid(&value) && profile.models.contains_key(&name) {
                    backup(root, entry.path())?;
                    fs::remove_file(entry.path())?;
                    report.converted.push(format!("模型自检：{} 含目标版本不接受的几何参数，已备份并使用对应原生模型，保留原包贴图",entry.path().strip_prefix(root)?.display()));
                    continue;
                }
                if name.split_once(':').unwrap().1.starts_with("item/") {
                    graph.roots.insert(name.clone());
                }
                references(&value, &mut graph.roots);
                graph.models.insert(name, value);
            } else if profile.models.contains_key(&name) {
                backup(root, entry.path())?;
                fs::remove_file(entry.path())?;
                report.converted.push(format!(
                    "模型自检：{} 的 JSON 无效，已备份并回退对应原生模型",
                    entry.path().strip_prefix(root)?.display()
                ));
            } else {
                pending.push(entry.path().strip_prefix(root)?.display().to_string());
            }
        } else if relative.contains("/blockstates/") || relative.contains("/items/") {
            if let Ok(value) = serde_json::from_slice::<Value>(&fs::read(entry.path())?) {
                references(&value, &mut graph.roots);
            }
        }
    }
    for path in pending {
        report.warnings.push(format!(
            "模型自检：{path} 无法解析且没有对应原生模型，已保留原文件"
        ));
    }
    Ok(graph)
}

fn repair(graph: &mut Graph<'_>) -> Result<usize> {
    let mut changed = BTreeSet::new();
    let mut initial = BTreeMap::new();
    for (name, value) in &graph.models {
        let mut value = value.clone();
        let mut adjusted = false;
        if !graph
            .resolve(name, false, &mut BTreeSet::new())
            .missing
            .is_empty()
        {
            if let Some(parent) = graph
                .profile
                .models
                .get(name)
                .and_then(|reference| reference.get("parent"))
                .filter(|parent| {
                    parent.as_str().and_then(id).is_some_and(|parent| {
                        parent.starts_with("minecraft:builtin/")
                            || graph.profile.models.contains_key(&parent)
                    })
                })
            {
                if value.get("parent") != Some(parent) {
                    value["parent"] = parent.clone();
                    adjusted = true;
                }
            }
        }
        if name.starts_with("minecraft:block/")
            && value.get("parent").is_none()
            && value["elements"].is_array()
            && graph
                .resolve(name, true, &mut BTreeSet::new())
                .parents
                .iter()
                .any(|parent| parent == "minecraft:block/block")
        {
            let parent = graph.profile.models[name]["parent"]
                .as_str()
                .and_then(id)
                .filter(|parent| {
                    let resolved = graph.resolve(parent, false, &mut BTreeSet::new());
                    resolved.missing.is_empty() && !resolved.parents.contains(name)
                })
                .unwrap_or_else(|| "minecraft:block/block".into());
            value["parent"] = json!(parent);
            adjusted = true;
        }
        let keys: BTreeSet<_> = value["textures"]
            .as_object()
            .into_iter()
            .flat_map(|values| values.keys().cloned())
            .collect();
        if let Some(elements) = value.get_mut("elements").and_then(Value::as_array_mut) {
            for element in elements {
                if let Some(faces) = element.get_mut("faces").and_then(Value::as_object_mut) {
                    for face in faces.values_mut() {
                        if let Some(texture) = face["texture"]
                            .as_str()
                            .filter(|texture| !texture.starts_with('#') && keys.contains(*texture))
                        {
                            if !id(texture).is_some_and(|texture| graph.texture_exists(&texture)) {
                                face["texture"] = json!(format!("#{texture}"));
                                adjusted = true;
                            }
                        }
                    }
                }
            }
        }
        if adjusted {
            initial.insert(name.clone(), value);
        }
    }
    if !initial.is_empty() {
        graph.cache[0].get_mut().clear();
    }
    for (name, value) in initial {
        write(graph.root, &model_path(graph.root, &name), &value)?;
        graph.models.insert(name.clone(), value);
        changed.insert(name);
    }
    for _ in 0..3 {
        let mut edits = BTreeMap::new();
        for name in &graph.roots {
            if !graph.profile.models.contains_key(name) {
                continue;
            }
            let resolved = graph.resolve(name, false, &mut BTreeSet::new());
            if !resolved
                .parents
                .iter()
                .any(|name| graph.models.contains_key(name))
            {
                continue;
            }
            let native = graph.resolve(name, true, &mut BTreeSet::new());
            let Some(reference) = graph.profile.models.get(name) else {
                continue;
            };
            let original = graph.models.get(name);
            let mut changed = false;
            let mut model = original.cloned().unwrap_or_else(|| {
                let mut value = reference.clone();
                value.as_object_mut().unwrap().remove("slots");
                value
            });
            let mut bindings = Map::new();
            let native_used: BTreeSet<_> = native
                .slots
                .iter()
                .filter_map(|slot| graph.binding(slot, &native))
                .collect();
            for slot in graph.used(&resolved) {
                let Some(key) = slot.strip_prefix('#') else {
                    continue;
                };
                if graph
                    .binding(&slot, &resolved)
                    .is_some_and(|texture| graph.texture_exists(&texture))
                {
                    continue;
                }
                let canonical = graph
                    .binding(&format!("#{key}"), &native)
                    .or_else(|| {
                        let alternative = match key {
                            "body" => "side",
                            "bottom" => "down",
                            "down" => "bottom",
                            "up" => "top",
                            "top" => "up",
                            "texture" => "all",
                            _ => key,
                        };
                        graph.binding(&format!("#{alternative}"), &native)
                    })
                    .or_else(|| {
                        (native_used.len() == 1).then(|| native_used.iter().next().unwrap().clone())
                    });
                if let Some(texture) = canonical.filter(|texture| graph.texture_exists(texture)) {
                    bindings.insert(key.to_string(), json!(texture));
                }
            }
            if !bindings.is_empty() {
                if original.is_none() && reference.get("slots").is_some() {
                    continue;
                }
                if !model["textures"].is_object() {
                    model["textures"] = json!({});
                }
                model["textures"].as_object_mut().unwrap().extend(bindings);
                changed = true;
            }
            if !resolved.missing.is_empty() {
                if let Some(parent) = reference.get("parent").filter(|parent| {
                    parent.as_str().is_some_and(|parent| {
                        id(parent).is_some_and(|parent| {
                            parent.starts_with("minecraft:builtin/")
                                || graph.profile.models.contains_key(&parent)
                        })
                    })
                }) {
                    if model.get("parent") != Some(parent) {
                        model["parent"] = parent.clone();
                        changed = true;
                    }
                }
            }
            if changed && original != Some(&model) && (!model.as_object().unwrap().is_empty()) {
                edits.insert(name.clone(), model);
            }
        }
        if edits.is_empty() {
            break;
        }
        graph.cache[0].get_mut().clear();
        for (name, model) in edits {
            if write(graph.root, &model_path(graph.root, &name), &model)? {
                changed.insert(name.clone());
            }
            graph.models.insert(name, model);
        }
    }
    Ok(changed.len())
}

fn sources(value: &Value, all_textures: &BTreeSet<String>, sprites: &mut BTreeSet<String>) {
    for source in value["sources"].as_array().into_iter().flatten() {
        match source["type"]
            .as_str()
            .unwrap_or("")
            .trim_start_matches("minecraft:")
        {
            "single" => {
                if let Some(name) = source["sprite"]
                    .as_str()
                    .or_else(|| source["resource"].as_str())
                    .and_then(id)
                {
                    sprites.insert(name);
                }
            }
            "directory" => {
                if let (Some(directory), Some(prefix)) =
                    (source["source"].as_str(), source["prefix"].as_str())
                {
                    for texture in all_textures {
                        let (namespace, path) = texture.split_once(':').unwrap();
                        if let Some(path) = path.strip_prefix(&format!("{directory}/")) {
                            sprites.insert(format!("{namespace}:{prefix}{path}"));
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn atlas(graph: &Graph<'_>, target: u32, report: &mut AssetReport) -> Result<usize> {
    if target < 12 {
        return Ok(0);
    }
    let available = &graph.textures;
    let mut registered = graph.profile.sprites.clone();
    for texture in available {
        let (_, path) = texture.split_once(':').unwrap();
        if path.starts_with("block/") || path.starts_with("item/") {
            registered.insert(texture.clone());
        }
    }
    let path = graph.root.join("assets/minecraft/atlases/blocks.json");
    let mut definition = if path.is_file() {
        let Ok(value) = serde_json::from_slice::<Value>(&fs::read(&path)?) else {
            report
                .warnings
                .push("图集自检：原包 blocks.json 无法解析，已保留，未覆盖".into());
            return Ok(0);
        };
        value
    } else {
        json!({"sources":[]})
    };
    if !definition["sources"].is_array() {
        report
            .warnings
            .push("图集自检：原包 blocks.json 的 sources 不是数组，已保留".into());
        return Ok(0);
    }
    sources(&definition, available, &mut registered);
    let mut needed = BTreeSet::new();
    for name in &graph.roots {
        let resolved = graph.resolve(name, false, &mut BTreeSet::new());
        if !resolved
            .parents
            .iter()
            .any(|name| graph.models.contains_key(name))
        {
            continue;
        }
        for slot in graph.used(&resolved) {
            if let Some(texture) = graph.binding(&slot, &resolved) {
                if available.contains(&texture) && !registered.contains(&texture) {
                    needed.insert(texture);
                }
            }
        }
    }
    for texture in &needed {
        definition["sources"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"minecraft:single","resource":texture,"sprite":texture}));
    }
    if !needed.is_empty() {
        write(graph.root, &path, &definition)?;
    }
    Ok(needed.len())
}

fn vector(value: &Value) -> Option<[f64; 3]> {
    let values = value.as_array()?;
    if values.len() != 3 {
        return None;
    }
    let result = [
        values[0].as_f64()?,
        values[1].as_f64()?,
        values[2].as_f64()?,
    ];
    result
        .iter()
        .all(|value| value.is_finite())
        .then_some(result)
}

fn rotate(mut point: [f64; 3], rotation: [f64; 3]) -> [f64; 3] {
    for axis in [2, 1, 0] {
        let a = (axis + 1) % 3;
        let b = (axis + 2) % 3;
        let (sin, cos) = rotation[axis].to_radians().sin_cos();
        let first = cos * point[a] - sin * point[b];
        point[b] = sin * point[a] + cos * point[b];
        point[a] = first;
    }
    point
}

fn vertices(value: &Value) -> Option<Vec<[f64; 3]>> {
    let mut points = Vec::new();
    for element in value["elements"].as_array()? {
        let from = vector(&element["from"])?;
        let to = vector(&element["to"])?;
        for x in [from[0], to[0]] {
            for y in [from[1], to[1]] {
                for z in [from[2], to[2]] {
                    let mut point = [x, y, z];
                    if let Some(element_rotation) = element.get("rotation") {
                        let origin = vector(&element_rotation["origin"])?;
                        let axis = match element_rotation["axis"].as_str()? {
                            "x" => 0,
                            "y" => 1,
                            "z" => 2,
                            _ => return None,
                        };
                        let angle = element_rotation["angle"].as_f64()?;
                        let mut rotation = [0.0; 3];
                        rotation[axis] = angle;
                        for component in 0..3 {
                            point[component] -= origin[component];
                            if component != axis && element_rotation["rescale"] == true {
                                point[component] /= angle.to_radians().cos();
                            }
                        }
                        point = rotate(point, rotation);
                        for component in 0..3 {
                            point[component] += origin[component];
                        }
                    }
                    points.push(point);
                }
            }
        }
    }
    (!points.is_empty()).then_some(points)
}

fn gui_bounds(points: &[[f64; 3]], display: &Value) -> Option<[[f64; 2]; 2]> {
    let rotation = display
        .get("rotation")
        .map(vector)
        .unwrap_or(Some([0.0; 3]))?;
    let scale = display.get("scale").map(vector).unwrap_or(Some([1.0; 3]))?;
    let translation = display
        .get("translation")
        .map(vector)
        .unwrap_or(Some([0.0; 3]))?;
    let mut minimum = [f64::INFINITY; 2];
    let mut maximum = [f64::NEG_INFINITY; 2];
    for point in points {
        let point = rotate(
            std::array::from_fn(|axis| (point[axis] - 8.0) * scale[axis]),
            rotation,
        );
        for axis in 0..2 {
            minimum[axis] = minimum[axis].min(point[axis] + translation[axis]);
            maximum[axis] = maximum[axis].max(point[axis] + translation[axis]);
        }
    }
    Some([minimum, maximum])
}

fn bed_icons(graph: &mut Graph<'_>) -> Result<usize> {
    let mut edits = BTreeMap::new();
    for (name, value) in &graph.models {
        let Some(item) = name.strip_prefix("minecraft:item/") else {
            continue;
        };
        if item != "bed" && !item.ends_with("_bed") {
            continue;
        }
        let resolved = graph.resolve(name, false, &mut BTreeSet::new());
        if !resolved
            .parents
            .iter()
            .any(|parent| parent == "minecraft:block/bed_inventory")
        {
            continue;
        }
        let Some(points) = resolved
            .parents
            .iter()
            .rev()
            .find_map(|parent| graph.models.get(parent).and_then(vertices))
        else {
            continue;
        };
        let gui = resolved
            .parents
            .iter()
            .rev()
            .find_map(|parent| graph.models.get(parent)?.get("display")?.get("gui"))
            .cloned()
            .unwrap_or_else(|| json!({}));
        let Some([minimum, maximum]) = gui_bounds(&points, &gui) else {
            continue;
        };
        let rotation = gui.get("rotation").and_then(vector).unwrap_or([0.0; 3]);
        let edge_on = rotation[0].to_radians().sin().abs() < 0.1
            && rotation[2].to_radians().sin().abs() < 0.1;
        if !edge_on && (0..2).all(|axis| minimum[axis] >= -7.75 && maximum[axis] <= 7.75) {
            continue;
        }
        let rotation = [30.0, 225.0, 0.0];
        let [minimum, maximum] = gui_bounds(&points, &json!({"rotation": rotation})).unwrap();
        let span = (maximum[0] - minimum[0]).max(maximum[1] - minimum[1]);
        if !span.is_finite() || span <= f64::EPSILON {
            continue;
        }
        let scale = 14.0 / span;
        let translation = [
            -(minimum[0] + maximum[0]) * 0.5 * scale,
            -(minimum[1] + maximum[1]) * 0.5 * scale,
            0.0,
        ];
        let mut model = value.clone();
        if !model["display"].is_object() {
            model["display"] = json!({});
        }
        model["display"]["gui"] = json!({"rotation": rotation, "translation": translation, "scale": [scale, scale, scale]});
        edits.insert(name.clone(), model);
    }
    let count = edits.len();
    if count > 0 {
        graph.cache[0].get_mut().clear();
    }
    for (name, value) in edits {
        write(graph.root, &model_path(graph.root, &name), &value)?;
        graph.models.insert(name, value);
    }
    Ok(count)
}

pub(super) fn migrate(root: &Path, target: u32, report: &mut AssetReport) -> Result<()> {
    if target < 4 || !root.join("assets").is_dir() {
        return Ok(());
    }
    let Some((_, reference)) = PROFILES.range(..=target).next_back() else {
        return Ok(());
    };
    let profile = reference
        .parsed
        .get_or_init(|| serde_json::from_str(reference.data).expect("模型自检参考表无效"));
    let mut graph = load(root, profile, report)?;
    let repaired = repair(&mut graph)?;
    let beds = bed_icons(&mut graph)?;
    if beds > 0 {
        report.converted.push(format!("床物品显示：为 {beds} 个旧版立体床图标修正背包视角，按模型实际尺寸居中并缩放，保留几何、UV、贴图与手持姿态"));
    }
    let sprites = atlas(&graph, target, report)?;
    let mut unresolved = 0;
    for name in &graph.roots {
        let resolved = graph.resolve(name, false, &mut BTreeSet::new());
        if !resolved
            .parents
            .iter()
            .any(|name| graph.models.contains_key(name))
        {
            continue;
        }
        let unresolved_slots: Vec<_> = graph
            .used(&resolved)
            .into_iter()
            .filter(|slot| {
                graph
                    .binding(slot, &resolved)
                    .is_none_or(|texture| !graph.texture_exists(&texture))
            })
            .collect();
        if !unresolved_slots.is_empty() || !resolved.missing.is_empty() {
            unresolved += 1;
            if unresolved <= 12 {
                report.warnings.push(format!(
                    "模型自检：{name} 仍有无法根据现有资源确定的贴图或父模型（{}；{}），已保留自定义内容",
                    unresolved_slots.join(", "),
                    resolved.missing.iter().cloned().collect::<Vec<_>>().join(", ")
                ));
            }
        }
    }
    if repaired + sprites > 0 {
        report.converted.push(format!("模型与图集自检：参考官方 {}，自动修复 {repaired} 个模型的缺失贴图绑定或父模型，为 {sprites} 张实际使用的贴图补齐图集登记；保留有效自定义几何与贴图",profile.version));
    }
    Ok(())
}
