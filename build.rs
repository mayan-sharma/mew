//! Checks the Mew window is built; on Windows, also embeds the cat icon and version info into mew.exe.

fn main() {
    let page = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("ui/dist/index.html");
    assert!(page.exists(), "The Mew window isn't built yet. Run: npm --prefix ui ci; npm --prefix ui run build");
    println!("cargo:rerun-if-changed=ui/dist/index.html");
    #[cfg(windows)]
    embed_resources();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        link_clang_runtime();
    }
}

/// ONNX Runtime's prebuilt library checks the macOS version with `@available`, whose helper
/// (`__isPlatformVersionAtLeast`) lives in clang's runtime library, which rustc doesn't link.
fn link_clang_runtime() {
    let out = std::process::Command::new("clang").arg("--print-runtime-dir").output().expect("clang (Xcode tools)");
    let dir = String::from_utf8(out.stdout).unwrap();
    println!("cargo:rustc-link-search=native={}", dir.trim());
    println!("cargo:rustc-link-lib=static=clang_rt.osx");
}

#[cfg(windows)]
fn embed_resources() {
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let parts: Vec<&str> = version.split(['.', '-']).take(3).collect();
    let numeric = format!("{},{},{},0", parts[0], parts[1], parts[2]);
    let icon = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets").join("mew.ico");
    let rc = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Mew"
      VALUE "FileDescription", "Mew - local dictation"
      VALUE "FileVersion", "{version}"
      VALUE "ProductName", "Mew"
      VALUE "ProductVersion", "{version}"
      VALUE "OriginalFilename", "mew.exe"
      VALUE "LegalCopyright", "MIT License"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = icon.display().to_string().replace('\\', "\\\\"),
    );
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("mew.rc");
    std::fs::write(&out, rc).unwrap();
    embed_resource::compile(&out, embed_resource::NONE).manifest_optional().unwrap();
    println!("cargo:rerun-if-changed=assets/mew.ico");
}
