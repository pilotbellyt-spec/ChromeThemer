fn main() {
    println!("cargo:rerun-if-changed=native/frame.c");
    println!("cargo:rerun-if-env-changed=CHROMETHEMER_FRAME_CC");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        build_frame_helper();
    }
    tauri_build::build();
}

fn build_frame_helper() {
    use std::{env, path::PathBuf, process::Command};

    let architecture = env::var("CARGO_CFG_TARGET_ARCH").expect("Cargo sets target architecture");
    let (default_compiler, runtime) = match architecture.as_str() {
        "x86_64" => ("C:/msys64/ucrt64/bin/gcc.exe", "-static-libgcc"),
        "aarch64" => ("C:/msys64/clangarm64/bin/clang.exe", "-static"),
        _ => panic!("The Windows frame helper supports only x64 and ARM64"),
    };
    let compiler = env::var_os("CHROMETHEMER_FRAME_CC")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default_compiler));
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    let source = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("native/frame.c");
    let helper = output.join("ChromeThemerFrame.dll");
    let status = Command::new(&compiler)
        .args([
            "-std=c11", "-shared", "-O2", "-Wall", "-Wextra", "-Werror", runtime,
        ])
        .arg(source)
        .arg("-o")
        .arg(&helper)
        .args(["-lcomctl32", "-luser32", "-lkernel32"])
        .status()
        .unwrap_or_else(|error| {
            panic!(
                "Cannot run the frame compiler at {}: {error}",
                compiler.display()
            )
        });
    assert!(status.success(), "Windows frame helper compilation failed");
    println!(
        "cargo:rustc-env=CHROMETHEMER_FRAME_DLL={}",
        helper.display()
    );

    // OUT_DIR is <target>/<optional-triple>/<profile>/build/<package>/out.
    let profile = output.ancestors().nth(3).expect("Cargo OUT_DIR layout");
    std::fs::copy(&helper, profile.join("ChromeThemerFrame.dll"))
        .expect("Cannot stage the Windows frame helper beside the desktop executable");
}
