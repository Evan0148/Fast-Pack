use anyhow::{Result, ensure};
use fast_pack::{ConversionOptions, convert_pack, pack_format_of};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn main() -> Result<()> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        arguments.len() >= 3,
        "conversion_profile <输出目录> <目标版本> <源包>..."
    );
    let directory = Path::new(&arguments[0]);
    fs::create_dir_all(directory)?;
    let target = arguments[1].to_string_lossy();
    let options = ConversionOptions {
        tgt_format: pack_format_of(&target)?,
        target_version: Some(target.to_string()),
        auto_client_setup: false,
        ..Default::default()
    };
    let mut results = Vec::new();
    for (index, source) in arguments[2..].iter().enumerate() {
        let source = PathBuf::from(source);
        let output = directory.join(format!("{index:02}.zip"));
        let timer = Instant::now();
        let mut previous_time = 0.0;
        let mut previous_step = String::new();
        let mut stages = Vec::new();
        let report = convert_pack(&source, &output, &options, |step, _| {
            let current = timer.elapsed().as_secs_f64();
            if !previous_step.is_empty() {
                stages.push(json!({"step": previous_step, "seconds": current - previous_time}));
            }
            previous_step = step.to_string();
            previous_time = current;
        })?;
        let seconds = timer.elapsed().as_secs_f64();
        stages.push(json!({"step": "收尾与临时文件清理", "seconds": seconds - previous_time}));
        fs::write(
            directory.join(format!("{index:02}-conversion.json")),
            serde_json::to_vec_pretty(&report)?,
        )?;
        results.push(json!({"source": source, "output": output, "seconds": seconds, "bytes": fs::metadata(&output)?.len(), "stages": stages}));
        println!("{}: {:.3}s", source.display(), seconds);
    }
    fs::write(
        directory.join("profile.json"),
        serde_json::to_vec_pretty(&results)?,
    )?;
    Ok(())
}
