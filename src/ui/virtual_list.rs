//! Variable-height row virtualisation for the message list.
//!
//! egui's `ScrollArea::show_rows` assumes uniform row heights, which clips
//! multi-line messages. This keeps a measured height per row instead, computes
//! where each row sits, and lets the caller draw only the rows that intersect
//! the viewport.
//!
//! Kept free of `egui` types so the layout maths is unit-tested without a
//! graphics context.

use std::ops::Range;

/// Default (estimated) height used until a row has actually been drawn.
pub const DEFAULT_ROW_HEIGHT: f32 = 52.0;

#[derive(Debug)]
pub struct RowMetrics {
    heights: Vec<f32>,
    default_height: f32,
}

impl Default for RowMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl RowMetrics {
    pub fn new() -> Self {
        Self {
            heights: Vec::new(),
            default_height: DEFAULT_ROW_HEIGHT,
        }
    }

    /// Grow to `len` rows, filling new rows with the default estimate.
    pub fn ensure_len(&mut self, len: usize) {
        if self.heights.len() < len {
            self.heights.resize(len, self.default_height);
        }
        self.heights.truncate(len);
    }

    pub fn set_height(&mut self, index: usize, height: f32) {
        if let Some(slot) = self.heights.get_mut(index) {
            // Clamp so a pathological measurement cannot blow up the layout.
            *slot = height.clamp(1.0, 20_000.0);
        }
    }

    pub fn height(&self, index: usize) -> f32 {
        self.heights
            .get(index)
            .copied()
            .unwrap_or(self.default_height)
    }

    /// Vertical offset of the top edge of `index`.
    pub fn offset_of(&self, index: usize) -> f32 {
        self.heights.iter().take(index).sum()
    }

    pub fn total(&self) -> f32 {
        self.heights.iter().sum()
    }

    pub fn len(&self) -> usize {
        self.heights.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heights.is_empty()
    }

    /// Rows intersecting `[top, bottom)`.
    ///
    /// Returns an empty range when nothing is visible, and clamps to the row
    /// count so a scrolled-past-end viewport cannot index out of bounds.
    pub fn range_for(&self, top: f32, bottom: f32) -> Range<usize> {
        let mut start = None;
        let mut end = self.heights.len();
        let mut y = 0.0f32;

        for (index, height) in self.heights.iter().enumerate() {
            let next = y + height;
            if start.is_none() && next > top {
                start = Some(index);
            }
            if next >= bottom {
                end = index + 1;
                break;
            }
            y = next;
        }

        match start {
            Some(start) if start < end => start..end,
            _ => 0..0,
        }
    }

    /// Drop all measurements, e.g. after switching channels.
    pub fn clear(&mut self) {
        self.heights.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uniform(len: usize, height: f32) -> RowMetrics {
        let mut metrics = RowMetrics::new();
        metrics.ensure_len(len);
        for index in 0..len {
            metrics.set_height(index, height);
        }
        metrics
    }

    #[test]
    fn offsets_and_total_follow_measured_heights() {
        let mut metrics = RowMetrics::new();
        metrics.ensure_len(3);
        metrics.set_height(0, 10.0);
        metrics.set_height(1, 20.0);
        metrics.set_height(2, 30.0);

        assert_eq!(metrics.offset_of(0), 0.0);
        assert_eq!(metrics.offset_of(1), 10.0);
        assert_eq!(metrics.offset_of(2), 30.0);
        assert_eq!(metrics.total(), 60.0);
    }

    #[test]
    fn new_rows_start_at_the_default_estimate() {
        let mut metrics = RowMetrics::new();
        metrics.ensure_len(2);
        assert_eq!(metrics.height(0), DEFAULT_ROW_HEIGHT);
        assert_eq!(metrics.height(1), DEFAULT_ROW_HEIGHT);
    }

    #[test]
    fn viewport_selects_only_intersecting_rows() {
        let metrics = uniform(100, 50.0);
        // Rows 0..=49; a 100px viewport covering rows 2..4.
        assert_eq!(metrics.range_for(100.0, 200.0), 2..4);
        assert_eq!(metrics.range_for(0.0, 50.0), 0..1);
    }

    #[test]
    fn viewport_in_the_middle_of_a_tall_row_still_includes_it() {
        let mut metrics = RowMetrics::new();
        metrics.ensure_len(3);
        metrics.set_height(0, 10.0);
        metrics.set_height(1, 400.0);
        metrics.set_height(2, 10.0);
        assert_eq!(metrics.range_for(200.0, 210.0), 1..2);
    }

    #[test]
    fn viewport_past_the_end_is_empty_not_panicking() {
        let metrics = uniform(5, 20.0);
        assert_eq!(metrics.range_for(10_000.0, 10_100.0), 0..0);
    }

    #[test]
    fn empty_list_has_no_visible_rows() {
        let metrics = RowMetrics::new();
        assert_eq!(metrics.range_for(0.0, 500.0), 0..0);
        assert_eq!(metrics.total(), 0.0);
        assert!(metrics.is_empty());
    }

    #[test]
    fn shrinking_the_list_drops_stale_measurements() {
        let mut metrics = uniform(10, 40.0);
        metrics.ensure_len(4);
        assert_eq!(metrics.len(), 4);
        assert_eq!(metrics.total(), 160.0);
    }

    #[test]
    fn heights_are_clamped_against_absurd_measurements() {
        let mut metrics = RowMetrics::new();
        metrics.ensure_len(1);
        metrics.set_height(0, f32::INFINITY);
        assert!(metrics.height(0) <= 20_000.0);
        metrics.set_height(0, -5.0);
        assert_eq!(metrics.height(0), 1.0);
    }

    #[test]
    fn measuring_a_10k_message_channel_is_cheap() {
        let metrics = uniform(10_000, 40.0);
        let start = std::time::Instant::now();
        for _ in 0..100 {
            let _ = metrics.range_for(200_000.0, 200_800.0);
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_millis() < 100,
            "100 viewport queries over 10k rows took {elapsed:?}"
        );
    }
}
