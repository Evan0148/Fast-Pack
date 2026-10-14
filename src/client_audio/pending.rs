use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize)]
struct Pending {
    client: PathBuf,
    source: PathBuf,
    destination: PathBuf,
    state: String,
}

#[cfg(windows)]
pub(super) fn running(client: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    let path = client.join("logs/latest.log");
    if !path.is_file() {
        return false;
    }
    OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(path)
        .is_err_and(|error| matches!(error.raw_os_error(), Some(32) | Some(33)))
}

#[cfg(not(windows))]
pub(super) fn running(_: &Path) -> bool {
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

pub(super) fn schedule(client: &Path, source: &Path, destination: &Path) -> Result<()> {
    let folder = folder()?;
    fs::create_dir_all(&folder)?;
    let destination = destination.canonicalize()?;
    let digest = format!(
        "{:x}",
        Sha256::digest(format!("{}|{}", client.display(), destination.display()).as_bytes())
    );
    let path = folder.join(format!("{digest}.json"));
    let pending = Pending {
        client: client.canonicalize()?,
        source: source.canonicalize()?,
        destination,
        state: "waiting-for-game-exit".into(),
    };
    write(&path, &pending)?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("--finish-heypixel")
        .arg(&path)
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

pub fn finish_pending() -> Result<bool> {
    let mut arguments = std::env::args_os().skip(1);
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new("--finish-heypixel")) {
        return Ok(false);
    }
    let path = PathBuf::from(arguments.next().context("缺少待处理任务")?).canonicalize()?;
    let folder = folder()?.canonicalize()?;
    ensure!(
        path.parent() == Some(folder.as_path())
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
    let start = Instant::now();
    loop {
        let mut pending: Pending = serde_json::from_slice(&fs::read(&path)?)?;
        if pending.state == "complete" {
            break;
        }
        if !running(&pending.client) {
            let result = (|| -> Result<()> {
                ensure!(
                    pending.client.join("heypixel").is_dir()
                        && super::compatible_version(&pending.client),
                    "客户端不符合待处理任务"
                );
                super::install(&pending.client)?;
                let (_temporary, root) = crate::converter::load_pack(&pending.destination)?;
                super::deployment::activate(
                    &pending.client,
                    &pending.source,
                    &pending.destination,
                    &root,
                )?;
                super::discover::remember(&pending.client)?;
                Ok(())
            })();
            match result {
                Ok(()) => {
                    pending.state = "complete".into();
                    write(&path, &pending)?;
                    break;
                }
                Err(error) => {
                    pending.state = format!("retrying: {error:#}");
                    write(&path, &pending)?;
                }
            }
        }
        if start.elapsed() > Duration::from_secs(14400) {
            break;
        }
        std::thread::sleep(Duration::from_secs(3));
    }
    drop(handle);
    let _ = fs::remove_file(lock);
    Ok(true)
}
