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
    println!("cargo::rustc-check-cfg=cfg(omnivoice)");
    #[cfg(feature = "omnivoice")]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        omnivoice();
    }
}

/// Builds omnivoice.cpp and ggml (Vulkan) as static libraries plus the C shim, and sets
/// `cfg(omnivoice)` for the Speaker's voice engine. Needs cmake and the Vulkan SDK (VULKAN_SDK).
#[cfg(feature = "omnivoice")]
fn omnivoice() {
    use std::path::PathBuf;
    let src = "third_party/omnivoice.cpp";
    assert!(
        std::path::Path::new(src)
            .join("ggml/CMakeLists.txt")
            .is_file(),
        "{src} is missing; run `git submodule update --init --recursive`"
    );
    let vulkan = PathBuf::from(std::env::var("VULKAN_SDK").expect("set VULKAN_SDK (Vulkan SDK)"));
    // ggml's Vulkan shader generator nests deep enough to pass MAX_PATH from inside OUT_DIR,
    // so build in <target>/ov-<profile> instead.
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let target = out.ancestors().nth(4).unwrap();
    let profile_dir = out.ancestors().nth(3).unwrap().file_name().unwrap();
    let mut short = target.join("ov-");
    short.as_mut_os_string().push(profile_dir);
    let dst = cmake::Config::new(src)
        .out_dir(&short)
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("GGML_VULKAN", "ON")
        // A portable AVX2 CPU backend instead of one tuned to the build machine.
        .define("GGML_NATIVE", "OFF")
        .define("GGML_AVX", "ON")
        .define("GGML_AVX2", "ON")
        .define("GGML_FMA", "ON")
        .define("GGML_F16C", "ON")
        .define("GGML_BUILD_TESTS", "OFF")
        .define("GGML_BUILD_EXAMPLES", "OFF")
        // Debug ggml is far too slow, and it would link the debug C runtime.
        .profile("Release")
        .build_target("omnivoice-core")
        .build()
        .join("build");
    for dir in ["", "ggml/src", "ggml/src/ggml-vulkan"] {
        println!(
            "cargo:rustc-link-search=native={}",
            dst.join(dir).join("Release").display()
        );
    }
    println!(
        "cargo:rustc-link-search=native={}",
        vulkan.join("Lib").display()
    );
    for lib in [
        "omnivoice-core",
        "ggml",
        "ggml-base",
        "ggml-cpu",
        "ggml-vulkan",
    ] {
        println!("cargo:rustc-link-lib=static={lib}");
    }
    println!("cargo:rustc-link-lib=dylib=vulkan-1");
    cc::Build::new()
        .file("src/tts/omnivoice_shim.c")
        .include(format!("{src}/src"))
        .define("OMNIVOICE_STATIC", None)
        .compile("hm_omnivoice_shim");
    println!("cargo:rerun-if-changed=assets/hammurabi.ico");
    println!("cargo:rerun-if-changed=src/tts/omnivoice_shim.c");
    println!("cargo:rerun-if-changed={src}/src");
    println!("cargo:rustc-cfg=omnivoice");
}
