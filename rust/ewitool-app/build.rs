//! For Windows builds only: embed the app icon and version details in the
//! .exe (shown in Explorer, the taskbar and Properties ▸ Details).
//! Cross-compiling from macOS needs mingw-w64 (`brew install mingw-w64`).

use std::{env, fs, path::Path};

fn main() {
    println!("cargo:rerun-if-changed=assets/icons/logo.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let mut parts: Vec<u16> = version.split('.').filter_map(|p| p.parse().ok()).collect();
    parts.resize(4, 0);
    let numeric = parts
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let icon = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("assets/icons/logo.ico")
        .display()
        .to_string()
        .replace('\\', "/");

    let rc = format!(
        r#"1 ICON "{icon}"

1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "EWITool2"
      VALUE "FileDescription", "EWITool2 - patch editor for the Akai EWI4000s"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "ewitool2"
      VALUE "LegalCopyright", "Copyright 2026 S.Merrony and ledhed2222. GPL-3.0-or-later."
      VALUE "OriginalFilename", "EWITool2.exe"
      VALUE "ProductName", "EWITool2"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    );
    let out = Path::new(&env::var("OUT_DIR").unwrap()).join("ewitool2.rc");
    fs::write(&out, rc).unwrap();
    embed_resource::compile(&out, embed_resource::NONE)
        .manifest_required()
        .expect("could not embed the Windows icon (is mingw-w64 installed?)");
}
