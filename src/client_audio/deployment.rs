use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

fn preserve(client: &Path, path: &Path) -> Result<()> {
    if path.is_file() {
        let digest = super::hash(path)?;
        let name = path
            .file_name()
            .context("备份文件名无效")?
            .to_string_lossy();
        let backup = client
            .join("heypixel/fastpack-backups/resource-packs")
            .join(format!("{digest}-{name}"));
        if !backup.exists() {
            super::atomic_copy(path, &backup)?;
        }
    }
    Ok(())
}

fn options(client: &Path, name: &str, source: &Path) -> Result<()> {
    let path = client.join("heypixel/options.txt");
    let original = if path.is_file() {
        fs::read_to_string(&path)?
    } else {
        String::new()
    };
    let active = format!("file/{name}");
    let source_name = source.file_name().unwrap_or_default().to_string_lossy();
    let source_id = format!("file/{source_name}");
    let newline = if original.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut lines = vec![];
    let mut saw_packs = false;
    for line in original.lines() {
        if let Some(values) = line.strip_prefix("resourcePacks:") {
            ensure!(!saw_packs, "客户端资源包设置存在重复项");
            saw_packs = true;
            let mut packs: Vec<Value> =
                serde_json::from_str(values).context("客户端资源包列表无效，原设置未修改")?;
            packs.retain(|value| {
                value.as_str() != Some(&active) && value.as_str() != Some(&source_id)
            });
            if !packs.iter().any(|value| value.as_str() == Some("vanilla")) {
                packs.insert(0, Value::String("vanilla".into()));
            }
            packs.push(Value::String(active.clone()));
            lines.push(format!("resourcePacks:{}", serde_json::to_string(&packs)?));
        } else if let Some(values) = line.strip_prefix("incompatibleResourcePacks:") {
            let mut packs: Vec<Value> =
                serde_json::from_str(values).context("客户端资源包兼容列表无效，原设置未修改")?;
            packs.retain(|value| value.as_str() != Some(&active));
            lines.push(format!(
                "incompatibleResourcePacks:{}",
                serde_json::to_string(&packs)?
            ));
        } else {
            lines.push(line.to_string());
        }
    }
    if !saw_packs {
        lines.push(format!(
            "resourcePacks:{}",
            serde_json::to_string(&vec!["vanilla".to_string(), active])?
        ));
    }
    let output = lines.join(newline) + newline;
    if output == original {
        return Ok(());
    }
    preserve(client, &path)?;
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    temporary.write_all(output.as_bytes())?;
    temporary.as_file().sync_all()?;
    ensure!(
        !path.exists() || fs::read_to_string(&path)? == original,
        "客户端设置在导入期间变化，未修改"
    );
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

pub(super) fn activate(
    client: &Path,
    source: &Path,
    destination: &Path,
    root: &Path,
) -> Result<PathBuf> {
    let folder = client.join("heypixel/packs");
    fs::create_dir_all(&folder)?;
    let name = if destination
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
    {
        destination
            .file_name()
            .context("导出文件名无效")?
            .to_string_lossy()
            .into_owned()
    } else {
        format!(
            "{}.zip",
            destination
                .file_name()
                .context("导出目录名称无效")?
                .to_string_lossy()
        )
    };
    let path = folder.join(&name);
    if source.canonicalize().ok().as_ref() == Some(&path.canonicalize().unwrap_or(path.clone())) {
        anyhow::bail!("导入名称会覆盖原始材质包，已保留原包与导出文件");
    }
    if destination.is_file() {
        let same = path.is_file() && super::hash(&path)? == super::hash(destination)?;
        if !same {
            preserve(client, &path)?;
            super::atomic_copy(destination, &path)?;
        }
    } else {
        let mut temporary = tempfile::NamedTempFile::new_in(&folder)?;
        {
            let mut zip = ZipWriter::new(temporary.as_file_mut());
            let settings =
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            for entry in WalkDir::new(root).sort_by_file_name() {
                let entry = entry?;
                if !entry.file_type().is_file() {
                    continue;
                }
                let name = entry
                    .path()
                    .strip_prefix(root)?
                    .to_string_lossy()
                    .replace('\\', "/");
                zip.start_file(name, settings)?;
                std::io::copy(&mut File::open(entry.path())?, &mut zip)?;
            }
            zip.finish()?;
        }
        temporary.as_file_mut().sync_all()?;
        preserve(client, &path)?;
        temporary.persist(&path).map_err(|error| error.error)?;
    }
    options(client, &name, source)?;
    Ok(path)
}
