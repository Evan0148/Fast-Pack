use anyhow::{Result, ensure};
use fast_pack::probe_pack;
use serde_json::json;
use std::fs;
use std::path::Path;
use std::time::Instant;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(args.len() >= 2, "probe_profile <output.json> <pack>...");
    let mut results = Vec::new();
    for source in &args[1..] {
        let source = Path::new(source);
        let mut samples = Vec::new();
        let mut initial = None;
        for _ in 0..16 {
            let started = Instant::now();
            let info = probe_pack(source);
            let elapsed = started.elapsed().as_secs_f64() * 1000.0;
            ensure!(info.ok, "{}: {:?}", source.display(), info.error);
            let value = serde_json::to_value(&info)?;
            if let Some(initial) = &initial {
                ensure!(*initial == value, "识别结果在重复读取时发生变化");
            } else {
                initial = Some(value);
            }
            samples.push(elapsed);
        }
        let first = samples[0];
        let mut sorted = samples.clone();
        sorted.sort_by(f64::total_cmp);
        let p50 = sorted[sorted.len() / 2];
        let p95 = sorted[(sorted.len() * 95 / 100).min(sorted.len() - 1)];
        let maximum = sorted.last().copied().unwrap();
        println!(
            "{}: first={first:.3}ms p50={p50:.3}ms p95={p95:.3}ms max={maximum:.3}ms",
            source.display()
        );
        results.push(json!({"source":source,"first_ms":first,"p50_ms":p50,"p95_ms":p95,"max_ms":maximum,"samples_ms":samples,"info":initial}));
    }
    fs::write(&args[0], serde_json::to_vec_pretty(&results)?)?;
    Ok(())
}
