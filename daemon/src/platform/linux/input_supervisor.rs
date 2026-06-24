use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

use super::evdev::{self, DeviceExit, ListenerContext};

const INPUT_DIR: &str = "/dev/input";
const RESCAN_INTERVAL: Duration = Duration::from_secs(2);
const OPEN_RETRY_INTERVAL: Duration = Duration::from_secs(10);
const NO_ACTIVE_WARNING_INTERVAL: Duration = Duration::from_secs(30);

pub(crate) fn start(context: ListenerContext) {
    let spawn_result = thread::Builder::new()
        .name("taurine-linux-input-supervisor".to_string())
        .spawn(move || run(context));

    if let Err(error) = spawn_result {
        error!(error = %error, "Failed to spawn Linux input supervisor");
    }
}

fn run(context: ListenerContext) {
    let (exit_tx, exit_rx) = mpsc::channel::<DeviceExit>();
    let mut active_devices = HashMap::<PathBuf, u64>::new();
    let mut ignored_devices = HashSet::<PathBuf>::new();
    let mut next_open_attempt = HashMap::<PathBuf, Instant>::new();
    let mut next_worker_id = 1_u64;
    let mut last_no_active_warning = None::<Instant>;

    info!("Starting Linux input supervisor");

    loop {
        for exit in exit_rx.try_iter() {
            if active_devices
                .get(&exit.path)
                .is_some_and(|worker_id| *worker_id == exit.worker_id)
            {
                active_devices.remove(&exit.path);
                next_open_attempt.remove(&exit.path);
                info!("Linux input listener exited for {:?}", exit.path);
            } else {
                debug!(
                    "Ignoring stale Linux input listener exit for {:?}",
                    exit.path
                );
            }
        }

        let event_paths = match discover_event_paths() {
            Ok(paths) => paths,
            Err(error) => {
                warn!(
                    "Failed to scan {} for keyboard devices: {}",
                    INPUT_DIR, error
                );
                thread::sleep(RESCAN_INTERVAL);
                continue;
            }
        };

        let present_paths: HashSet<_> = event_paths.iter().cloned().collect();
        active_devices.retain(|path, _| {
            let still_present = present_paths.contains(path);
            if !still_present {
                info!("Linux input device disappeared: {:?}", path);
            }
            still_present
        });
        ignored_devices.retain(|path| present_paths.contains(path));
        next_open_attempt.retain(|path, _| present_paths.contains(path));

        for path in event_paths {
            if active_devices.contains_key(&path)
                || ignored_devices.contains(&path)
                || !open_attempt_due(&next_open_attempt, &path)
            {
                continue;
            }

            match evdev::open_keyboard_device(&path) {
                Ok(Some(device)) => {
                    let worker_id = next_worker_id;
                    next_worker_id = next_worker_id.wrapping_add(1).max(1);

                    match evdev::spawn_device_listener(
                        path.clone(),
                        worker_id,
                        device,
                        context.clone(),
                        exit_tx.clone(),
                    ) {
                        Ok(()) => {
                            active_devices.insert(path.clone(), worker_id);
                            next_open_attempt.remove(&path);
                            info!("Started Linux input listener for {:?}", path);
                        }
                        Err(error) => {
                            warn!(
                                "Failed to spawn Linux input listener for {:?}: {}",
                                path, error
                            );
                            schedule_open_retry(&mut next_open_attempt, path);
                        }
                    }
                }
                Ok(None) => {
                    ignored_devices.insert(path);
                }
                Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                    warn!(
                        "Permission denied opening {:?}. You may need to add your user to the 'input' group.",
                        path
                    );
                    schedule_open_retry(&mut next_open_attempt, path);
                }
                Err(error) => {
                    debug!("Failed to open input device {:?}: {}", path, error);
                    schedule_open_retry(&mut next_open_attempt, path);
                }
            }
        }

        if active_devices.is_empty() {
            let should_warn = last_no_active_warning
                .map(|last_warning| last_warning.elapsed() >= NO_ACTIVE_WARNING_INTERVAL)
                .unwrap_or(true);

            if should_warn {
                warn!(
                    "No Linux keyboard input listeners are active; waiting for a keyboard device to appear"
                );
                last_no_active_warning = Some(Instant::now());
            }
        } else {
            last_no_active_warning = None;
        }

        thread::sleep(RESCAN_INTERVAL);
    }
}

fn discover_event_paths() -> io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();

    for entry in fs::read_dir(INPUT_DIR)? {
        let entry = entry?;
        let path = entry.path();
        if is_event_device_path(&path) {
            paths.push(path);
        }
    }

    paths.sort();
    Ok(paths)
}

fn is_event_device_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|file_name| file_name.to_str())
        .is_some_and(|file_name| file_name.starts_with("event"))
}

fn open_attempt_due(next_open_attempt: &HashMap<PathBuf, Instant>, path: &Path) -> bool {
    next_open_attempt
        .get(path)
        .map_or(true, |next_attempt| Instant::now() >= *next_attempt)
}

fn schedule_open_retry(next_open_attempt: &mut HashMap<PathBuf, Instant>, path: PathBuf) {
    next_open_attempt.insert(path, Instant::now() + OPEN_RETRY_INTERVAL);
}
