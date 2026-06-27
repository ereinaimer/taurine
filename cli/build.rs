fn main() {
    if cfg!(target_os = "windows") {
        let mut res = winres::WindowsResource::new();
        res.set("FileDescription", "Taurine");
        res.set("CompanyName", "Erein Aimer");
        res.set("ProductName", "Taurine");
        res.set("OriginalFilename", "taurine.exe");
        res.set("LegalCopyright", "Copyright (c) Erein Aimer");
        if let Err(e) = res.compile() {
            println!("cargo:warning=Failed to compile windows resources: {}", e);
        }
    }
}
