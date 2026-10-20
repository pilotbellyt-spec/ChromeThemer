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

    assert_eq!(
        env::var("CARGO_CFG_TARGET_ARCH").as_deref(),
        Ok("x86_64"),
        "The Windows frame helper requires the MSYS2 UCRT64 x86_64 toolchain"
    );
    let compiler = env::var_os("CHROMETHEMER_FRAME_CC")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:/msys64/ucrt64/bin/gcc.exe"));
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    let source = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("native/frame.c");
    let helper = output.join("ChromeThemerFrame.dll");
    let status = Command::new(&compiler)
        .args([
            "-std=c11",
            "-shared",
            "-O2",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-static-libgcc",
        ])
        .arg(source)
        .arg("-o")
        .arg(&helper)
        .args(["-lcomctl32", "-luser32", "-lkernel32"])
        .status()
        .unwrap_or_else(|error| panic!("Cannot run UCRT64 GCC at {}: {error}", compiler.display()));
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
