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
    #[arg(
        long,
        value_name = "MINECRAFT",
        help = "通过网易 1.20.1 的 ViaBackwards 用户映射配置汉化名称与附魔"
    )]
    client_audio: Option<PathBuf>,
    #[arg(long, requires = "client_audio", help = "恢复 ViaForge 原始备份")]
    restore_client_audio: bool,
    #[arg(long, help = "关闭目标 1.20.1 的布吉岛资源与客户端自动适配")]
    no_heypixel: bool,
    #[arg(long, help = "仅生成兼容资源，不修改或导入本机客户端")]
    no_client_setup: bool,
    #[arg(
        long,
        help = "关闭缺失下界合金剑、工具和盔甲材质的自动生成，保留原包已有材质"
    )]
    no_netherite: bool,
    #[arg(long, help = "关闭缺失下界合金剑材质的自动生成，保留原包已有材质")]
    no_netherite_sword: bool,
    #[arg(value_name = "SRC", help = "源 ZIP 或材质包文件夹")]
    source: Option<PathBuf>,
    #[arg(
        value_name = "OUT",
        help = "输出 ZIP 或文件夹，省略时按 [目标版本]原名自动命名"
    )]
    output: Option<PathBuf>,
    #[arg(long = "from", default_value = "auto", help = "源版本或 auto")]
    source_version: String,
    #[arg(long = "to", default_value = crate::LATEST_VERSION, help = "目标版本；26.4 对应当前官方快照")]
    target_version: String,
    #[arg(long, help = "输出为文件夹")]
    folder: bool,
    #[arg(long, help = "材质包描述，默认沿用原包")]
    desc: Option<String>,
    #[arg(long, value_parser = crate::compat::glint::parse_color, value_name = "RRGGBB", help = "自定义附魔闪烁颜色，例如 #FFFFFF；省略则保留原包效果")]
    glint_color: Option<[u8; 3]>,
    #[arg(
        long,
        conflicts_with = "glint_color",
        help = "恢复原版紫色附魔光效，保留原有图案与动画"
    )]
    vanilla_glint: bool,
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
    if crate::client_audio::finish_pending()? {
        return Ok(());
    }
    let args = Arguments::parse();
    if let Some(client) = args.client_audio {
        if args.restore_client_audio {
            println!(
                "已恢复：{}",
                crate::client_audio::restore(&client)?.display()
            );
        } else {
            let installed = crate::client_audio::install(&client)?;
            println!("{}", serde_json::to_string_pretty(&installed)?);
            println!(
                "已配置官方名称与附魔汉化，保持模组不变；下一次启动读取配置。原声音效尚未启用，不能通过普通映射配置恢复被合并或取消的声音。"
            );
        }
        return Ok(());
    }
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
        glint_color: args.glint_color,
        restore_vanilla_glint: args.vanilla_glint,
        heypixel_compat: !args.no_heypixel,
        auto_client_setup: !args.no_client_setup,
        auto_netherite: !args.no_netherite,
        auto_netherite_sword: !args.no_netherite_sword,
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
