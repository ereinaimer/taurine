fn main() {
    if cfg!(target_os = "windows") {
        let res = winres::WindowsResource::new();
        if let Err(e) = res.compile() {
            println!("cargo:warning=Failed to compile windows resources: {}", e);
        }
    }
}
