//! Windows: the icon and version info in the executable. Every target: where libmpv is, when a
//! packaging build provides it outside the system's library paths (YTMFAST_MPV_DIR).

fn main() {
    println!("cargo:rerun-if-env-changed=YTMFAST_MPV_DIR");
    if let Some(dir) = std::env::var_os("YTMFAST_MPV_DIR") {
        println!("cargo:rustc-link-search=native={}", std::path::Path::new(&dir).display());
    }
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=packaging/windows/ytmfast.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("packaging/windows/ytmfast.ico").set("ProductName", "YtmFast").set("FileDescription", "A native YouTube Music client");
        if let Err(e) = res.compile() {
            println!("cargo:warning=Windows resources not embedded: {e}");
        }
    }
}
