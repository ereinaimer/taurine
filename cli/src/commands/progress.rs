// Licensed under the Aimer Software License (ASL)
// See LICENSE for details.

use std::io::{IsTerminal, Write};
use taurine_core::error::{Error, Result};
use taurine_core::settings::SpinnerStyle;
use taurine_core::utils::spinner::{SpinnerRenderer, ThreadSpinnerHandle, spawn_threaded};
use taurine_core::voice::{
    download_model, download_model_with_status, get_model_entry, is_model_downloaded,
    resolve_configured_model,
};
use tracing::info;

pub(crate) struct StdoutStepRenderer {
    pub(crate) label: String,
}

impl SpinnerRenderer for StdoutStepRenderer {
    fn inject_frame(&mut self, _frame: &str) {
        print!("\r{}", self.label);
        let _ = std::io::Write::flush(&mut std::io::stdout());
    }
    fn backspace(&mut self, _: usize) {}
    fn move_left(&mut self, _: usize) {}
    fn move_right(&mut self, _: usize) {}
    fn finish(&mut self) {
        print!("\r");
        let _ = std::io::Write::flush(&mut std::io::stdout());
    }
}

pub(crate) struct Stepper {
    pub(crate) label: String,
    pub(crate) handle: Option<ThreadSpinnerHandle>,
    pub(crate) visual: bool,
    pub(crate) progress_open: bool,
    pub(crate) frame_idx: usize,
    pub(crate) cursor_hidden: bool,
}

impl Stepper {
    pub(crate) fn start_visual(label: &str, visual: bool) -> Self {
        let handle = visual.then(|| {
            let renderer = StdoutStepRenderer {
                label: label.to_string(),
            };
            spawn_threaded(SpinnerStyle::Braille, renderer)
        });
        Self {
            label: label.to_string(),
            handle,
            visual,
            progress_open: false,
            frame_idx: 0,
            cursor_hidden: false,
        }
    }

    pub(crate) fn start_attempt_stepper(label: &str, visual: bool) -> Self {
        let mut stepper = Self {
            label: label.to_string(),
            handle: None,
            visual,
            progress_open: false,
            frame_idx: 0,
            cursor_hidden: false,
        };
        stepper.start_attempt(label);
        stepper
    }

    pub(crate) fn stop_thread(&mut self) {
        if let Some(h) = self.handle.take() {
            h.stop();
        }
    }

    pub(crate) fn step(&mut self, next_label: &str) {
        let done = std::mem::replace(&mut self.label, next_label.to_string());
        self.stop_thread();
        self.clear_progress();
        info!("{done}");
        if self.visual {
            let renderer = StdoutStepRenderer {
                label: next_label.to_string(),
            };
            self.handle = Some(spawn_threaded(SpinnerStyle::Braille, renderer));
        }
    }

    /// Same as step, but logs a custom done line (used to report download
    /// totals instead of echoing the in-progress label).
    pub(crate) fn step_with_done(&mut self, next_label: &str, done_label: &str) {
        self.stop_thread();
        self.clear_progress();
        info!("{done_label}");
        self.label = next_label.to_string();
        if self.visual {
            let renderer = StdoutStepRenderer {
                label: next_label.to_string(),
            };
            self.handle = Some(spawn_threaded(SpinnerStyle::Braille, renderer));
        }
    }

    /// Freeze any spinner and open the two-line download block: line 1 keeps
    /// the label, line 2 carries live progress or connecting state without padding.
    /// Hides the terminal cursor while the block is open to prevent cursor blinking.
    pub(crate) fn start_attempt(&mut self, label: &str) {
        self.stop_thread();
        self.label = label.to_string();
        if !self.visual {
            info!("{label}...");
            return;
        }
        self.clear_progress();
        print!("\x1b[?25l\r{label}\x1b[K\n\rConnecting...\x1b[K");
        let _ = std::io::stdout().flush();
        self.progress_open = true;
        self.frame_idx = 0;
        self.cursor_hidden = true;
    }

    /// Single-writer redraw of both download lines (spinner thread is stopped
    /// while the block is open, so no output races). Call throttled ~5Hz.
    pub(crate) fn draw_download(&mut self, downloaded: u64, total: Option<u64>, speed_bps: f64) {
        if !self.visual || !self.progress_open {
            return;
        }
        let line2 = download_progress_line(downloaded, total, speed_bps);
        print!("\x1b[1A\r{}\x1b[K\n\r{line2}\x1b[K", self.label);
        let _ = std::io::stdout().flush();
    }

    /// Redraw the open block with a status message during post-download phases (e.g. decompression).
    pub(crate) fn draw_status(&mut self, status: &str) {
        if !self.visual {
            info!("{status}...");
            return;
        }
        if !self.progress_open {
            self.start_attempt(status);
            return;
        }
        self.label = status.to_string();
        print!(
            "\x1b[1A\r{}\x1b[K\n\rDecompressing model files...\x1b[K",
            self.label
        );
        let _ = std::io::stdout().flush();
    }

    /// Clear the open download block (line 2, then back up to line 1).
    /// Restores the terminal cursor if hidden. No-op unless a block is open.
    pub(crate) fn clear_progress(&mut self) {
        if !self.visual || !self.progress_open {
            return;
        }
        print!("\r\x1b[K\x1b[1A\r\x1b[K\x1b[?25h");
        let _ = std::io::stdout().flush();
        self.progress_open = false;
        self.cursor_hidden = false;
    }

    /// Clear a half-finished block without logging (error paths log via Err).
    pub(crate) fn abort_progress(&mut self) {
        self.stop_thread();
        self.clear_progress();
        if self.visual && self.cursor_hidden {
            print!("\x1b[?25h");
            let _ = std::io::stdout().flush();
            self.cursor_hidden = false;
        }
    }

    pub(crate) fn finish(mut self) {
        let label = std::mem::take(&mut self.label);
        self.stop_thread();
        self.clear_progress();
        if self.visual && self.cursor_hidden {
            print!("\x1b[?25h");
            let _ = std::io::stdout().flush();
            self.cursor_hidden = false;
        }
        info!("{label}");
    }

    pub(crate) fn finish_with_label(mut self, done_label: &str) {
        self.stop_thread();
        self.clear_progress();
        if self.visual && self.cursor_hidden {
            print!("\x1b[?25h");
            let _ = std::io::stdout().flush();
            self.cursor_hidden = false;
        }
        info!("{done_label}");
    }
}

impl Drop for Stepper {
    fn drop(&mut self) {
        self.stop_thread();
        if self.progress_open {
            self.clear_progress();
        }
        if self.visual && self.cursor_hidden {
            print!("\x1b[?25h");
            let _ = std::io::stdout().flush();
            self.cursor_hidden = false;
        }
    }
}

const SIZE_UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];

fn size_unit(bytes: u64) -> usize {
    let mut unit = 0;
    let mut scaled = bytes;
    while scaled >= 1024 && unit < SIZE_UNITS.len() - 1 {
        scaled /= 1024;
        unit += 1;
    }
    unit
}

fn fmt_scaled(bytes: u64, unit: usize) -> String {
    if unit == 0 {
        format!("{bytes}")
    } else {
        format!("{:.1}", bytes as f64 / 1024f64.powi(unit as i32))
    }
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    let unit = size_unit(bytes);
    format!("{} {}", fmt_scaled(bytes, unit), SIZE_UNITS[unit])
}

/// `downloaded/total` sharing one unit: `12.4/28.1 MB`.
pub(crate) fn format_pair(downloaded: u64, total: u64) -> String {
    let unit = size_unit(downloaded.max(total));
    format!(
        "{}/{} {}",
        fmt_scaled(downloaded, unit),
        fmt_scaled(total, unit),
        SIZE_UNITS[unit]
    )
}

pub(crate) fn format_speed(bytes_per_sec: f64) -> String {
    if !bytes_per_sec.is_finite() || bytes_per_sec <= 0.0 {
        return "0 B/s".to_string();
    }
    format!("{}/s", format_bytes(bytes_per_sec as u64))
}

pub(crate) fn format_duration(total_secs: u64) -> String {
    if total_secs < 60 {
        format!("{total_secs}s")
    } else {
        format!("{}m {}s", total_secs / 60, total_secs % 60)
    }
}

/// Line-2 content for the open download block.
pub(crate) fn download_progress_line(
    downloaded: u64,
    total: Option<u64>,
    speed_bps: f64,
) -> String {
    match total {
        Some(t) if t > 0 => {
            let pct = ((downloaded as f64 / t as f64) * 100.0)
                .floor()
                .clamp(0.0, 100.0) as u64;
            format!(
                "{} ({pct}%) @ {}",
                format_pair(downloaded, t),
                format_speed(speed_bps)
            )
        }
        _ => format!(
            "{} downloaded @ {}",
            format_bytes(downloaded),
            format_speed(speed_bps)
        ),
    }
}

/// Ensures the requested voice model is downloaded to the models directory.
///
/// If already present, returns immediately.
/// If missing, downloads it and displays a live terminal progress indicator
/// matching `taurine update`.
pub fn ensure_voice_model_downloaded(model_id: &str, json: bool) -> Result<()> {
    let canonical = resolve_configured_model(model_id);
    if is_model_downloaded(canonical, None) {
        if canonical == "parakeet-unified-en-0.6b" {
            taurine_core::voice::ensure_unified_hotwords_asset(None);
        }
        return Ok(());
    }

    let entry = get_model_entry(canonical)
        .ok_or_else(|| Error::Config(format!("Unknown voice model identifier: '{model_id}'")))?;

    if json {
        download_model(canonical, None, None)?;
        if canonical == "parakeet-unified-en-0.6b" {
            taurine_core::voice::ensure_unified_hotwords_asset(None);
        }
        return Ok(());
    }

    let visual = std::io::stdout().is_terminal();
    let download_label = format!(
        "Downloading voice model '{}' ({})",
        entry.id, entry.size_display
    );
    let stepper = std::cell::RefCell::new(Stepper::start_attempt_stepper(&download_label, visual));

    let res = download_model_with_status(
        canonical,
        None,
        Some(&mut |downloaded, total, speed_bps| {
            stepper
                .borrow_mut()
                .draw_download(downloaded, total, speed_bps);
        }),
        Some(&mut |status| {
            stepper.borrow_mut().draw_status(status);
        }),
    );

    let mut stepper = stepper.into_inner();
    match res {
        Ok(_) => {
            if canonical == "parakeet-unified-en-0.6b" {
                taurine_core::voice::ensure_unified_hotwords_asset(None);
            }
            stepper.finish_with_label(&format!(
                "Successfully downloaded and installed '{}'",
                entry.id
            ));
            Ok(())
        }
        Err(e) => {
            stepper.abort_progress();
            Err(e)
        }
    }
}
