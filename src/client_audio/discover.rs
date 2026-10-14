use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn cache() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .map(|folder| folder.join("Fast Pack/heypixel-clients.json"))
}

fn candidate(path: &Path, roots: &mut BTreeSet<PathBuf>) {
    for suffix in [
        "",
        ".minecraft",
        "Game/.minecraft",
        "MCLDownload/Game/.minecraft",
        "Game",
        "heypixel",
    ] {
        let root = path.join(suffix);
        if root.join("heypixel").is_dir()
            && root.join("mods").is_dir()
            && super::compatible_version(&root)
        {
            if let Ok(root) = root.canonicalize() {
                roots.insert(root);
            }
        }
    }
}

#[cfg(windows)]
fn registered() -> Vec<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegGetValueW,
    };
    let mut paths = vec![];
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        for key in [
            "Software\\Netease\\MCLauncher",
            "Software\\WOW6432Node\\Netease\\MCLauncher",
        ] {
            let key: Vec<_> = key.encode_utf16().chain(Some(0)).collect();
            for name in [
                "DownloadPath",
                "InstallLocation",
                "MinecraftPath",
                "GamePath",
            ] {
                let name: Vec<_> = name.encode_utf16().chain(Some(0)).collect();
                let mut bytes = 0;
                let result = unsafe {
                    RegGetValueW(
                        hive,
                        key.as_ptr(),
                        name.as_ptr(),
                        RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        &mut bytes,
                    )
                };
                if result != 0 || bytes == 0 || bytes > 65536 {
                    continue;
                }
                let mut value = vec![0u16; bytes as usize / 2 + 1];
                let result = unsafe {
                    RegGetValueW(
                        hive,
                        key.as_ptr(),
                        name.as_ptr(),
                        RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
                        std::ptr::null_mut(),
                        value.as_mut_ptr().cast(),
                        &mut bytes,
                    )
                };
                if result == 0 {
                    let end = value
                        .iter()
                        .position(|value| *value == 0)
                        .unwrap_or(value.len());
                    let path = PathBuf::from(OsString::from_wide(&value[..end]));
                    if path.is_absolute() {
                        paths.push(path);
                    }
                }
            }
        }
    }
    paths
}

#[cfg(not(windows))]
fn registered() -> Vec<PathBuf> {
    vec![]
}

#[cfg(windows)]
fn drives() -> Vec<PathBuf> {
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    let mask = unsafe { GetLogicalDrives() };
    (0..26)
        .filter(|index| mask & (1 << index) != 0)
        .filter_map(|index| {
            let path = format!("{}:\\", (b'A' + index as u8) as char);
            let wide: Vec<_> = path.encode_utf16().chain(Some(0)).collect();
            (unsafe { GetDriveTypeW(wide.as_ptr()) } == 3).then(|| PathBuf::from(path))
        })
        .collect()
}

#[cfg(not(windows))]
fn drives() -> Vec<PathBuf> {
    vec![]
}

fn found(roots: BTreeSet<PathBuf>) -> Vec<PathBuf> {
    let mut roots: Vec<_> = roots
        .into_iter()
        .filter(|root| super::find(root).is_ok())
        .collect();
    roots.sort_by_key(|root| {
        std::cmp::Reverse(
            fs::metadata(root.join("heypixel/options.txt"))
                .and_then(|metadata| metadata.modified())
                .ok(),
        )
    });
    roots
}

pub(super) fn find(source: &Path, destination: &Path) -> Vec<PathBuf> {
    let mut roots = BTreeSet::new();
    if let Some(path) = std::env::var_os("FAST_PACK_HEYPIXEL_CLIENT") {
        candidate(&PathBuf::from(path), &mut roots);
        return found(roots);
    }
    for path in registered() {
        candidate(&path, &mut roots);
    }
    if !roots.is_empty() {
        let current = found(roots.clone());
        if !current.is_empty() {
            return current;
        }
    }
    for path in [source, destination] {
        for parent in path.ancestors().take(8) {
            candidate(parent, &mut roots);
        }
    }
    if let Some(Value::Array(paths)) = cache()
        .and_then(|path| fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
    {
        for path in paths
            .into_iter()
            .filter_map(|value| value.as_str().map(PathBuf::from))
        {
            candidate(&path, &mut roots);
        }
    }
    for name in [
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "ProgramFiles",
        "ProgramFiles(x86)",
    ] {
        if let Some(path) = std::env::var_os(name) {
            let path = PathBuf::from(path);
            for suffix in [
                "",
                "NetEase/MCLauncher",
                "Minecraft",
                "MC",
                "Games",
                "Documents",
            ] {
                candidate(&path.join(suffix), &mut roots);
            }
        }
    }
    let mut queue = VecDeque::new();
    for drive in drives() {
        for suffix in ["MCLDownload", "Games/MCLDownload", "NetEase/MCLDownload"] {
            candidate(&drive.join(suffix), &mut roots);
        }
        queue.push_back((drive, 0));
    }
    let current = found(roots.clone());
    if !current.is_empty() {
        return current;
    }
    let started = Instant::now();
    let mut visited = 0;
    while let Some((path, depth)) = queue.pop_front() {
        if visited >= 12000 || started.elapsed() > Duration::from_secs(8) {
            break;
        }
        visited += 1;
        candidate(&path, &mut roots);
        if !roots.is_empty() {
            let current = found(roots.clone());
            if !current.is_empty() {
                return current;
            }
        }
        if depth >= 5 {
            continue;
        }
        let Ok(entries) = fs::read_dir(path) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if [
                "windows",
                "$recycle.bin",
                "system volume information",
                "node_modules",
                ".git",
                "target",
                "target-native",
                "target-win7",
                "libraries",
                "assets",
                "cache",
                "fastpack-backups",
            ]
            .contains(&name.as_str())
            {
                continue;
            }
            if entry
                .file_type()
                .is_ok_and(|kind| kind.is_dir() && !kind.is_symlink())
            {
                queue.push_back((entry.path(), depth + 1));
            }
        }
    }
    found(roots)
}

pub(super) fn remember(root: &Path) -> Result<()> {
    let path = cache().context("无法找到用户配置目录")?;
    let mut roots: Vec<PathBuf> = fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    roots.retain(|path| path != root && path.is_dir());
    roots.insert(0, root.into());
    roots.truncate(8);
    let folder = path.parent().unwrap();
    fs::create_dir_all(folder)?;
    let mut temporary = tempfile::NamedTempFile::new_in(folder)?;
    temporary.write_all(&serde_json::to_vec_pretty(&roots)?)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}
