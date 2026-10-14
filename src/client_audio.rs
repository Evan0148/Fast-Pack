mod deployment;
mod discover;
mod mapping_config;
mod pending;

pub use pending::finish_pending;
pub use pending::{Imported, deploy as import_resource_pack};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;
use zip::ZipArchive;

const MARKER: &str = "fastpack/client-audio-bridge.json";
const SOUND_CLASSES: &[&str] = &[
    "com/viaversion/viaversion/rewriter/SoundRewriter.class",
    "com/viaversion/viabackwards/api/rewriters/SoundRewriter.class",
];
const MAPPINGS: &[&str] = &["mappings-1.20.5to1.20.3.nbt", "mappings-1.21to1.20.5.nbt"];

#[derive(Debug, Serialize)]
pub struct Configuration {
    pub client: PathBuf,
    pub files: Vec<PathBuf>,
    pub translated_names: usize,
    pub mod_sha256: String,
    pub mods_modified: bool,
    pub original_sounds_enabled: bool,
    pub next_launch_required: bool,
}

#[derive(Deserialize)]
struct LegacyInstallation {
    backup: PathBuf,
    original_sha256: String,
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
    for entry in fs::read_dir(mods)? {
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
        if SOUND_CLASSES
            .iter()
            .all(|name| archive.by_name(name).is_ok())
        {
            let Ok(mut entry) = archive.by_name("META-INF/mods.toml") else {
                continue;
            };
            let mut metadata = String::new();
            entry.read_to_string(&mut metadata)?;
            if metadata.contains("viaforge") {
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

pub fn install(client: &Path) -> Result<Configuration> {
    let client = client.canonicalize().context("无法打开客户端目录")?;
    ensure!(
        compatible_version(&client),
        "本功能仅适用于网易 1.20.1 客户端"
    );
    let jar = find(&client)?;
    let before = hash(&jar)?;
    let mut archive = ZipArchive::new(File::open(&jar)?)?;
    let data = client.join("heypixel/ViaForge/ViaLoader");
    ensure!(
        data.join("viabackwards.yml").is_file(),
        "未找到 ViaBackwards 用户配置目录"
    );
    let mut jobs = vec![];
    let mut total = 0;
    for name in MAPPINGS {
        let mut bytes = vec![];
        archive
            .by_name(&format!("assets/viabackwards/data/{name}"))?
            .read_to_end(&mut bytes)?;
        let original = mapping_config::read(&bytes)?;
        let changes = mapping_config::official_names(&original);
        if changes.is_empty() {
            continue;
        }
        let path = data.join(name);
        let mut values = if path.is_file() {
            mapping_config::read(&fs::read(&path)?)?
        } else {
            std::collections::BTreeMap::new()
        };
        total += mapping_config::merge(&mut values, changes)?;
        jobs.push((path, mapping_config::write(&values)?));
    }
    ensure!(
        total >= 5,
        "这份 ViaBackwards 未提供已核对的五个名称配置入口"
    );
    drop(archive);
    let mut files = vec![];
    for (path, bytes) in jobs {
        if path.is_file() && fs::read(&path)? == bytes {
            files.push(path);
            continue;
        }
        if path.is_file() {
            let backup = client
                .join("heypixel/fastpack-backups/mapping-config")
                .join(format!(
                    "{}-{}",
                    hash(&path)?,
                    path.file_name().unwrap().to_string_lossy()
                ));
            if !backup.exists() {
                atomic_copy(&path, &backup)?;
            }
        }
        let mut temporary = NamedTempFile::new_in(&data)?;
        temporary.write_all(&bytes)?;
        temporary.as_file().sync_all()?;
        temporary.persist(&path).map_err(|error| error.error)?;
        files.push(path);
    }
    ensure!(hash(&jar)? == before, "模组在配置处理期间变化");
    Ok(Configuration {
        client,
        files,
        translated_names: total,
        mod_sha256: before,
        mods_modified: false,
        original_sounds_enabled: false,
        next_launch_required: true,
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
        report.warnings.push(
            "布吉岛适配：兼容资源已导出，本机未找到网易 1.20.1 布吉岛客户端，未自动配置汉化或导入"
                .into(),
        );
        return;
    };
    report.warnings.push("布吉岛原声尚未启用：启动器会校验并还原模组，资源包不能恢复被本地协议合并或取消的声音；需要服务器保留原声事件或客户端提供受支持的音效扩展".into());
    progress("配置布吉岛官方名称汉化");
    match install(client) {
        Ok(configuration)=>report.generated.push(format!("布吉岛汉化：通过 ViaBackwards 用户映射配置写入 {} 个官方名称，保持模组原始文件不变，下一次启动时读取",configuration.translated_names)),
        Err(error)=>report.warnings.push(format!("布吉岛汉化配置未完成：{error:#}；转换包已正常导出")),
    }
    progress("导入布吉岛材质包，保留现有选择");
    match import_resource_pack(client, source, destination, converted) {
        Ok(imported) if imported.queued => report.generated.push(format!("布吉岛客户端：自动定位 {}，游戏正在运行；退出后自动导入最近一次转换结果，仅替换已经启用的原包，保留玩家后续选择", client.display())),
        Ok(imported) if imported.enabled => report.generated.push(format!("布吉岛材质包：已导入 {}，在原位置更新已启用的材质包，保留其他包的启用状态与顺序", imported.path.display())),
        Ok(imported) => report.generated.push(format!("布吉岛材质包：已导入 {}，保留现有已加载列表，没有额外启用资源包", imported.path.display())),
        Err(error) => report
            .warnings
            .push(format!("布吉岛材质包自动导入未完成：{error:#}")),
    }
    if let Err(error) = discover::remember(client) {
        report
            .warnings
            .push(format!("布吉岛定位缓存未保存：{error:#}"));
    }
}

pub fn restore(client: &Path) -> Result<PathBuf> {
    let client = client.canonicalize()?;
    let path = find(&client)?;
    let mut archive = ZipArchive::new(File::open(&path)?)?;
    let mut bytes = vec![];
    if let Ok(mut entry) = archive.by_name(MARKER) {
        entry.read_to_end(&mut bytes)?;
    } else {
        bail!("当前模组已是原版，没有旧 Fast Pack 改写需要恢复");
    }
    let installation: LegacyInstallation = serde_json::from_slice(&bytes)?;
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
