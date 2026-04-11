pub mod security;

pub fn init() -> Result<(), String> {
    // For now, immediately drop privileges
    security::drop_privileges()
}
