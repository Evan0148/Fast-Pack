use anyhow::{Result, ensure};
use serde_json::json;
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 3,
        "compare_packs <before.zip> <after.zip> <report.json>"
    );
    let mut before = ZipArchive::new(File::open(&args[0])?)?;
    let mut after = ZipArchive::new(File::open(&args[1])?)?;
    let a: BTreeSet<_> = before.file_names().map(str::to_string).collect();
    let b: BTreeSet<_> = after.file_names().map(str::to_string).collect();
    let missing: Vec<_> = a.difference(&b).cloned().collect();
    let added: Vec<_> = b.difference(&a).cloned().collect();
    let mut different = Vec::new();
    let mut same = 0;
    for name in a.intersection(&b) {
        let mut left = Vec::new();
        let mut right = Vec::new();
        before.by_name(name)?.read_to_end(&mut left)?;
        after.by_name(name)?.read_to_end(&mut right)?;
        if left == right {
            same += 1;
            continue;
        }
        if name.ends_with(".png") {
            if let (Ok(a), Ok(b)) = (
                image::load_from_memory(&left),
                image::load_from_memory(&right),
            ) {
                if a.to_rgba8() == b.to_rgba8() {
                    same += 1;
                    continue;
                }
            }
        }
        if name.ends_with(".json") {
            if let (Ok(a), Ok(b)) = (
                serde_json::from_slice::<serde_json::Value>(&left),
                serde_json::from_slice::<serde_json::Value>(&right),
            ) {
                if a == b {
                    same += 1;
                    continue;
                }
            }
        }
        different.push(name.clone());
    }
    let result = json!({"before":Path::new(&args[0]),"after":Path::new(&args[1]),"before_files":a.len(),"after_files":b.len(),"same":same,"missing":missing,"added":added,"different":different});
    fs::write(&args[2], serde_json::to_vec_pretty(&result)?)?;
    println!(
        "files={} -> {}, same={}, missing={}, added={}, different={}",
        a.len(),
        b.len(),
        same,
        missing.len(),
        added.len(),
        different.len()
    );
    Ok(())
}
