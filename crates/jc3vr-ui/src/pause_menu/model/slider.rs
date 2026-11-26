//! Slider mapping: the pause movie's slider holds an integer between its minimum and maximum and
//! shows no number, so a float setting is mapped onto integer notches and its value is written into
//! the row's label instead.

use smol_str::{SmolStr, format_smolstr};

/// A slider's value range and notch count: the setting's `[min, max]` maps linearly onto the
/// integer notches `[0, steps]`. The movie's left and right step a tenth of the notch range, so
/// `steps` of 100 gives ten keyboard steps across the range; an integer setting uses
/// `max - min` notches for a one-to-one mapping.
#[derive(Copy, Clone, PartialEq, Debug)]
pub(crate) struct SliderRange {
    pub(crate) min: f32,
    pub(crate) max: f32,
    pub(crate) steps: u16,
}

impl SliderRange {
    /// A range over `[min, max]` with `steps` notches (at least one).
    pub(crate) const fn new(min: f32, max: f32, steps: u16) -> Self {
        Self {
            min,
            max,
            steps: if steps == 0 { 1 } else { steps },
        }
    }

    /// The notch nearest `value`, clamped into `[0, steps]`. A non-finite value maps to notch 0.
    pub(crate) fn notch_of(&self, value: f32) -> i32 {
        let span = self.max - self.min;
        if !value.is_finite() || span <= 0.0 || !span.is_finite() {
            return 0;
        }
        let t = ((value - self.min) / span).clamp(0.0, 1.0);
        (t * f32::from(self.steps)).round() as i32
    }

    /// The setting value at `notch`, clamped into the range (the movie clamps too, but a stale or
    /// hand-crafted command need not).
    pub(crate) fn value_at(&self, notch: i32) -> f32 {
        let span = self.max - self.min;
        if span <= 0.0 || !span.is_finite() {
            return self.min;
        }
        let notch = notch.clamp(0, i32::from(self.steps));
        // Multiply before dividing, so an integer range maps its notches to exact integers.
        self.min + span * notch as f32 / f32::from(self.steps)
    }

    /// The movie's `m_MaxValue` for this slider (its `m_MinValue` is always zero).
    pub(crate) fn max_notch(&self) -> i32 {
        i32::from(self.steps)
    }
}

/// How a slider's value reads in its row label.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) struct SliderFormat {
    /// Digits after the decimal point.
    pub(crate) decimals: u8,
    /// Text appended to the number, such as a unit (`"M"`, `"°"`).
    pub(crate) suffix: &'static str,
}

impl SliderFormat {
    /// A format with `decimals` digits after the point and `suffix` after the number.
    pub(crate) const fn new(decimals: u8, suffix: &'static str) -> Self {
        Self { decimals, suffix }
    }
}

/// The label a slider row shows: its name, then its current value in `format`. The movie
/// upper-cases it.
pub(crate) fn slider_label(label: &str, format: SliderFormat, value: f32) -> SmolStr {
    let decimals = usize::from(format.decimals);
    format_smolstr!("{label}  {value:.decimals$}{}", format.suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_the_ends_and_the_middle() {
        let range = SliderRange::new(10.0, 90.0, 100);
        assert_eq!(range.notch_of(10.0), 0);
        assert_eq!(range.notch_of(90.0), 100);
        assert_eq!(range.notch_of(50.0), 50);
        assert_eq!(range.value_at(0), 10.0);
        assert_eq!(range.value_at(100), 90.0);
        assert_eq!(range.value_at(50), 50.0);
    }

    #[test]
    fn clamps_out_of_range_values_and_notches() {
        let range = SliderRange::new(0.0, 1.0, 10);
        assert_eq!(range.notch_of(-3.0), 0);
        assert_eq!(range.notch_of(7.0), 10);
        assert_eq!(range.value_at(-1), 0.0);
        assert_eq!(range.value_at(11), 1.0);
    }

    #[test]
    fn rounds_to_the_nearest_notch() {
        let range = SliderRange::new(0.0, 1.0, 10);
        assert_eq!(range.notch_of(0.14), 1);
        assert_eq!(range.notch_of(0.16), 2);
    }

    #[test]
    fn integer_settings_map_one_to_one() {
        let range = SliderRange::new(15.0, 90.0, 75);
        for degrees in 15..=90 {
            let notch = range.notch_of(degrees as f32);
            assert_eq!(notch, degrees - 15);
            assert_eq!(range.value_at(notch), degrees as f32);
        }
    }

    #[test]
    fn every_notch_round_trips() {
        let range = SliderRange::new(-0.25, 2.5, 100);
        for notch in 0..=100 {
            assert_eq!(range.notch_of(range.value_at(notch)), notch);
        }
    }

    #[test]
    fn degenerate_ranges_are_safe() {
        let empty = SliderRange::new(1.0, 1.0, 10);
        assert_eq!(empty.notch_of(1.0), 0);
        assert_eq!(empty.value_at(5), 1.0);
        let inverted = SliderRange::new(2.0, 1.0, 10);
        assert_eq!(inverted.notch_of(1.5), 0);
        assert_eq!(inverted.value_at(5), 2.0);
        let no_steps = SliderRange::new(0.0, 1.0, 0);
        assert_eq!(no_steps.max_notch(), 1);
        assert_eq!(SliderRange::new(0.0, 1.0, 10).notch_of(f32::NAN), 0);
    }

    #[test]
    fn labels_carry_the_value_and_unit() {
        assert_eq!(
            slider_label("Turn angle", SliderFormat::new(0, "°"), 30.0),
            "Turn angle  30°"
        );
        assert_eq!(
            slider_label("Height", SliderFormat::new(2, " M"), 1.8),
            "Height  1.80 M"
        );
    }
}
