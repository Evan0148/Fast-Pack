use anyhow::{Context, Result, ensure};
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

fn main() -> Result<()> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        !arguments.is_empty(),
        "用法: startup_profile <FastPack.exe> [次数]"
    );
    let runs = arguments
        .get(1)
        .and_then(|value| value.to_str())
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(5)
        .clamp(1, 30);
    let work = tempfile::tempdir()?;
    let mut values = Vec::new();
    for index in 0..runs {
        let trace = work.path().join(format!("startup-{index}.jsonl"));
        let launch = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs_f64() * 1000.0;
        let started = Instant::now();
        let mut command = Command::new(&arguments[0]);
        command
            .env(
                "FAST_PACK_CAPTURE",
                work.path().join(format!("startup-{index}.png")),
            )
            .env("FAST_PACK_STARTUP_TIMING", &trace)
            .env("FAST_PACK_STARTUP_LAUNCH", launch.to_string());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().context("无法启动程序")?;
        loop {
            if let Some(status) = child.try_wait()? {
                ensure!(status.success(), "程序启动失败: {status}");
                break;
            }
            if started.elapsed().as_secs() >= 15 {
                child.kill()?;
                anyhow::bail!("启动超时")
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let events: Vec<serde_json::Value> = std::fs::read_to_string(&trace)?
            .lines()
            .map(serde_json::from_str)
            .collect::<std::result::Result<_, _>>()?;
        let event = events
            .iter()
            .find(|event| event["stage"] == "first_frame")
            .context("未收到首帧事件")?;
        let elapsed = event["launch_milliseconds"]
            .as_f64()
            .context("未记录进程启动时间")?;
        println!(
            "{}",
            serde_json::json!({"run":index+1,"first_frame_ms":elapsed,"events":events})
        );
        values.push(elapsed);
    }
    values.sort_by(f64::total_cmp);
    println!(
        "{}",
        serde_json::json!({"runs":runs,"minimum_ms":values[0],"median_ms":values[values.len()/2],"maximum_ms":values[runs-1]})
    );
    Ok(())
}
