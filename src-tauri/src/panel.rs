//! Main panel window lifecycle.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, LogicalSize, Manager};

use crate::state::AppCtx;
use crate::window_geometry;

pub const PANEL_LABEL: &str = "popover";

/// Default and minimum client size, in logical pixels. Keep in step with the
/// `popover` entry in tauri.conf.json (`width`/`height`, `minWidth`/`minHeight`).
/// The width never goes below the designed 520 (narrower would clip the
/// summary values); the height may shrink so small or highly scaled screens
/// can still show the whole window, with the dashboard scrolling inside it.
const DEFAULT_SIZE: PanelSize = PanelSize {
    width: 520.0,
    height: 620.0,
};
const MIN_SIZE: PanelSize = PanelSize {
    width: 520.0,
    height: 360.0,
};
/// A saved edge longer than this is a damaged file, not a real window.
const MAX_SAVED_EDGE: f64 = 16_384.0;

/// Remembered panel size, next to settings.json. Kept out of AppSettings on
/// purpose: the settings window writes that whole object back, and an older
/// copy there would undo a resize made in the meantime.
const SIZE_FILE_NAME: &str = "window-size.json";

/// Last size written to (or read from) the file, so hiding an unchanged
/// panel does not rewrite it. Starts at the default size, so a panel the user
/// never resized never creates the file.
static LAST_SAVED: Mutex<PanelSize> = Mutex::new(DEFAULT_SIZE);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
struct PanelSize {
    width: f64,
    height: f64,
}

impl PanelSize {
    /// Whole logical pixels, or `None` for sizes the panel can never have
    /// (minimized windows report 0×0). One pixel of slack below the minimum
    /// absorbs physical→logical rounding at fractional display scales.
    fn usable(self) -> Option<Self> {
        let edge = |value: f64, min: f64| {
            (value.is_finite() && value >= min - 1.0 && value <= MAX_SAVED_EDGE)
                .then(|| value.round().max(min))
        };
        Some(Self {
            width: edge(self.width, MIN_SIZE.width)?,
            height: edge(self.height, MIN_SIZE.height)?,
        })
    }
}

fn size_file(app: &AppHandle) -> PathBuf {
    app.state::<AppCtx>()
        .settings_path
        .with_file_name(SIZE_FILE_NAME)
}

fn read_size(path: &Path) -> Option<PanelSize> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<PanelSize>(&raw).ok()?.usable()
}

/// First show at launch. The panel is created hidden (tauri.conf.json) so the
/// remembered size can be applied before the user sees it.
pub fn show_on_launch(app: &AppHandle) {
    let Some(window) = app.get_webview_window(PANEL_LABEL) else {
        return;
    };
    end_quiet_start(app);
    restore_size_once(app, &window);
    window_geometry::present(&window);
}

/// The remembered size, applied before the first show only — at launch, or
/// on the first tray click after a quiet login launch.
fn restore_size_once(app: &AppHandle, window: &tauri::WebviewWindow) {
    if SIZE_RESTORED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    if let Some(size) = read_size(&size_file(app)) {
        *LAST_SAVED.lock().unwrap() = size;
        let _ = window.set_size(LogicalSize::new(size.width, size.height));
    }
}

static SIZE_RESTORED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Show the normal app window and bring it to the foreground. If it was hidden
/// to the tray, fit it to the screen and center it so a relaunch has a
/// predictable position.
pub fn show(app: &AppHandle) {
    let Some(window) = app.get_webview_window(PANEL_LABEL) else {
        return;
    };
    end_quiet_start(app);
    restore_size_once(app, &window);
    window_geometry::present(&window);
    let _ = app.emit("panel-shown", ());
}

/// Keep the tray app alive when users close the main window.
pub fn hide_now(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(PANEL_LABEL) {
        remember_size(app);
        let _ = window.hide();
    }
}

/// Write the panel's current size to disk if it changed. Called when the panel
/// is hidden and when the app is about to exit.
pub fn remember_size(app: &AppHandle) {
    // Before the first show restores the saved size (a quiet login launch),
    // the hidden window's size is not the user's: keep the saved one.
    if !SIZE_RESTORED.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let Some(window) = app.get_webview_window(PANEL_LABEL) else {
        return;
    };
    if window.is_minimized().unwrap_or(false) {
        return;
    }
    let (Ok(inner), Ok(scale)) = (window.inner_size(), window.scale_factor()) else {
        return;
    };
    let logical = inner.to_logical::<f64>(scale);
    let Some(size) = (PanelSize {
        width: logical.width,
        height: logical.height,
    })
    .usable() else {
        return;
    };
    let mut last = LAST_SAVED.lock().unwrap();
    if *last == size {
        return;
    }
    let Ok(data) = serde_json::to_string(&size) else {
        return;
    };
    match vibe_core::config::atomic_write(&size_file(app), data.as_bytes()) {
        Ok(()) => *last = size,
        Err(error) => log::warn!("panel size not saved: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(width: f64, height: f64) -> PanelSize {
        PanelSize { width, height }
    }

    #[test]
    fn real_sizes_round_to_whole_pixels() {
        assert_eq!(size(640.4, 812.6).usable(), Some(size(640.0, 813.0)));
        assert_eq!(DEFAULT_SIZE.usable(), Some(DEFAULT_SIZE));
    }

    #[test]
    fn rounding_just_below_the_minimum_snaps_to_it() {
        // 629 physical px at 175% is 359.43 logical.
        assert_eq!(size(519.6, 359.43).usable(), Some(MIN_SIZE));
    }

    #[test]
    fn impossible_sizes_are_rejected() {
        assert_eq!(size(0.0, 0.0).usable(), None); // minimized
        assert_eq!(size(300.0, 620.0).usable(), None);
        assert_eq!(size(520.0, 200.0).usable(), None);
        assert_eq!(size(f64::NAN, 620.0).usable(), None);
        assert_eq!(size(520.0, f64::INFINITY).usable(), None);
        assert_eq!(size(520.0, 100_000.0).usable(), None);
    }

    #[test]
    fn size_file_round_trips_and_ignores_damage() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(SIZE_FILE_NAME);
        assert_eq!(read_size(&path), None);

        std::fs::write(&path, serde_json::to_string(&size(700.0, 900.0)).unwrap()).unwrap();
        assert_eq!(read_size(&path), Some(size(700.0, 900.0)));

        std::fs::write(&path, "{\"width\":700}").unwrap();
        assert_eq!(read_size(&path), None);
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(read_size(&path), None);
        std::fs::write(&path, "{\"width\":10,\"height\":10}").unwrap();
        assert_eq!(read_size(&path), None);
    }
}

/// The user opened the window: quota cards may read again (see
/// `AppCtx::quiet_start`).
fn end_quiet_start(app: &AppHandle) {
    app.state::<AppCtx>()
        .quiet_start
        .store(false, std::sync::atomic::Ordering::SeqCst);
}
