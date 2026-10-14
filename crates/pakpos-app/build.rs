//! Embeds the application icon and version info into the Windows executable.

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icon.ico");
    #[cfg(windows)]
    {
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("../../assets/icon.ico");
            res.set("ProductName", "PakPos");
            res.set("FileDescription", "PakPos API client");
            if let Err(e) = res.compile() {
                println!("cargo:warning=could not embed Windows icon: {e}");
            }
        }
    }
}
