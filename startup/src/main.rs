#![windows_subsystem = "windows"]

use std::env;
use std::os::windows::process::CommandExt;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x08000000;

fn main() {
    // Determine the path to the main taurine executable.
    // It should be located in the same directory, or we can look it up from the current exe.
    // However, when installed, `taurine-startup.exe` is placed in `%APPDATA%\Taurine`,
    // while `taurine.exe` might be located anywhere (e.g. in PATH).
    // So how does `taurine-startup.exe` know where `taurine.exe` is?
    // We can read it from a registry key, or pass it as an argument, or write it next to the startup exe.
    // Wait, currently `sync_boot` puts the full path to `taurine.exe` inside the `.vbs` script!
    // So the runner needs to know the path to the original `taurine.exe`.
    // The easiest way is for `taurine-startup.exe` to accept the path to `taurine.exe` as its first argument.
    // Because we control the `Run` registry key, we can set the Run key to:
    // `%APPDATA%\Taurine\taurine-startup.exe "C:\Path\To\taurine.exe"`
    // Or we can just read the arguments!
    
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        // Fallback if no argument is provided, though it shouldn't happen.
        return;
    }
    
    let target_exe = &args[1];
    
    let _ = Command::new(target_exe)
        .arg("--daemon")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}
