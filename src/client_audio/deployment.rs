use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use zip::ZipWriter;

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

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct Selection {
    packs: Option<Vec<String>>,
    incompatible: Option<Vec<String>>,
}

pub(super) fn selection(client: &Path) -> Result<Selection> {
    let path = client.join("heypixel/options.txt");
    let original = if path.is_file() {
        fs::read_to_string(path)?
    } else {
        String::new()
    };
    let mut selection = Selection::default();
    for line in original.lines() {
        if let Some(values) = line.strip_prefix("resourcePacks:") {
            ensure!(selection.packs.is_none(), "客户端资源包设置存在重复项");
            selection.packs =
                Some(serde_json::from_str(values).context("客户端资源包列表无效，原设置未修改")?);
        } else if let Some(values) = line.strip_prefix("incompatibleResourcePacks:") {
            ensure!(
                selection.incompatible.is_none(),
                "客户端资源包兼容设置存在重复项"
            );
            selection.incompatible = Some(
                serde_json::from_str(values).context("客户端资源包兼容列表无效，原设置未修改")?,
            );
        }
    }
    Ok(selection)
}

fn source_matches(client: &Path, source: &Path) -> bool {
    let Some(name) = source.file_name() else {
        return false;
    };
    let installed = client.join("heypixel/packs").join(name);
    if let (Ok(source), Ok(installed)) = (source.canonicalize(), installed.canonicalize()) {
        if source == installed {
            return true;
        }
    }
    source.is_file()
        && installed.is_file()
        && super::hash(source)
            .ok()
            .zip(super::hash(&installed).ok())
            .is_some_and(|(source, installed)| source == installed)
}

fn options(client: &Path, name: &str, source: &Path, expected: &Selection) -> Result<bool> {
    let path = client.join("heypixel/options.txt");
    let original = if path.is_file() {
        fs::read_to_string(&path)?
    } else {
        String::new()
    };
    let active = format!("file/{name}");
    let source_name = source.file_name().unwrap_or_default().to_string_lossy();
    let source_id = format!("file/{source_name}");
    let current = selection(client)?;
    if &current != expected {
        return Ok(false);
    }
    let Some(mut packs) = current.packs.clone() else {
        return Ok(false);
    };
    let already_active = packs.iter().any(|value| value == &active);
    let replace = !already_active
        && packs.iter().any(|value| value == &source_id)
        && source_matches(client, source);
    if !replace && !already_active {
        return Ok(false);
    }
    if replace {
        let mut kept_active = false;
        packs = packs
            .into_iter()
            .filter_map(|id| {
                let id = if id == source_id { active.clone() } else { id };
                if id == active {
                    if kept_active {
                        return None;
                    }
                    kept_active = true;
                }
                Some(id)
            })
            .collect();
    }
    let mut incompatible = current.incompatible.clone();
    if let Some(values) = &mut incompatible {
        values.retain(|id| id != &active && (!replace || id != &source_id));
    }
    if Some(&packs) == current.packs.as_ref() && incompatible == current.incompatible {
        return Ok(true);
    }
    let newline = if original.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut lines = vec![];
    for line in original.lines() {
        if line.starts_with("resourcePacks:") {
            lines.push(format!("resourcePacks:{}", serde_json::to_string(&packs)?));
        } else if line.starts_with("incompatibleResourcePacks:") {
            lines.push(format!(
                "incompatibleResourcePacks:{}",
                serde_json::to_string(incompatible.as_ref().unwrap())?
            ));
        } else {
            lines.push(line.to_string());
        }
    }
    let output = lines.join(newline)
        + if original.ends_with('\n') {
            newline
        } else {
            ""
        };
    if output == original {
        return Ok(true);
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
    Ok(true)
}

pub(super) fn name(destination: &Path) -> Result<String> {
    let name = destination
        .file_name()
        .context("导出文件名无效")?
        .to_string_lossy();
    Ok(
        if destination
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
        {
            name.into_owned()
        } else {
            format!("{name}.zip")
        },
    )
}

pub(super) fn activate(
    client: &Path,
    source: &Path,
    destination: &Path,
    root: &Path,
    expected: &Selection,
) -> Result<(PathBuf, bool)> {
    let folder = client.join("heypixel/packs");
    fs::create_dir_all(&folder)?;
    let name = name(destination)?;
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
            let mut zip =
                ZipWriter::new(crate::archive::ArchiveWriter::new(temporary.as_file_mut())?);
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
                zip.start_file(name, crate::converter::archive_options(entry.path()))?;
                std::io::copy(&mut File::open(entry.path())?, &mut zip)?;
            }
            zip.finish()?.flush()?;
        }
        temporary.as_file_mut().sync_all()?;
        preserve(client, &path)?;
        temporary.persist(&path).map_err(|error| error.error)?;
    }
    let enabled = options(client, &name, source, expected)?;
    Ok((path, enabled))
}
