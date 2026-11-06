//! Windows: embed the app icon and version information in openvclient.exe so
//! Explorer, the taskbar and "Apps & features" show them.

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icon/openvclient.ico");
    #[cfg(windows)]
    {
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("../../assets/icon/openvclient.ico")
                .set("ProductName", "OpenVClient")
                .set("FileDescription", "OpenVClient — databases and infrastructure")
                .set("CompanyName", "OpenVClient contributors")
                .set("LegalCopyright", "© OpenVClient contributors · MIT OR Apache-2.0")
                .set("OriginalFilename", "OpenVClient.exe");
            if let Err(e) = res.compile() {
                // Missing rc.exe should not break developer builds; installers need it.
                println!("cargo:warning=could not embed the Windows icon/version resource: {e}");
            }
        }
    }
}
