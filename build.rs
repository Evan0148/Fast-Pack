fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/app.ico");
    use std::io::Write;
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    for (source, name) in [
        ("assets/model-integrity.jsonl", "model-integrity.deflate"),
        ("assets/modern-items.json", "modern-items.deflate"),
        ("assets/via-items/catalog.json", "via-catalog.deflate"),
    ] {
        println!("cargo:rerun-if-changed={source}");
        let original = std::fs::read(source).expect("无法读取内置资源");
        let bytes = if std::env::var_os("CARGO_FEATURE_COMPACT_DATA").is_some() {
            let mut encoder =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
            encoder.write_all(&original).expect("无法压缩内置资源");
            encoder.finish().expect("内置资源压缩未完成")
        } else {
            original
        };
        std::fs::write(output.join(name), bytes).expect("无法写入内置资源");
    }
    #[cfg(windows)]
    {
        if std::env::var("CARGO_CFG_TARGET_VENDOR").as_deref() == Ok("win7") {
            println!("cargo:rustc-link-arg-bin=FastPack=/SUBSYSTEM:WINDOWS,6.01");
            println!("cargo:rustc-link-arg-bin=fast-pack-cli=/SUBSYSTEM:CONSOLE,6.01");
        }
        let mut resource = winresource::WindowsResource::new();
        resource.set_manifest(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
<assemblyIdentity version="1.0.0.0" processorArchitecture="*" name="ColdEternity.FastPack" type="win32"/>
<trustInfo xmlns="urn:schemas-microsoft-com:asm.v3"><security><requestedPrivileges><requestedExecutionLevel level="asInvoker" uiAccess="false"/></requestedPrivileges></security></trustInfo>
<compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1"><application>
<supportedOS Id="{35138b9a-5d96-4fbd-8e2d-a2440225f93a}"/>
<supportedOS Id="{4a2f28e3-53b9-4441-ba9c-d69d4a4a6e38}"/>
<supportedOS Id="{1f676c76-80e1-4239-95bb-83d0f6d0da78}"/>
<supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
</application></compatibility>
<application xmlns="urn:schemas-microsoft-com:asm.v3"><windowsSettings>
<dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
<dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2,PerMonitor</dpiAwareness>
<longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
</windowsSettings></application>
</assembly>"#);
        resource.set_icon("assets/app.ico");
        resource.set("ProductName", "Fast Pack");
        resource.set("FileDescription", "Fast Pack 材质包转换器");
        resource.set(
            "OriginalFilename",
            &format!(
                "FastPack-v{}.exe",
                std::env::var("CARGO_PKG_VERSION").unwrap()
            ),
        );
        resource.set("CompanyName", "ColdEternity Team");
        resource.set(
            "LegalCopyright",
            "Copyright © 2026 ColdEternity Team, Long_Cold, Wind_",
        );
        resource.compile().expect("无法编译 Windows 资源");
    }
}
