fn main() {
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=app.manifest.rc");
        println!("cargo:rerun-if-changed=app.exe.manifest");
        embed_resource::compile("app.manifest.rc", embed_resource::NONE)
            .manifest_required()
            .expect("failed to embed Windows application manifest");
    }
    tauri_build::build()
}
