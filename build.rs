fn main() {
    // Window / taskbar / Explorer icon (gpui loads icon resource 1).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/icon.rc");
        println!("cargo:rerun-if-changed=assets/icon.ico");
        embed_resource::compile("assets/icon.rc", embed_resource::NONE).manifest_optional().unwrap();
    }
}
