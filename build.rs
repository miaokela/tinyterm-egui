//! Embeds the Windows icon and version info into `TinyTerm.exe`.
//!
//! Without this the .exe shows the generic Windows application icon in Explorer
//! and in the installer's "Apps & features" entry. The work is Windows-only, so
//! build scripts on macOS/Linux skip it entirely.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");

    #[cfg(windows)]
    {
        // Best effort: a missing rc.exe or an unreadable icon must not break the
        // build, it just leaves the default icon in place.
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "TinyTerm");
        res.set("FileDescription", "TinyTerm SSH client");
        res.set("LegalCopyright", "MIT License");
        if let Err(err) = res.compile() {
            println!("cargo:warning=Windows resources not embedded: {err}");
        }
    }
}
