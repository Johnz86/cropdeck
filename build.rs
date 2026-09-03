const WINDOWS_ICON: &str = "assets/windows/cropdeck.ico";

#[cfg(windows)]
fn embed_windows_resources() {
    winresource::WindowsResource::new()
        .set_icon(WINDOWS_ICON)
        .set("ProductName", "CropDeck")
        .set("FileDescription", "CropDeck")
        .compile()
        .expect("Windows resource compilation failed");
}

#[cfg(not(windows))]
fn embed_windows_resources() {}

fn main() {
    println!("cargo:rerun-if-changed={WINDOWS_ICON}");
    embed_windows_resources();
}
