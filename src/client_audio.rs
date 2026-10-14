mod classfile;
mod deployment;
mod pending;

pub use pending::finish_pending;
mod discover;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const MARKER: &str = "fastpack/client-audio-bridge.json";
const ITEM_CLASS: &str = "com/viaversion/viabackwards/api/rewriters/BackwardsItemRewriter.class";
const CLASSES: &[(&str, bool)] = &[
    (
        "com/viaversion/viaversion/rewriter/SoundRewriter.class",
        false,
    ),
    (
        "com/viaversion/viabackwards/api/rewriters/SoundRewriter.class",
        true,
    ),
];

#[derive(Debug, Serialize, Deserialize)]
pub struct Installation {
    pub schema: u32,
    pub jar: PathBuf,
    pub backup: PathBuf,
    pub original_sha256: String,
    pub events: Vec<String>,
    pub restart_required: bool,
}

fn hash(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn find(client: &Path) -> Result<PathBuf> {
    let mods = client.join("mods");
    ensure!(mods.is_dir(), "未找到客户端 mods 目录");
    let mut candidates = vec![];
    for entry in fs::read_dir(&mods)? {
        let path = entry?.path();
        if !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("jar"))
        {
            continue;
        }
        let Ok(mut archive) = ZipArchive::new(File::open(&path)?) else {
            continue;
        };
        if CLASSES
            .iter()
            .all(|(name, _)| archive.by_name(name).is_ok())
        {
            let Ok(mut entry) = archive.by_name("META-INF/mods.toml") else {
                continue;
            };
            let mut text = String::new();
            entry.read_to_string(&mut text)?;
            if text.contains("viaforge") {
                candidates.push(path);
            }
        }
    }
    ensure!(
        candidates.len() == 1,
        "需要唯一的 ViaForge 模组，实际找到 {} 个",
        candidates.len()
    );
    Ok(candidates.remove(0))
}

pub(super) fn compatible_version(client: &Path) -> bool {
    let Ok(versions) = fs::read_dir(client.join("versions")) else {
        return false;
    };
    versions.filter_map(Result::ok).any(|version| {
        let path = version.path();
        path.is_dir()
            && fs::read_dir(path)
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .any(|entry| {
                    entry
                        .path()
                        .extension()
                        .is_some_and(|extension| extension == "json")
                        && fs::read(entry.path())
                            .ok()
                            .and_then(|bytes| {
                                serde_json::from_slice::<serde_json::Value>(&bytes).ok()
                            })
                            .is_some_and(|value| {
                                value["id"] == "1.20.1" || value["inheritsFrom"] == "1.20.1"
                            })
                })
    })
}

pub fn automatic(
    source: &Path,
    destination: &Path,
    converted: &Path,
    report: &mut crate::compat::assets::AssetReport,
    mut progress: impl FnMut(&str),
) {
    progress("查找网易布吉岛客户端");
    let clients = discover::find(source, destination);
    let Some(client) = clients.first() else {
        report.warnings.push("布吉岛适配：新版物品和音效已加入导出包，本机未找到网易 1.20.1 布吉岛客户端，未安装本地声音适配或自动导入".into());
        return;
    };
    if pending::running(client) {
        match pending::schedule(client,source,destination) {
            Ok(()) => report.generated.push(format!("布吉岛客户端：自动定位 {}，游戏正在运行；已安排在游戏退出后自动安装原声适配、导入并启用转换包，无需重复转换",client.display())),
            Err(error) => report.warnings.push(format!("布吉岛客户端自动处理排队失败：{error:#}；转换包已正常导出")),
        }
        return;
    }
    progress("安装布吉岛原声音效适配");
    match install(client) {
        Ok(installed) => {
            report.generated.push(format!(
                "布吉岛客户端：自动定位 {}，原声及名称附魔汉化已就绪，原模组备份在 {}",
                client.display(),
                installed.backup.display()
            ));
        }
        Err(error) => {
            report.warnings.push(format!(
                "布吉岛客户端 {}：本地声音适配未完成：{error:#}；转换包已正常导出",
                client.display()
            ));
            return;
        }
    }
    progress("导入并启用布吉岛材质包");
    match deployment::activate(client, source, destination, converted) {
        Ok(pack) => report.generated.push(format!(
            "布吉岛材质包：已自动导入并启用 {}；下次启动游戏时加载原声音效适配",
            pack.display()
        )),
        Err(error) => report.warnings.push(format!(
            "布吉岛材质包自动导入未完成：{error:#}；转换包已正常导出"
        )),
    }
    if let Err(error) = discover::remember(client) {
        report
            .warnings
            .push(format!("布吉岛客户端定位缓存未保存：{error:#}"));
    }
}

fn atomic_copy(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination.parent().context("缺少目标父目录")?)?;
    let mut temporary = NamedTempFile::new_in(destination.parent().unwrap())?;
    std::io::copy(&mut File::open(source)?, temporary.as_file_mut())?;
    temporary.as_file_mut().sync_all()?;
    temporary
        .persist(destination)
        .map_err(|error| error.error)?;
    Ok(())
}

pub fn install(client: &Path) -> Result<Installation> {
    let client = client.canonicalize().context("无法打开客户端目录")?;
    let path = find(&client)?;
    let mut archive = ZipArchive::new(File::open(&path)?)?;
    let current_sha256 = hash(&path)?;
    let mut upgrade = None;
    if let Ok(mut entry) = archive.by_name(MARKER) {
        let mut bytes = vec![];
        entry.read_to_end(&mut bytes)?;
        let installed: Installation = serde_json::from_slice(&bytes)?;
        ensure!(matches!(installed.schema, 1 | 2), "未知的声音适配器版本");
        ensure!(installed.jar == path, "声音适配器对应的模组路径已变化");
        ensure!(
            hash(&installed.backup)? == installed.original_sha256,
            "原始模组备份校验失败"
        );
        if installed.schema == 2 {
            return Ok(installed);
        }
        upgrade = Some(installed);
    }
    if let Some(installed) = upgrade.as_ref() {
        archive = ZipArchive::new(File::open(&installed.backup)?)?;
    }
    for name in archive.file_names() {
        let name = name.to_ascii_uppercase();
        ensure!(
            !(name.starts_with("META-INF/")
                && [".SF", ".RSA", ".DSA", ".EC"]
                    .iter()
                    .any(|suffix| name.ends_with(suffix))),
            "模组含数字签名，未修改"
        );
    }
    ensure!(
        archive
            .by_name(
                "com/viaversion/viabackwards/protocol/v1_20_5to1_20_3/Protocol1_20_5To1_20_3.class"
            )
            .is_ok(),
        "未找到 1.20.5 协议适配器"
    );
    ensure!(
        compatible_version(&client),
        "本适配器仅适用于已核对的网易 1.20.1 客户端"
    );
    let original_sha256 = upgrade
        .as_ref()
        .map(|installed| installed.original_sha256.clone())
        .unwrap_or_else(|| current_sha256.clone());
    let backup = client
        .join("heypixel/fastpack-backups/audio-bridge")
        .join(format!("{original_sha256}.jar"));
    let installation = Installation {
        schema: 2,
        jar: path.clone(),
        backup: backup.clone(),
        original_sha256,
        events: classfile::RULES
            .iter()
            .map(|(_, event)| format!("fastpack:{event}"))
            .collect(),
        restart_required: true,
    };
    let mut patches = std::collections::BTreeMap::new();
    for (name, backwards) in CLASSES {
        let mut original = vec![];
        archive.by_name(name)?.read_to_end(&mut original)?;
        patches.insert(
            *name,
            classfile::patch(&original, *backwards)
                .with_context(|| format!("未支持的声音处理类 {name}"))?,
        );
    }
    let mut original = vec![];
    archive.by_name(ITEM_CLASS)?.read_to_end(&mut original)?;
    patches.insert(
        ITEM_CLASS,
        classfile::patch_localization(&original).context("未支持的物品名称与附魔汉化结构")?,
    );
    let mut temporary = NamedTempFile::new_in(path.parent().unwrap())?;
    {
        let mut writer = ZipWriter::new(temporary.as_file_mut());
        for index in 0..archive.len() {
            let entry = archive.by_index(index)?;
            if let Some(bytes) = patches.get(entry.name()) {
                writer.start_file(
                    entry.name(),
                    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
                )?;
                writer.write_all(bytes)?;
            } else {
                writer.raw_copy_file(entry)?;
            }
        }
        writer.start_file(
            MARKER,
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )?;
        writer.write_all(&serde_json::to_vec_pretty(&installation)?)?;
        writer.finish()?;
    }
    temporary.as_file_mut().sync_all()?;
    drop(archive);
    if backup.exists() {
        ensure!(
            hash(&backup)? == installation.original_sha256,
            "备份文件冲突"
        );
    } else if upgrade.is_none() {
        atomic_copy(&path, &backup)?;
    } else {
        bail!("升级缺少原始模组备份");
    }
    ensure!(
        hash(&path)? == current_sha256,
        "模组在处理期间变化，已停止安装"
    );
    temporary
        .persist(&path)
        .map_err(|error| error.error)
        .context("无法替换模组；请退出游戏后重试")?;
    Ok(installation)
}

pub fn restore(client: &Path) -> Result<PathBuf> {
    let client = client.canonicalize()?;
    let path = find(&client)?;
    let mut archive = ZipArchive::new(File::open(&path)?)?;
    let mut bytes = vec![];
    if let Ok(mut entry) = archive.by_name(MARKER) {
        entry.read_to_end(&mut bytes)?;
    } else {
        bail!("未检测到 Fast Pack 声音适配器");
    }
    let installation: Installation = serde_json::from_slice(&bytes)?;
    let folder = client
        .join("heypixel/fastpack-backups/audio-bridge")
        .canonicalize()?;
    let backup = installation.backup.canonicalize()?;
    ensure!(
        backup.parent() == Some(folder.as_path()),
        "备份不在预期目录"
    );
    ensure!(
        hash(&backup)? == installation.original_sha256,
        "备份文件校验失败"
    );
    drop(archive);
    atomic_copy(&backup, &path)?;
    Ok(path)
}
