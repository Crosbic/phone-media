fn main() {
    println!("cargo:rerun-if-changed=assets");
    winresource::WindowsResource::new()
        .set_icon_with_id("assets/phone-media.ico", "1")
        .set_icon_with_id("assets/phone-media-off.ico", "2")
        .set("FileDescription", "Phone Media")
        .set("ProductName", "Phone Media")
        .set_manifest_file("assets/phone-media.manifest")
        .compile()
        .expect("не удалось встроить ресурсы в exe");
}
