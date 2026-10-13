use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    for entry in WalkDir::new(source).min_depth(1) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let output = destination.join(entry.path().strip_prefix(source)?);
        fs::create_dir_all(output.parent().unwrap())?;
        fs::copy(entry.path(), output)?;
    }
    Ok(())
}

fn digest(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65_536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn archive(source: &Path, output: &Path) -> Result<()> {
    let mut files = WalkDir::new(source)
        .min_depth(1)
        .into_iter()
        .collect::<std::result::Result<Vec<_>, _>>()?;
    files.sort_by(|left, right| left.path().cmp(right.path()));
    let mut zip = ZipWriter::new(File::create(output)?);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for entry in files {
        if entry.file_type().is_file() {
            let name = entry
                .path()
                .strip_prefix(source)?
                .to_string_lossy()
                .replace('\\', "/");
            zip.start_file(name, options)?;
            std::io::copy(&mut File::open(entry.path())?, &mut zip)?;
        }
    }
    zip.finish()?;
    Ok(())
}

fn main() -> Result<()> {
    let arguments: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    ensure!(
        arguments.len() == 3,
        "用法: release_bundle <release 二进制目录> <发布目录> <第三方许可证目录>"
    );
    let binaries = &arguments[0];
    let releases = &arguments[1];
    let licenses = &arguments[2];
    ensure!(
        binaries.join("FastPack.exe").is_file(),
        "请先构建 FastPack.exe"
    );
    ensure!(
        binaries.join("fast-pack-cli.exe").is_file(),
        "请先构建命令行程序"
    );
    ensure!(licenses.is_dir(), "第三方许可证目录不存在");
    let version = env!("CARGO_PKG_VERSION");
    let stage = releases.join(format!("v{version}"));
    let portable = stage.join("portable");
    fs::create_dir_all(&portable)?;
    let main_name = format!("FastPack-v{version}.exe");
    let cli_name = format!("fast-pack-cli-v{version}.exe");
    for (original, name) in [
        ("FastPack.exe", &main_name),
        ("fast-pack-cli.exe", &cli_name),
    ] {
        fs::copy(binaries.join(original), stage.join(name)).context("无法复制发布程序")?;
        fs::copy(binaries.join(original), portable.join(name))?;
    }
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("LICENSE"),
        portable.join("LICENSE"),
    )?;
    copy_tree(licenses, &portable.join("third-party-licenses"))?;
    fs::write(
        portable.join("README.txt"),
        format!(
            "Fast Pack v{version}\nWindows: Windows 7 SP1 / Windows 10 / Windows 11, x64\nDevelopers: ColdEternity Team, Long_Cold, Wind_\nSource: https://github.com/LongCold-ColdEternity-Team/Fast-Pack\nLicense: ColdEternity Team Source-Available License 1.0. Free non-commercial use only.\nGUI: {main_name}\nCLI: {cli_name} 9178.zip --to 1.20.1\nQQ: 663420633; Answer: 快手/B站/抖音\n"
        ),
    )?;
    let archive_name = format!("FastPack-v{version}-windows-x64.zip");
    archive(&portable, &stage.join(&archive_name))?;
    let mut sums = String::new();
    for name in [&main_name, &cli_name, &archive_name] {
        sums.push_str(&format!("{}  {name}\n", digest(&stage.join(name))?));
    }
    fs::write(stage.join("SHA256SUMS.txt"), sums)?;
    println!("Fast Pack v{version}: {}", stage.display());
    Ok(())
}
