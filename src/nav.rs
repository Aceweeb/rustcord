//! Keyboard-only navigation: move a visible focus through the rail and the
//! channel list with arrows, open with Enter, leave with Escape.
//!
//! This is deliberately *not* the switcher (`switcher.rs`): nothing is typed and
//! no overlay appears, so the focus has to be drawn in place by the sidebar.
//! The model is pure: it owns only where the focus is, and reports what a key
//! should do, so the app can decide what "activate" means for a folder, a guild
//! or a channel.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NavArea {
    #[default]
    Rail,
    Channels,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavKey {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Escape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavOutcome {
    /// Nothing to do (unknown key for the area, or an empty list).
    Ignored,
    Moved,
    SwitchArea,
    Activate,
    Exit,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Navigator {
    pub active: bool,
    pub area: NavArea,
    pub rail: usize,
    pub channel: usize,
}

impl Navigator {
    /// Start in the rail, on the server (or folder) that is already selected.
    pub fn begin(&mut self, rail_index: usize, channel_index: usize) {
        self.active = true;
        self.area = NavArea::Rail;
        self.rail = rail_index;
        self.channel = channel_index;
    }

    pub fn stop(&mut self) {
        self.active = false;
    }

    /// Keep the cursors inside their lists when servers, channels or folders
    /// change underneath the mode.
    pub fn clamp(&mut self, rail_len: usize, channel_len: usize) {
        self.rail = self.rail.min(rail_len.saturating_sub(1));
        self.channel = self.channel.min(channel_len.saturating_sub(1));
    }

    pub fn switch_area(&mut self) {
        self.area = match self.area {
            NavArea::Rail => NavArea::Channels,
            NavArea::Channels => NavArea::Rail,
        };
    }

    /// Apply one key. `rail_len` counts rail items (folders included) and
    /// `channel_len` counts the selected server's navigable channels.
    pub fn handle(&mut self, key: NavKey, rail_len: usize, channel_len: usize) -> NavOutcome {
        if !self.active {
            return NavOutcome::Ignored;
        }
        self.clamp(rail_len, channel_len);

        match key {
            NavKey::Escape => {
                self.stop();
                NavOutcome::Exit
            }
            NavKey::Enter => NavOutcome::Activate,
            NavKey::Left => {
                if self.area == NavArea::Channels {
                    self.area = NavArea::Rail;
                    NavOutcome::SwitchArea
                } else {
                    NavOutcome::Ignored
                }
            }
            NavKey::Right => {
                if self.area == NavArea::Rail && channel_len > 0 {
                    self.area = NavArea::Channels;
                    NavOutcome::SwitchArea
                } else {
                    NavOutcome::Ignored
                }
            }
            NavKey::Up | NavKey::Down => {
                let len = match self.area {
                    NavArea::Rail => rail_len,
                    NavArea::Channels => channel_len,
                };
                if len == 0 {
                    return NavOutcome::Ignored;
                }
                let cursor = match self.area {
                    NavArea::Rail => &mut self.rail,
                    NavArea::Channels => &mut self.channel,
                };
                let delta = if key == NavKey::Up { -1 } else { 1 };
                *cursor = (*cursor as i32 + delta).rem_euclid(len as i32) as usize;
                NavOutcome::Moved
            }
        }
    }

    pub fn on_rail(&self, index: usize) -> bool {
        self.active && self.area == NavArea::Rail && self.rail == index
    }

    pub fn on_channel(&self, index: usize) -> bool {
        self.active && self.area == NavArea::Channels && self.channel == index
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begins_on_the_rail_and_wraps_clamped() {
        let mut nav = Navigator::default();
        nav.begin(2, 5);
        assert!(nav.active);
        assert_eq!(nav.area, NavArea::Rail);

        // Only three rail items now: the stale index is clamped to the last one,
        // and moving down from there wraps to the first.
        assert_eq!(nav.handle(NavKey::Down, 3, 4), NavOutcome::Moved);
        assert_eq!(nav.rail, 0, "clamped to 2, then wrapped");

        assert_eq!(nav.handle(NavKey::Down, 3, 4), NavOutcome::Moved);
        assert_eq!(nav.rail, 1);
        assert_eq!(nav.handle(NavKey::Up, 3, 4), NavOutcome::Moved);
        assert_eq!(nav.rail, 0);
        assert_eq!(nav.handle(NavKey::Up, 3, 4), NavOutcome::Moved);
        assert_eq!(nav.rail, 2, "wraps to the last item");
    }

    #[test]
    fn right_enters_the_channels_and_left_comes_back() {
        let mut nav = Navigator::default();
        nav.begin(0, 0);

        assert_eq!(nav.handle(NavKey::Right, 4, 3), NavOutcome::SwitchArea);
        assert_eq!(nav.area, NavArea::Channels);

        assert_eq!(nav.handle(NavKey::Down, 4, 3), NavOutcome::Moved);
        assert_eq!(nav.channel, 1);

        assert_eq!(nav.handle(NavKey::Left, 4, 3), NavOutcome::SwitchArea);
        assert_eq!(nav.area, NavArea::Rail);
        assert_eq!(
            nav.handle(NavKey::Left, 4, 3),
            NavOutcome::Ignored,
            "already on the rail"
        );
    }

    #[test]
    fn a_server_without_channels_cannot_be_entered() {
        let mut nav = Navigator::default();
        nav.begin(0, 0);
        assert_eq!(nav.handle(NavKey::Right, 2, 0), NavOutcome::Ignored);
        assert_eq!(nav.area, NavArea::Rail);
    }

    #[test]
    fn empty_lists_do_not_move_or_divide_by_zero() {
        let mut nav = Navigator::default();
        nav.begin(0, 0);
        assert_eq!(nav.handle(NavKey::Down, 0, 0), NavOutcome::Ignored);
        assert_eq!(nav.rail, 0);
    }

    #[test]
    fn enter_activates_wherever_the_focus_is_and_escape_leaves() {
        let mut nav = Navigator::default();
        nav.begin(1, 2);
        assert_eq!(nav.handle(NavKey::Enter, 2, 3), NavOutcome::Activate);

        assert_eq!(nav.handle(NavKey::Escape, 2, 3), NavOutcome::Exit);
        assert!(!nav.active);
        assert_eq!(nav.handle(NavKey::Down, 2, 3), NavOutcome::Ignored);
    }

    #[test]
    fn focus_helpers_follow_the_area() {
        let mut nav = Navigator::default();
        nav.begin(1, 0);
        assert!(nav.on_rail(1));
        assert!(!nav.on_rail(0));
        assert!(!nav.on_channel(0), "the channel focus is not live yet");

        nav.switch_area();
        assert!(nav.on_channel(0));
        assert!(!nav.on_rail(1));

        nav.stop();
        assert!(
            !nav.on_channel(0),
            "nothing is focused once the mode is off"
        );
    }
}
