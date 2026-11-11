fn main() {
    // Window / taskbar / Explorer icon (gpui loads icon resource 1) and the
    // version information Explorer shows, generated from Cargo.toml so the
    // two never disagree.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/icon.ico");
        println!("cargo:rerun-if-changed=Cargo.toml");
        let version = std::env::var("CARGO_PKG_VERSION").unwrap();
        let mut parts: Vec<u32> = version.split(['.', '-']).filter_map(|p| p.parse().ok()).collect();
        parts.resize(4, 0);
        let numeric = parts.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
        let icon = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets").join("icon.ico");
        let icon = icon.display().to_string().replace('\\', "\\\\");
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
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Abyss project"
      VALUE "FileDescription", "Abyss - unofficial TIDAL desktop client"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "Abyss"
      VALUE "LegalCopyright", "MIT License. Not affiliated with TIDAL."
      VALUE "OriginalFilename", "Abyss.exe"
      VALUE "ProductName", "Abyss"
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
        let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("abyss.rc");
        std::fs::write(&out, rc).unwrap();
        embed_resource::compile(&out, embed_resource::NONE).manifest_optional().unwrap();
    }
}
