include!("../src/client_audio/mapping_config.rs");

fn main() -> Result<()> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("需要映射文件"))?;
    let values = read(&std::fs::read(path)?)?;
    for (group, value) in official_names(&values) {
        println!("{group}: {value:?}");
    }
    Ok(())
}
