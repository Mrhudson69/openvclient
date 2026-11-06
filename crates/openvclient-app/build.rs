//! Windows: embed the app icon and version information in openvclient.exe so
//! Explorer, the taskbar and "Apps & features" show them. FileVersion and
//! ProductVersion come from the Cargo package version (winresource default).

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icon/openvclient.ico");
    println!("cargo:rerun-if-env-changed=OPENVCLIENT_REQUIRE_RESOURCES");
    #[cfg(windows)]
    {
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
            let mut res = winresource::WindowsResource::new();
            // FileDescription is the name Task Manager and "Open with" show.
            res.set_icon("../../assets/icon/openvclient.ico")
                .set("ProductName", "OpenVClient")
                .set("FileDescription", "OpenVClient")
                .set("CompanyName", "OpenVClient contributors")
                .set("LegalCopyright", "© OpenVClient contributors · MIT OR Apache-2.0")
                .set("OriginalFilename", "OpenVClient.exe")
                .set("InternalName", "openvclient");
            if let Err(e) = res.compile() {
                // Developer builds go on without the icon; bundle-windows.ps1 sets
                // OPENVCLIENT_REQUIRE_RESOURCES so an icon-less exe never ships.
                if std::env::var_os("OPENVCLIENT_REQUIRE_RESOURCES").is_some() {
                    panic!("could not embed the Windows icon/version resource (is rc.exe from the Windows SDK installed?): {e}");
                }
                println!("cargo:warning=could not embed the Windows icon/version resource: {e}");
            }
        }
    }
}
