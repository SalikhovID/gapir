fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // CPAL links CoreAudio functions that exist only on macOS 14.2+.
        println!("cargo:rustc-link-arg=-Wl,-weak_framework,CoreAudio");
        link_compiler_runtime();
    }
    tauri_build::build()
}

/// ggml-metal guards newer Metal APIs with `@available`. With a deployment target older than the
/// SDK those checks become calls into `libclang_rt.osx.a`, which rustc does not link by itself.
fn link_compiler_runtime() {
    let output = std::process::Command::new("clang")
        .arg("--print-resource-dir")
        .output()
        .expect("clang must be available to locate libclang_rt.osx.a");
    let resource_dir = String::from_utf8(output.stdout).expect("clang path is valid UTF-8");
    let runtime_dir = std::path::Path::new(resource_dir.trim()).join("lib/darwin");
    assert!(
        runtime_dir.join("libclang_rt.osx.a").exists(),
        "libclang_rt.osx.a not found in {}",
        runtime_dir.display()
    );
    println!("cargo:rustc-link-search=native={}", runtime_dir.display());
    println!("cargo:rustc-link-lib=static=clang_rt.osx");
}
