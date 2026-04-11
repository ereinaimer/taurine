pub mod evdev;
pub mod security;
pub mod uinput;
pub mod xkb;

pub fn init() -> Result<(), String> {
    uinput::init_uinput()?;
    // For now, immediately drop privileges
    security::drop_privileges()
}
