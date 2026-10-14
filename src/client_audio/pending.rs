use super::deployment::Selection;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

#[derive(Serialize, Deserialize)]
struct Pending {
    client: PathBuf,
    source: PathBuf,
    destination: PathBuf,
    state: String,
    #[serde(default)]
    schema: u32,
    #[serde(default)]
    selection: Selection,
    #[serde(default)]
    output_sha256: String,
}

impl Pending {
    fn finished(&self) -> bool {
        self.state == "complete"
            || self.state.starts_with("cancelled")
            || self.state.starts_with("failed")
    }
}

#[derive(Debug, Serialize)]
pub struct Imported {
    pub path: PathBuf,
    pub queued: bool,
    pub enabled: bool,
}

#[cfg(windows)]
fn running(client: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    ["logs/latest.log", "heypixel/logs/latest.log"]
        .into_iter()
        .any(|name| {
            let path = client.join(name);
            path.is_file()
                && OpenOptions::new()
                    .read(true)
                    .share_mode(0)
                    .open(path)
                    .is_err_and(|error| matches!(error.raw_os_error(), Some(32) | Some(33)))
        })
}

#[cfg(not(windows))]
fn running(_: &Path) -> bool {
    false
}

fn folder() -> Result<PathBuf> {
    let root = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .context("无法找到用户配置目录")?;
    Ok(root.join("Fast Pack/pending-heypixel"))
}

fn write(path: &Path, value: &Pending) -> Result<()> {
    let mut temporary =
        tempfile::NamedTempFile::new_in(path.parent().context("待处理任务缺少目录")?)?;
    temporary.write_all(&serde_json::to_vec_pretty(value)?)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn fingerprint(path: &Path) -> Result<String> {
    if path.is_file() {
        return super::hash(path);
    }
    ensure!(path.is_dir(), "转换结果已经不存在");
    let mut digest = Sha256::new();
    for entry in WalkDir::new(path).sort_by_file_name() {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry
            .path()
            .strip_prefix(path)?
            .to_string_lossy()
            .replace('\\', "/");
        digest.update((name.len() as u64).to_le_bytes());
        digest.update(name.as_bytes());
        digest.update(super::hash(entry.path())?.as_bytes());
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn client_lock(client: &Path, directory: &Path) -> Result<File> {
    let key = format!(
        "{:x}",
        Sha256::digest(client.to_string_lossy().to_ascii_lowercase().as_bytes())
    );
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    let path = directory.join(format!("client-{key}.lock"));
    for _ in 0..200 {
        match options.open(&path) {
            Ok(file) => return Ok(file),
            Err(error) if matches!(error.raw_os_error(), Some(32) | Some(33)) => {
                std::thread::sleep(Duration::from_millis(25))
            }
            Err(error) => return Err(error.into()),
        }
    }
    anyhow::bail!("客户端已有资源包导入正在完成，请稍后重试")
}

fn cancel_older(directory: &Path, client: Option<&Path>) -> Result<()> {
    if !directory.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let Ok(mut pending) = serde_json::from_slice::<Pending>(&bytes) else {
            continue;
        };
        if pending.finished() {
            continue;
        }
        if client.is_some_and(|client| {
            pending
                .client
                .to_string_lossy()
                .eq_ignore_ascii_case(&client.to_string_lossy())
        }) {
            pending.state = "cancelled-superseded".into();
        } else if client.is_none() && pending.schema < 2 {
            pending.state = "cancelled-legacy-selection".into();
        } else {
            continue;
        }
        let backup = path.with_extension("cancelled-backup");
        if !backup.exists() {
            fs::write(backup, bytes)?;
        }
        write(&path, &pending)?;
    }
    Ok(())
}

fn spawn(path: &Path) -> Result<()> {
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("--finish-heypixel")
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command.spawn().context("无法启动布吉岛自动处理任务")?;
    Ok(())
}

pub fn deploy(
    client: &Path,
    source: &Path,
    destination: &Path,
    converted: &Path,
) -> Result<Imported> {
    let client = client.canonicalize()?;
    ensure!(
        client.join("heypixel").is_dir() && super::compatible_version(&client),
        "本功能仅处理网易 1.20.1 布吉岛客户端"
    );
    let directory = folder()?;
    fs::create_dir_all(&directory)?;
    let _guard = client_lock(&client, &directory)?;
    let destination = destination.canonicalize()?;
    let source = source.canonicalize().unwrap_or_else(|_| source.to_owned());
    let output_sha256 = fingerprint(&destination)?;
    let selection = super::deployment::selection(&client)?;
    cancel_older(&directory, Some(&client))?;
    let key = format!(
        "{:x}",
        Sha256::digest(
            format!(
                "{}|{}|{}|{}",
                client.display(),
                destination.display(),
                std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
            )
            .as_bytes()
        )
    );
    let path = directory.join(format!("{key}.json"));
    let mut pending = Pending {
        client: client.clone(),
        source,
        destination: destination.clone(),
        state: "waiting-for-game-exit".into(),
        schema: 2,
        selection,
        output_sha256,
    };
    write(&path, &pending)?;
    let imported = client
        .join("heypixel/packs")
        .join(super::deployment::name(&destination)?);
    if running(&client) {
        spawn(&path)?;
        return Ok(Imported {
            path: imported,
            queued: true,
            enabled: false,
        });
    }
    match super::deployment::activate(
        &client,
        &pending.source,
        &destination,
        converted,
        &pending.selection,
    ) {
        Ok((pack, enabled)) => {
            pending.state = "complete".into();
            write(&path, &pending)?;
            Ok(Imported {
                path: pack,
                queued: false,
                enabled,
            })
        }
        Err(error) => {
            pending.state = format!("failed: {error:#}");
            write(&path, &pending)?;
            Err(error)
        }
    }
}

pub fn finish_pending() -> Result<bool> {
    let mut arguments = std::env::args_os().skip(1);
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new("--finish-heypixel")) {
        if let Ok(directory) = folder() {
            let _ = cancel_older(&directory, None);
        }
        return Ok(false);
    }
    let path = PathBuf::from(arguments.next().context("缺少待处理任务")?).canonicalize()?;
    let directory = folder()?.canonicalize()?;
    ensure!(
        path.parent() == Some(directory.as_path())
            && path
                .extension()
                .is_some_and(|extension| extension == "json"),
        "待处理任务不在用户配置目录"
    );
    let lock = path.with_extension("lock");
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    let Ok(handle) = options.open(&lock) else {
        return Ok(true);
    };
    let started = Instant::now();
    loop {
        let mut pending: Pending = serde_json::from_slice(&fs::read(&path)?)?;
        if pending.finished() {
            break;
        }
        if pending.schema < 2 {
            pending.state = "cancelled-legacy-selection".into();
            write(&path, &pending)?;
            break;
        }
        if !running(&pending.client) {
            let _guard = client_lock(&pending.client, &directory)?;
            pending = serde_json::from_slice(&fs::read(&path)?)?;
            if pending.finished() {
                break;
            }
            let output_check = fingerprint(&pending.destination);
            if output_check
                .as_ref()
                .is_ok_and(|hash| hash != &pending.output_sha256)
            {
                pending.state = "cancelled-output-changed".into();
                write(&path, &pending)?;
                break;
            }
            let result = (|| -> Result<()> {
                output_check?;
                ensure!(
                    pending.client.join("heypixel").is_dir()
                        && super::compatible_version(&pending.client),
                    "客户端不符合待处理任务"
                );
                let _ = super::install(&pending.client);
                let (_temporary, root) = crate::converter::load_pack(&pending.destination)?;
                super::deployment::activate(
                    &pending.client,
                    &pending.source,
                    &pending.destination,
                    &root,
                    &pending.selection,
                )?;
                Ok(())
            })();
            pending.state = match result {
                Ok(()) => "complete".into(),
                Err(error) => format!("retrying: {error:#}"),
            };
            write(&path, &pending)?;
            if pending.state == "complete" {
                break;
            }
        }
        if started.elapsed() > Duration::from_secs(14400) {
            pending.state = "cancelled-timeout".into();
            write(&path, &pending)?;
            break;
        }
        std::thread::sleep(Duration::from_secs(3));
    }
    drop(handle);
    let _ = fs::remove_file(lock);
    Ok(true)
}
