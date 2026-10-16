use anyhow::{Result, ensure};
use fast_pack::batch::{Event, Job, Pool, output_path, parallelism};
use fast_pack::{ConversionOptions, pack_format_of};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

fn main() -> Result<()> {
    let mut arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let workers = if arguments.first().is_some_and(|value| value == "--serial") {
        arguments.remove(0);
        1
    } else {
        parallelism()
    };
    ensure!(
        arguments.len() >= 3,
        "用法: batch_profile <目标版本> <导出目录> <材质包>..."
    );
    let target = arguments[0].to_string_lossy().to_string();
    let directory = PathBuf::from(&arguments[1]);
    let inputs: Vec<PathBuf> = arguments[2..].iter().map(PathBuf::from).collect();
    let options = ConversionOptions {
        tgt_format: pack_format_of(&target)?,
        target_version: Some(target.clone()),
        auto_client_setup: false,
        ..Default::default()
    };
    let (sender, receiver) = mpsc::channel();
    let started = Instant::now();
    let pool = Pool::new(workers, move |event| {
        let _ = sender.send(event);
    });
    let mut outputs = Vec::new();
    for (id, source) in inputs.iter().enumerate() {
        let output = output_path(
            source,
            &target,
            true,
            Some(&directory),
            &outputs,
            &inputs,
            true,
        );
        outputs.push(output.clone());
        pool.submit(Job {
            id: id as u64,
            source: source.clone(),
            output,
            options: options.clone(),
        })?;
    }
    let mut finished = 0;
    let mut active = 0;
    let mut peak = 0;
    let mut failed = 0;
    while finished < inputs.len() {
        match receiver.recv()? {
            Event::Started(_) => {
                active += 1;
                peak = peak.max(active);
            }
            Event::Finished(id, result) => {
                active -= 1;
                finished += 1;
                if result.is_err() {
                    failed += 1;
                }
                println!(
                    "{}",
                    serde_json::json!({"id":id,"ok":result.is_ok(),"elapsed_ms":started.elapsed().as_millis(),"error":result.err()})
                );
            }
            _ => {}
        }
    }
    println!(
        "{}",
        serde_json::json!({"packs":inputs.len(),"failed":failed,"peak_parallel":peak,"total_ms":started.elapsed().as_millis()})
    );
    ensure!(failed == 0, "部分材质包转换失败");
    Ok(())
}
