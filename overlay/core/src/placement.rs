//! Panel placement: where the panel goes by default, and whether a saved position is still usable.
//!
//! All coordinates are physical pixels (what the window manager reports); logical panel sizes are
//! scaled by the monitor's scale factor. Kept in core so the geometry is unit-tested in WSL.

/// Panel size in logical pixels (matches `tauri.conf.json` and the CSS).
pub const PANEL_W: f64 = 380.0;
pub const PANEL_H: f64 = 300.0;
pub const PANEL_H_COLLAPSED: f64 = 64.0;
/// The header row (the drag handle), logical pixels.
pub const HEADER_H: f64 = 32.0;
/// Bottom-right default: keep clear of the minimap (about 30% of the screen height wide) with a margin.
const MINIMAP_SHARE: f64 = 0.30;
const MARGIN: f64 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Rect { x, y, w, h }
    }
    pub fn right(&self) -> i32 {
        self.x.saturating_add(self.w)
    }
    pub fn bottom(&self) -> i32 {
        self.y.saturating_add(self.h)
    }
    /// `other` lies entirely inside `self`.
    pub fn contains_rect(&self, other: &Rect) -> bool {
        other.x >= self.x && other.y >= self.y && other.right() <= self.right() && other.bottom() <= self.bottom()
    }
}

/// A monitor: bounds and work area (bounds minus the taskbar) in physical pixels, plus its scale factor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub bounds: Rect,
    pub work: Rect,
    pub scale: f64,
}

impl Screen {
    /// A screen whose work area is the whole screen (no taskbar known).
    pub fn new(x: i32, y: i32, w: i32, h: i32, scale: f64) -> Self {
        let bounds = Rect::new(x, y, w, h);
        Screen { bounds, work: bounds, scale }
    }
    pub fn with_work_area(mut self, x: i32, y: i32, w: i32, h: i32) -> Self {
        self.work = Rect::new(x, y, w, h);
        self
    }
    fn px(&self, logical: f64) -> i32 {
        (logical * self.scale).round() as i32
    }
}

/// Where the panel goes when there is no usable saved position: bottom-right of the primary
/// screen, just left of where the minimap sits (the minimap scales with the full screen height),
/// and above the taskbar so nothing is hidden while the desktop is showing.
pub fn default_position(primary: &Screen) -> (i32, i32) {
    let (b, work) = (primary.bounds, primary.work);
    let (w, h, margin) = (primary.px(PANEL_W), primary.px(PANEL_H), primary.px(MARGIN));
    let minimap = (b.h as f64 * MINIMAP_SHARE) as i32;
    let x = b.x + (b.w - minimap - w - margin).max(0);
    let y = (work.bottom() - h - margin).max(work.y);
    (x, y)
}

/// A saved position is usable when the panel's header (the drag handle) is entirely on one of the
/// screens, so the panel is visible and can still be moved. Anything else, such as a monitor that
/// was unplugged, a resolution change or garbage in the settings file, falls back to the default.
pub fn saved_position_usable(x: i32, y: i32, screens: &[Screen]) -> bool {
    screens.iter().any(|s| {
        let header = Rect::new(x, y, s.px(PANEL_W), s.px(HEADER_H));
        s.bounds.contains_rect(&header)
    })
}

/// The position to start at: the saved one when usable, else the default on the primary screen.
/// `None` when nothing is known about the monitors (leave the window where the OS put it).
pub fn startup_position(saved: Option<(i32, i32)>, primary: Option<&Screen>, screens: &[Screen]) -> Option<(i32, i32)> {
    match saved {
        Some((x, y)) if saved_position_usable(x, y, screens) => Some((x, y)),
        _ => primary.or(screens.first()).map(default_position),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The dev machine: 5120x2160 at 150%.
    fn wide() -> Screen {
        Screen::new(0, 0, 5120, 2160, 1.5)
    }

    #[test]
    fn default_is_bottom_right_clear_of_the_minimap() {
        assert_eq!(default_position(&wide()), (5120 - 648 - 570 - 18, 2160 - 450 - 18));
        assert_eq!(default_position(&Screen::new(0, 0, 1920, 1080, 1.0)), (1920 - 324 - 380 - 12, 1080 - 300 - 12));
    }

    #[test]
    fn default_stays_above_the_taskbar() {
        // The dev machine's taskbar is 72 px tall; in game (borderless) the work area still excludes it.
        let s = wide().with_work_area(0, 0, 5120, 2088);
        assert_eq!(default_position(&s), (3884, 2088 - 450 - 18));
        // A taskbar on the left shifts the work area but not the minimap-relative x.
        let s = Screen::new(0, 0, 1920, 1080, 1.0).with_work_area(60, 0, 1860, 1080);
        assert_eq!(default_position(&s), (1204, 768));
    }

    #[test]
    fn default_respects_a_monitor_offset() {
        let second = Screen::new(5120, 0, 1920, 1080, 1.0);
        assert_eq!(default_position(&second), (5120 + 1204, 768));
    }

    #[test]
    fn default_never_goes_negative_on_a_tiny_screen() {
        assert_eq!(default_position(&Screen::new(0, 0, 400, 200, 1.0)), (0, 0));
    }

    #[test]
    fn saved_position_on_screen_is_kept() {
        let screens = [wide()];
        assert!(saved_position_usable(3803, 322, &screens));
        assert!(saved_position_usable(0, 0, &screens));
        assert!(saved_position_usable(5120 - 570, 2160 - 48, &screens));
        assert_eq!(startup_position(Some((3803, 322)), Some(&wide()), &screens), Some((3803, 322)));
    }

    #[test]
    fn saved_position_off_screen_falls_back_to_default() {
        let screens = [wide()];
        let default = default_position(&wide());
        for (x, y) in [(-1000, 322), (5000, 322), (100, 2200), (100, 2160 - 20), (100, -10), (i32::MAX, 0)] {
            assert!(!saved_position_usable(x, y, &screens), "({x},{y}) should be rejected");
            assert_eq!(startup_position(Some((x, y)), Some(&wide()), &screens), Some(default));
        }
    }

    #[test]
    fn missing_saved_position_uses_the_default() {
        assert_eq!(startup_position(None, Some(&wide()), &[wide()]), Some(default_position(&wide())));
    }

    #[test]
    fn saved_position_on_a_second_monitor_is_kept() {
        let screens = [wide(), Screen::new(5120, 0, 1920, 1080, 1.0)];
        assert!(saved_position_usable(5200, 100, &screens));
        assert!(!saved_position_usable(5120 + 1920 - 100, 100, &screens));
    }

    #[test]
    fn nothing_known_about_monitors() {
        assert!(!saved_position_usable(10, 10, &[]));
        assert_eq!(startup_position(Some((10, 10)), None, &[]), None);
        // A screen list without a primary still yields a default.
        assert_eq!(startup_position(None, None, &[wide()]), Some(default_position(&wide())));
    }
}
