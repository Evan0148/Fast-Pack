use anyhow::{Context, Result, ensure};
use std::path::PathBuf;
use std::process::Command;

fn main() -> Result<()> {
    let target_dir = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target-win7"));
    let raw_data = std::env::args_os()
        .skip(2)
        .any(|argument| argument == "--raw-data");
    let mut command = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("RUSTC_BOOTSTRAP", "1")
        .args([
            "-Z",
            "build-std=std,panic_abort",
            "build",
            "--release",
            "--locked",
            "--target",
            "x86_64-win7-windows-msvc",
            "--bins",
            "--target-dir",
        ])
        .arg(&target_dir);
    if raw_data {
        command.arg("--no-default-features");
    }
    let status = command.status().context("无法启动 Windows 兼容构建")?;
    ensure!(
        status.success(),
        "Windows 兼容构建失败，请确认已安装 rust-src"
    );
    println!(
        "Windows 7 SP1 / 10 / 11 x64: {}",
        target_dir
            .join("x86_64-win7-windows-msvc/release")
            .display()
    );
    Ok(())
}
