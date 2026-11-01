//! Where the scorecard window opens: last where it was left, if that still fits on a screen, or
//! sized to the main screen. The position is kept in the database.

use crate::{lock, App};
use std::sync::mpsc;
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// Default width of the scorecard window: the page's content plus a little room on each side.
const MAIN_WIDTH: f64 = 1085.0;
/// Default height of the scorecard window, on a screen tall enough: the whole scorecard, no scrolling.
const MAIN_HEIGHT: f64 = 1195.0;
/// Settings key: the scorecard window's last position and size, "x,y,width,height" in points.
pub(crate) const MAIN_WINDOW_KEY: &str = "window.main";

/// A screen rectangle in points (logical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl Rect {
    fn parse(saved: &str) -> Option<Rect> {
        let parts: Vec<f64> = saved.split(',').map(|p| p.trim().parse().ok()).collect::<Option<_>>()?;
        match parts[..] {
            [x, y, width, height] if width > 0.0 && height > 0.0 => Some(Rect { x, y, w: width, h: height }),
            _ => None,
        }
    }

    /// A monitor's usable area (below the menu bar, above the Dock), in points.
    fn work_area(m: &tauri::Monitor) -> Rect {
        let (a, scale) = (m.work_area(), m.scale_factor());
        Rect {
            x: f64::from(a.position.x) / scale,
            y: f64::from(a.position.y) / scale,
            w: f64::from(a.size.width) / scale,
            h: f64::from(a.size.height) / scale,
        }
    }

    /// Whether `self` sits entirely inside `area` (a pixel of slack for rounding).
    fn inside(&self, area: &Rect) -> bool {
        self.x >= area.x - 1.0
            && self.y >= area.y - 1.0
            && self.x + self.w <= area.x + area.w + 1.0
            && self.y + self.h <= area.y + area.h + 1.0
    }
}

/// The best fit for a screen: [`MAIN_WIDTH`] by [`MAIN_HEIGHT`], or as much of that as it has, centered.
fn best_fit(area: &Rect) -> Rect {
    let (w, h) = (MAIN_WIDTH.min(area.w), MAIN_HEIGHT.min(area.h));
    Rect { x: area.x + (area.w - w) / 2.0, y: area.y + (area.h - h) / 2.0, w, h }
}

/// Where the scorecard opens: where you left it, if that still fits fully on a screen; otherwise
/// the best fit for the main screen. `None` when no screen can be read (the config size stands).
fn main_placement(saved: Option<Rect>, areas: &[Rect], primary: Option<Rect>) -> Option<Rect> {
    if let Some(r) = saved.filter(|r| areas.iter().any(|a| r.inside(a))) {
        return Some(r);
    }
    primary.or_else(|| areas.first().copied()).map(|a| best_fit(&a))
}

/// Opens the scorecard where it was last left, or sized to the screen the first time.
pub(crate) fn place_main(handle: &AppHandle, saved: Option<&str>) {
    let Some(w) = handle.get_webview_window("main") else { return };
    let areas: Vec<Rect> = w.available_monitors().unwrap_or_default().iter().map(Rect::work_area).collect();
    let primary = w.primary_monitor().ok().flatten().as_ref().map(Rect::work_area);
    let Some(r) = main_placement(saved.and_then(Rect::parse), &areas, primary) else { return };
    let _ = w.set_size(tauri::LogicalSize::new(r.w, r.h));
    let _ = w.set_position(tauri::LogicalPosition::new(r.x, r.y));
}

/// The scorecard's position and size as saved: "x,y,width,height" in points.
fn placement_string(pos: (i32, i32), size: (u32, u32), scale: f64) -> String {
    format!(
        "{},{},{},{}",
        f64::from(pos.0) / scale,
        f64::from(pos.1) / scale,
        f64::from(size.0) / scale,
        f64::from(size.1) / scale
    )
}

/// How long a quit waits for the placement to be written. A sync pass can hold the writer far longer.
const EXIT_SAVE_WAIT: Duration = Duration::from_millis(1500);

/// Remembers the scorecard's position and size (outer frame, in points) for next launch. The window
/// is read here, but the write waits for the writer on its own thread, so a long sync pass never
/// freezes the window. The receiver hears when it's written.
pub(crate) fn save_main_placement(handle: &AppHandle) -> Option<mpsc::Receiver<()>> {
    let w = handle.get_webview_window("main")?;
    if w.is_minimized().unwrap_or(false) || w.is_fullscreen().unwrap_or(false) {
        return None;
    }
    let (Ok(pos), Ok(size), Ok(scale)) = (w.outer_position(), w.outer_size(), w.scale_factor()) else { return None };
    let r = placement_string((pos.x, pos.y), (size.width, size.height), scale);
    let app = std::sync::Arc::clone(&*handle.try_state::<std::sync::Arc<App>>()?);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = lock(&app.writer).set_setting(MAIN_WINDOW_KEY, &r);
        let _ = tx.send(());
    });
    Some(rx)
}

/// [`save_main_placement`] for a quit: waits a moment for the write, but never for a whole sync pass.
pub(crate) fn save_main_placement_before_exit(handle: &AppHandle) {
    // A tray Quit waits here and then again on the exit it asks for: once is enough.
    static WAITED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if WAITED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    if let Some(done) = save_main_placement(handle) {
        let _ = done.recv_timeout(EXIT_SAVE_WAIT);
    }
}

#[cfg(test)]
mod tests {
    use super::{main_placement, placement_string, Rect, MAIN_HEIGHT, MAIN_WIDTH};

    const LAPTOP: Rect = Rect { x: 0.0, y: 25.0, w: 1512.0, h: 920.0 };
    const SIDE: Rect = Rect { x: 1512.0, y: 0.0, w: 1920.0, h: 1055.0 };

    #[test]
    fn the_first_open_fills_the_screen_height_and_centers() {
        let r = main_placement(None, &[LAPTOP], Some(LAPTOP)).unwrap();
        assert_eq!(r, Rect { x: (1512.0 - MAIN_WIDTH) / 2.0, y: 25.0, w: MAIN_WIDTH, h: 920.0 });
    }

    #[test]
    fn a_big_screen_gets_the_full_default_size_centered() {
        let big = Rect { x: 0.0, y: 25.0, w: 2560.0, h: 1415.0 };
        let r = main_placement(None, &[big], Some(big)).unwrap();
        assert_eq!(r, Rect { x: (2560.0 - MAIN_WIDTH) / 2.0, y: 135.0, w: MAIN_WIDTH, h: MAIN_HEIGHT });
    }

    #[test]
    fn a_saved_window_reopens_as_you_left_it_on_any_screen() {
        let saved = Rect::parse("1600,40,1200,900");
        assert_eq!(main_placement(saved, &[LAPTOP, SIDE], Some(LAPTOP)), saved);
    }

    #[test]
    fn a_saved_window_that_no_longer_fits_goes_back_to_the_best_fit() {
        let best = main_placement(None, &[LAPTOP], Some(LAPTOP));
        // Left on a monitor that's gone, too tall for this one, or hanging off the edge.
        for saved in ["1600,40,1200,900", "100,25,900,1400", "1000,25,900,600"] {
            assert_eq!(main_placement(Rect::parse(saved), &[LAPTOP], Some(LAPTOP)), best, "{saved}");
        }
        assert_eq!(Rect::parse("nonsense"), None);
        assert_eq!(Rect::parse("1,2,0,4"), None);
    }

    #[test]
    fn a_narrow_screen_caps_the_width() {
        let small = Rect { x: 0.0, y: 0.0, w: 800.0, h: 600.0 };
        assert_eq!(main_placement(None, &[small], None).map(|r| r.w), Some(800.0));
    }

    #[test]
    fn the_placement_is_saved_in_points() {
        assert_eq!(placement_string((200, 100), (2400, 1800), 2.0), "100,50,1200,900");
        let saved = placement_string((-30, 50), (1085, 900), 1.0);
        assert_eq!(Rect::parse(&saved), Some(Rect { x: -30.0, y: 50.0, w: 1085.0, h: 900.0 }));
    }
}
