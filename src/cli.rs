use crate::converter::load_extra_renames;
use crate::{ConversionOptions, convert_pack, pack_format_of, probe_pack, suggest_output, tables};
use anyhow::{Context, Result};
use clap::Parser;
use std::io::Write;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "fast-pack",
    version,
    about = "Fast Pack · Minecraft Java 材质包转换器"
)]
struct Arguments {
    #[arg(value_name = "SRC", help = "源 ZIP 或材质包文件夹")]
    source: Option<PathBuf>,
    #[arg(
        value_name = "OUT",
        help = "输出 ZIP 或文件夹，省略时按 [目标版本]原名自动命名"
    )]
    output: Option<PathBuf>,
    #[arg(long = "from", default_value = "auto", help = "源版本或 auto")]
    source_version: String,
    #[arg(long = "to", default_value = "26.2", help = "目标版本")]
    target_version: String,
    #[arg(long, help = "输出为文件夹")]
    folder: bool,
    #[arg(long, help = "材质包描述，默认沿用原包")]
    desc: Option<String>,
    #[arg(long, help = "仅输出最终结果")]
    quiet: bool,
    #[arg(long, help = "仅识别材质包并输出 JSON")]
    probe: bool,
    #[arg(long, help = "列出支持的版本")]
    versions: bool,
    #[arg(long, help = "自定义纹理名称映射 JSON 文件")]
    renames: Option<PathBuf>,
    #[arg(long, help = "关闭小写规范化")]
    no_lowercase: bool,
    #[arg(long, help = "关闭语言文件转换")]
    no_lang: bool,
    #[arg(long, help = "将结构化转换报告另存为 JSON")]
    report_json: Option<PathBuf>,
}

pub fn run() -> Result<()> {
    let args = Arguments::parse();
    if args.versions {
        for (label, format) in &tables().pack_formats {
            println!("{label}\t{format}");
        }
        return Ok(());
    }
    let source = args
        .source
        .context("请指定源材质包，使用 --help 查看用法")?;
    if args.probe {
        let info = probe_pack(&source);
        println!("{}", serde_json::to_string_pretty(&info)?);
        anyhow::ensure!(
            info.ok,
            "{}",
            info.error
                .unwrap_or_else(|| "没有找到 assets 或 pack.mcmeta".into())
        );
        return Ok(());
    }
    let output = args
        .output
        .unwrap_or_else(|| suggest_output(&source, &args.target_version, !args.folder));
    let as_zip = if output
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
    {
        true
    } else {
        !args.folder && !output.is_dir()
    };
    let options = ConversionOptions {
        src_format: if args.source_version == "auto" {
            None
        } else {
            Some(pack_format_of(&args.source_version)?)
        },
        tgt_format: pack_format_of(&args.target_version)?,
        target_version: Some(args.target_version.clone()),
        description: args.desc,
        as_zip,
        lowercase: !args.no_lowercase,
        convert_lang: !args.no_lang,
        extra_renames: args
            .renames
            .as_deref()
            .map(load_extra_renames)
            .transpose()?
            .unwrap_or_default(),
    };
    let report = convert_pack(&source, &output, &options, |step, percent| {
        if !args.quiet {
            eprint!("\r  [{percent:3}%] {step:<22}");
            let _ = std::io::stderr().flush();
        }
    })?;
    if !args.quiet {
        eprintln!("\n");
        println!("{}", report.render());
    }
    if let Some(path) = args.report_json {
        if let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(&report)?)?;
    }
    println!("✔ Fast Pack 完成：{}", output.display());
    Ok(())
}
