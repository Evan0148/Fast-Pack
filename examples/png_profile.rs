use anyhow::Result;
use image::ImageEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use std::fs::{self, File};
use std::io::Read;
use std::time::Instant;
use zip::ZipArchive;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let mut archive = ZipArchive::new(File::open(&args[0])?)?;
    let name = archive
        .file_names()
        .find(|name| {
            name.starts_with("assets/fastpack/textures/sky/converted_") && name.ends_with(".png")
        })
        .unwrap()
        .to_string();
    let mut bytes = Vec::new();
    archive.by_name(&name)?.read_to_end(&mut bytes)?;
    let image = image::load_from_memory(&bytes)?.to_rgba8();
    let mut results = Vec::new();
    for compression in [
        CompressionType::Fast,
        CompressionType::Level(1),
        CompressionType::Level(3),
        CompressionType::Default,
    ] {
        let mut output = Vec::new();
        let time = Instant::now();
        PngEncoder::new_with_quality(&mut output, compression, FilterType::Adaptive).write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgba8,
        )?;
        let seconds = time.elapsed().as_secs_f64();
        println!("{compression:?}: {seconds:.3}s {} bytes", output.len());
        results.push(serde_json::json!({"compression":format!("{compression:?}"),"seconds":seconds,"bytes":output.len(),"original_bytes":bytes.len()}));
    }
    fs::write(&args[1], serde_json::to_vec_pretty(&results)?)?;
    Ok(())
}
