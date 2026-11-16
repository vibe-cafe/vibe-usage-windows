//! Keeping resizable windows on screen when they are brought back.
//!
//! Both app windows can be resized and stay alive while hidden. A size that
//! fit one monitor (or one display-scale setting) can be taller than the work
//! area of another, and centering a too-tall window pushes its title bar above
//! the screen where it can no longer be dragged. `preventOverflow` in
//! tauri.conf.json covers window creation; this covers every later show.

use tauri::{PhysicalSize, Runtime, WebviewWindow};

/// Unminimize, show and focus `window`. A window that was hidden is first
/// shrunk to fit its monitor's work area and centered, so a relaunch from the
/// tray has a predictable position and never opens partly off screen. A
/// window that is already visible keeps the size and place the user gave it.
pub fn present<R: Runtime>(window: &WebviewWindow<R>) {
    let was_visible = window.is_visible().unwrap_or(false);
    let _ = window.unminimize();
    if !was_visible {
        fit_to_work_area(window);
        let _ = window.center();
    }
    let _ = window.show();
    let _ = window.set_focus();
}

/// Shrink the window, if needed, so the whole frame fits the work area of the
/// monitor it is on. Never grows it; the min size from the config still wins.
pub fn fit_to_work_area<R: Runtime>(window: &WebviewWindow<R>) {
    let Ok(Some(monitor)) = window.current_monitor() else {
        return;
    };
    let (Ok(inner), Ok(outer)) = (window.inner_size(), window.outer_size()) else {
        return;
    };
    if let Some(size) = fitted_inner_size(inner, outer, monitor.work_area().size) {
        let _ = window.set_size(size);
    }
}

/// New client size that makes the outer frame fit `work_area`, or `None` when
/// it already fits. The frame (title bar and borders) is whatever `outer`
/// adds to `inner`, so it follows the display scale and the Windows theme.
fn fitted_inner_size(
    inner: PhysicalSize<u32>,
    outer: PhysicalSize<u32>,
    work_area: PhysicalSize<u32>,
) -> Option<PhysicalSize<u32>> {
    let max_width = work_area
        .width
        .saturating_sub(outer.width.saturating_sub(inner.width));
    let max_height = work_area
        .height
        .saturating_sub(outer.height.saturating_sub(inner.height));
    if inner.width <= max_width && inner.height <= max_height {
        return None;
    }
    Some(PhysicalSize::new(
        inner.width.min(max_width),
        inner.height.min(max_height),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(width: u32, height: u32) -> PhysicalSize<u32> {
        PhysicalSize::new(width, height)
    }

    #[test]
    fn window_that_fits_is_left_alone() {
        // 520×620 at 100% on a 1920×1080 screen with a 48px taskbar.
        assert_eq!(
            fitted_inner_size(size(520, 620), size(536, 659), size(1920, 1032)),
            None
        );
    }

    #[test]
    fn too_tall_window_loses_exactly_the_overflow() {
        // 1366×768 at 125%: the 520×620 window is 650×775 physical (+frame),
        // the work area is 1366×708. Width is untouched.
        let fitted = fitted_inner_size(size(650, 775), size(670, 824), size(1366, 708));
        assert_eq!(fitted, Some(size(650, 708 - 49)));
    }

    #[test]
    fn too_wide_window_is_narrowed_too() {
        let fitted = fitted_inner_size(size(2000, 600), size(2016, 639), size(1920, 1032));
        assert_eq!(fitted, Some(size(1904, 600)));
    }

    #[test]
    fn degenerate_sizes_do_not_underflow() {
        assert_eq!(
            fitted_inner_size(size(520, 620), size(536, 659), size(0, 0)),
            Some(size(0, 0))
        );
        assert_eq!(
            fitted_inner_size(size(0, 0), size(0, 0), size(1920, 1032)),
            None
        );
    }
}
