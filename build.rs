fn main() {
    // CARGO_CFG_* describes the target; cfg!(windows) here would describe the host.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/hammurabi.ico")
            .set("ProductName", "Hammurabi")
            .set("FileDescription", "Hammurabi")
            .set("LegalCopyright", "Copyright (c) 2026 Can Ozmen");
        res.compile().expect("compile Windows resources");
    }
    // ONNX Runtime's CoreML code calls clang's __isPlatformVersionAtLeast (compiler-rt), which
    // rustc does not link by itself.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-fapple-link-rtlib");
    }
}
