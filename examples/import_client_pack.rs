use anyhow::{Context, Result};
use std::path::PathBuf;

fn main() -> Result<()> {
    if fast_pack::client_audio::finish_pending()? {
        return Ok(());
    }
    let mut args = std::env::args_os().skip(1).map(PathBuf::from);
    let client = args
        .next()
        .context("用法: import_client_pack <Minecraft目录> <原包> <转换结果>")?;
    let source = args.next().context("请指定原包")?;
    let output = args.next().context("请指定转换结果")?;
    let (_temporary, root) = fast_pack::converter::load_pack(&output)?;
    let imported = fast_pack::client_audio::import_resource_pack(&client, &source, &output, &root)?;
    println!("{}", serde_json::to_string_pretty(&imported)?);
    Ok(())
}
