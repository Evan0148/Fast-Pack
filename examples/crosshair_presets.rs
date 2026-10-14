use anyhow::{Context, Result, bail, ensure};
use fast_pack::crosshair::{Generator, Store, Style};
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let directory = args.next().context("用法: crosshair_presets <预设目录> list | import <包或PNG>... | delete <编号> | generate <名字> <cross|dot|ring|diamond> [颜色]")?;
    let store = Store::new(PathBuf::from(directory));
    match args.next().as_deref().unwrap_or("list") {
        "list" => {
            for preset in store.load()? {
                println!(
                    "{}\t{}\t{}x{}",
                    preset.id,
                    preset.crosshair.name,
                    preset.crosshair.image.width(),
                    preset.crosshair.image.height()
                );
            }
        }
        "import" => {
            let paths: Vec<_> = args.map(PathBuf::from).collect();
            ensure!(!paths.is_empty(), "请指定准星包或 PNG");
            for path in paths {
                for crosshair in fast_pack::crosshair::import(&path)? {
                    println!("{}\t{}", store.save(&crosshair)?, crosshair.name);
                }
            }
        }
        "delete" => store.delete(&args.next().context("请指定准星编号")?)?,
        "restore" => store.restore_builtins()?,
        "draw" => {
            let name = args.next().context("请指定预设名字")?;
            let path = args.next().context("请指定画布 JSON 文件")?;
            let canvas: fast_pack::crosshair::Canvas =
                serde_json::from_slice(&std::fs::read(path)?)?;
            ensure!(!canvas.is_empty(), "画布不能为空");
            let crosshair = canvas.render(&name);
            println!("{}\t{}", store.save(&crosshair)?, crosshair.name);
        }
        "generate" => {
            let name = args.next().context("请指定预设名字")?;
            let style = match args.next().as_deref().unwrap_or("cross") {
                "cross" => Style::Cross,
                "dot" => Style::Dot,
                "ring" => Style::Ring,
                "diamond" => Style::Diamond,
                value => bail!("未知准星样式: {value}"),
            };
            let color = args
                .next()
                .map(|value| fast_pack::compat::glint::parse_color(&value))
                .transpose()?
                .unwrap_or([255; 3]);
            let mut crosshair = Generator {
                style,
                color,
                ..Default::default()
            }
            .render();
            crosshair.name = name;
            println!("{}\t{}", store.save(&crosshair)?, crosshair.name);
        }
        value => bail!("未知操作: {value}"),
    }
    Ok(())
}
