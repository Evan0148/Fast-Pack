use anyhow::{Result, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use zip::ZipArchive;

fn canonical(value: Value) -> Value {
    match value {
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .map(|(name, value)| (name, canonical(value)))
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(canonical).collect()),
        Value::Number(number)
            if number
                .as_f64()
                .is_some_and(|n| n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0) =>
        {
            Value::from(number.as_f64().unwrap() as i64)
        }
        value => value,
    }
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 2,
        "modern_default_catalog <reference jars.json> <output.json>"
    );
    let jars: BTreeMap<String, String> = serde_json::from_slice(&fs::read(&args[0])?)?;
    let mut result = BTreeMap::<String, BTreeSet<String>>::new();
    for (_, jar) in jars {
        let mut archive = ZipArchive::new(File::open(jar)?)?;
        for index in 0..archive.len() {
            let mut file = archive.by_index(index)?;
            let name = file.name().to_string();
            if !name.ends_with(".json")
                || !name.starts_with("assets/minecraft/models/")
                    && !name.starts_with("assets/minecraft/blockstates/")
            {
                continue;
            }
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            let value: Value = serde_json::from_slice(&bytes)?;
            let digest = format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&canonical(value))?)
            );
            result.entry(name).or_default().insert(digest);
        }
    }
    fs::write(&args[1], serde_json::to_vec(&result)?)?;
    println!(
        "resources={}, fingerprints={}",
        result.len(),
        result.values().map(BTreeSet::len).sum::<usize>()
    );
    Ok(())
}
