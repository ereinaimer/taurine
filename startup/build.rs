use std::io;

fn main() -> io::Result<()> {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        let mut res = winres::WindowsResource::new();
        res.set("FileDescription", "Taurine");
        res.set("CompanyName", "Erein Aimer");
        res.set("ProductName", "Taurine");
        res.set("OriginalFilename", "taurine-startup.exe");
        res.set("LegalCopyright", "Copyright (c) Erein Aimer");
        res.compile()?;
    }
    Ok(())
}
