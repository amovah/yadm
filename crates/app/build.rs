use std::path::PathBuf;

fn main() {
    let icon = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../assets/icon.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    // Executable icon shown by Explorer, the taskbar and Alt+Tab. gpui embeds
    // the application manifest itself.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let rc = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("yadm.rc");
        // Forward slashes keep the path valid for both rc.exe and windres.
        let path = icon.display().to_string().replace('\\', "/");
        std::fs::write(&rc, format!("1 ICON \"{path}\"\n")).unwrap();
        embed_resource::compile(&rc, embed_resource::NONE).manifest_optional().expect("compile yadm.rc");
    }
}
