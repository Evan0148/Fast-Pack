use anyhow::{Result, ensure};
use fast_pack::compat::sky::preview;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::time::Instant;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() > 1,
        "sky_preview_profile <output directory> <pack>..."
    );
    let output = Path::new(&args[0]);
    fs::create_dir_all(output)?;
    let mut results = Vec::new();
    for (index, source) in args[1..].iter().enumerate() {
        let source = Path::new(source);
        let started = Instant::now();
        let result = preview(source)?;
        let milliseconds = started.elapsed().as_secs_f64() * 1000.0;
        if let Some(result) = result {
            let hash = format!("{:x}", Sha256::digest(result.image.as_raw()));
            result.image.save(output.join(format!("{index:02}.png")))?;
            println!(
                "{}: {} ({milliseconds:.2}ms)",
                source.display(),
                result.selected_layer
            );
            results.push(serde_json::json!({"source":source,"selected":result.selected_layer,"daytime":result.daytime,"layers":result.names,"pixel_sha256":hash,"milliseconds":milliseconds}));
        } else {
            println!(
                "{}: no daytime preview ({milliseconds:.2}ms)",
                source.display()
            );
            results.push(
                serde_json::json!({"source":source,"selected":null,"milliseconds":milliseconds}),
            );
        }
    }
    fs::write(
        output.join("preview-results.json"),
        serde_json::to_vec_pretty(&results)?,
    )?;
    Ok(())
}
