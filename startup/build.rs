use std::io;

fn main() -> io::Result<()> {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        let mut res = winres::WindowsResource::new();
        res.set("FileDescription", "Nite");
        res.set("CompanyName", "Erein Aimer");
        res.set("ProductName", "Nite");
        res.set("OriginalFilename", "nite-startup.exe");
        res.set("LegalCopyright", "Copyright (c) Erein Aimer");
        res.compile()?;
    }
    Ok(())
}
