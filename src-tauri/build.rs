fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // CPAL links CoreAudio functions that exist only on macOS 14.2+.
        println!("cargo:rustc-link-arg=-Wl,-weak_framework,CoreAudio");
        link_compiler_runtime();
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        copy_speech_dlls();
    }
    tauri_build::build()
}

/// transcribe-cpp-sys puts its DLLs next to the binaries only when its own build script runs. A
/// cached build (CI) skips it and the tests fail with STATUS_DLL_NOT_FOUND, so copy them each time.
fn copy_speech_dlls() {
    let source =
        std::env::var("DEP_TRANSCRIBE_RUNTIME_DIR").expect("transcribe-cpp-sys is built shared");
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    // OUT_DIR is <profile>/build/<crate>-<hash>/out.
    let profile = out_dir
        .ancestors()
        .nth(3)
        .expect("OUT_DIR is inside the profile dir");
    let dlls: Vec<_> = std::fs::read_dir(&source)
        .unwrap_or_else(|error| panic!("{source}: {error}"))
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "dll"))
        .collect();
    assert!(!dlls.is_empty(), "no DLLs in {source}");
    for dir in [profile.to_path_buf(), profile.join("deps")] {
        for dll in &dlls {
            // A running gapir.exe locks its copy; that copy is already in place.
            let _ = std::fs::copy(dll, dir.join(dll.file_name().expect("file name")));
        }
    }
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
