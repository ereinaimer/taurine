pub mod evdev;
pub mod security;
pub mod xkb;

pub fn init() -> Result<(), String> {
    // For now, immediately drop privileges
    security::drop_privileges()
}
