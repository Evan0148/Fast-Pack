fn main() {
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
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
