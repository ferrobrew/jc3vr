//! Row encoding: each [`Item`] becomes the row object one of the pause movie's settings widgets
//! draws. The encoding is pure data ([`RowSpec`]); the runtime copies it into an AS3 object.
//!
//! Headers, text, readouts, and buttons use the key-binding format, whose right-hand text is free
//! (`m_KeyText`) and whose selection either plays the refusal sound or, for a customisable row,
//! sends `get_key_for_action`, which the command detour turns into the button's action. Toggles and
//! choices use the toggle format's option picker, and sliders the slider format.

use jc3gi::ui::pause_ui::SettingsUI;
use smol_str::SmolStr;

use crate::pause_menu::model::{
    ids::RowId,
    page::{Item, Page},
    slider::slider_label,
};

/// The labels a toggle row offers, in the order of its values `0` and `1`.
pub(crate) const TOGGLE_LABELS: [&str; 2] = ["OFF", "ON"];

/// One row object, as the movie reads it.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct RowSpec {
    /// `m_Type`: the row's id, sent back with its value.
    pub(crate) id: i32,
    /// `m_Name`: the label (upper-cased by the movie).
    pub(crate) name: SmolStr,
    /// `m_Value`.
    pub(crate) value: i32,
    /// `m_DefaultValue`. Every row sets it, since the movie's reset-all reads it for every row.
    pub(crate) default: i32,
    /// The widget and its format-specific fields.
    pub(crate) widget: RowWidget,
}

/// A row's widget: its `m_Format` and the fields that format reads.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum RowWidget {
    /// The option picker: `m_Labels[i]` names `m_Values[i]`.
    Toggle {
        labels: Vec<SmolStr>,
        values: Vec<i32>,
    },
    /// The slider: an integer in `[m_MinValue, m_MaxValue]`.
    Slider { min: i32, max: i32 },
    /// The key-binding row: `m_KeyText` on the right, with an empty `m_Image` and the plain
    /// `m_Interaction`. `m_IsCustomizable` makes selecting it send `get_key_for_action` with
    /// `m_Action`, which is the row's id.
    Key {
        key_text: SmolStr,
        customizable: bool,
    },
}

impl RowWidget {
    /// The row's `m_Format`.
    pub(crate) fn format(&self) -> i32 {
        match self {
            RowWidget::Toggle { .. } => SettingsUI::WIDGET_TOGGLE,
            RowWidget::Slider { .. } => SettingsUI::WIDGET_SLIDER,
            RowWidget::Key { .. } => SettingsUI::WIDGET_KEY,
        }
    }
}

/// Encoding failures. These are page-model bugs (a page too large for the id band), reported
/// rather than drawn wrong.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum EncodeError {
    /// The page or item index does not fit the id band.
    IdOutOfBand { page: usize, item: usize },
}

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EncodeError::IdOutOfBand { page, item } => write!(
                f,
                "row encoding: item {item} of page {page} does not fit the row id band"
            ),
        }
    }
}

impl std::error::Error for EncodeError {}

/// Encode every item of `page` (registered at index `page_index`) as a row, reading each item's
/// current value.
pub(crate) fn encode_page(page_index: usize, page: &Page) -> Result<Vec<RowSpec>, EncodeError> {
    page.items
        .iter()
        .enumerate()
        .map(|(item_index, item)| {
            let id =
                RowId::new(page_index, item_index)
                    .encode()
                    .ok_or(EncodeError::IdOutOfBand {
                        page: page_index,
                        item: item_index,
                    })?;
            Ok(encode_item(id, item))
        })
        .collect()
}

/// Encode one item as the row with id `id`.
pub(crate) fn encode_item(id: i32, item: &Item) -> RowSpec {
    let key = |name: &SmolStr, key_text: SmolStr, customizable: bool| RowSpec {
        id,
        name: name.clone(),
        value: 0,
        default: 0,
        widget: RowWidget::Key {
            key_text,
            customizable,
        },
    };
    match item {
        Item::Header(title) => key(title, SmolStr::default(), false),
        Item::Text(text) => key(text, SmolStr::default(), false),
        Item::Readout(readout) => key(&readout.label, (readout.read)(), false),
        Item::Button(button) => key(&button.label, button.caption.clone(), true),
        Item::Toggle(toggle) => RowSpec {
            id,
            name: toggle.label.clone(),
            value: i32::from((toggle.get)()),
            default: i32::from(toggle.default),
            widget: RowWidget::Toggle {
                labels: TOGGLE_LABELS.iter().copied().map(SmolStr::new).collect(),
                values: vec![0, 1],
            },
        },
        Item::Choice(choice) => RowSpec {
            id,
            name: choice.label.clone(),
            value: index_value((choice.get)()),
            default: index_value(choice.default),
            widget: RowWidget::Toggle {
                labels: choice.options.iter().copied().map(SmolStr::new).collect(),
                values: (0..choice.options.len()).map(index_value).collect(),
            },
        },
        Item::Slider(slider) => {
            let value = (slider.get)();
            RowSpec {
                id,
                name: slider_label(&slider.label, slider.format, value),
                value: slider.range.notch_of(value),
                default: slider.range.notch_of(slider.default),
                widget: RowWidget::Slider {
                    min: 0,
                    max: slider.range.max_notch(),
                },
            }
        }
    }
}

/// A choice index as a row value. Indices are small; a value past `i32::MAX` saturates, which the
/// picker shows as no match.
fn index_value(index: usize) -> i32 {
    i32::try_from(index).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pause_menu::model::{
        page::{Button, ButtonOutcome, Choice, Readout, Slider, Toggle},
        slider::{SliderFormat, SliderRange},
    };

    fn sample_page() -> Page {
        Page::new("Sample")
            .header("Section")
            .text("Hello")
            .item(Toggle {
                label: "Flag".into(),
                get: || true,
                set: |_| {},
                default: false,
            })
            .item(Choice {
                label: "Mode".into(),
                options: &["SNAP", "SMOOTH", "OFF"],
                get: || 1,
                set: |_| {},
                default: 0,
            })
            .item(Slider {
                label: "Angle".into(),
                range: SliderRange::new(15.0, 90.0, 75),
                format: SliderFormat::new(0, "°"),
                get: || 30.0,
                set: |_| {},
                default: 45.0,
            })
            .item(Readout {
                label: "Height".into(),
                read: || "1.80 M".into(),
            })
            .item(Button {
                label: "Measure".into(),
                caption: "GO".into(),
                action: || ButtonOutcome::Stay,
            })
    }

    #[test]
    fn encodes_each_kind() {
        let rows = encode_page(2, &sample_page()).unwrap();
        let ids: Vec<i32> = (0..7)
            .map(|item| RowId::new(2, item).encode().unwrap())
            .collect();
        let key = |id, name: &str, key_text: &str, customizable| RowSpec {
            id,
            name: name.into(),
            value: 0,
            default: 0,
            widget: RowWidget::Key {
                key_text: key_text.into(),
                customizable,
            },
        };
        assert_eq!(
            rows,
            vec![
                key(ids[0], "Section", "", false),
                key(ids[1], "Hello", "", false),
                RowSpec {
                    id: ids[2],
                    name: "Flag".into(),
                    value: 1,
                    default: 0,
                    widget: RowWidget::Toggle {
                        labels: vec!["OFF".into(), "ON".into()],
                        values: vec![0, 1],
                    },
                },
                RowSpec {
                    id: ids[3],
                    name: "Mode".into(),
                    value: 1,
                    default: 0,
                    widget: RowWidget::Toggle {
                        labels: vec!["SNAP".into(), "SMOOTH".into(), "OFF".into()],
                        values: vec![0, 1, 2],
                    },
                },
                RowSpec {
                    id: ids[4],
                    name: "Angle  30°".into(),
                    value: 15,
                    default: 30,
                    widget: RowWidget::Slider { min: 0, max: 75 },
                },
                key(ids[5], "Height", "1.80 M", false),
                key(ids[6], "Measure", "GO", true),
            ]
        );
    }

    #[test]
    fn formats_match_the_widgets() {
        let rows = encode_page(0, &sample_page()).unwrap();
        let formats: Vec<i32> = rows.iter().map(|row| row.widget.format()).collect();
        assert_eq!(
            formats,
            vec![
                SettingsUI::WIDGET_KEY,
                SettingsUI::WIDGET_KEY,
                SettingsUI::WIDGET_TOGGLE,
                SettingsUI::WIDGET_TOGGLE,
                SettingsUI::WIDGET_SLIDER,
                SettingsUI::WIDGET_KEY,
                SettingsUI::WIDGET_KEY,
            ]
        );
    }

    #[test]
    fn row_values_are_never_negative() {
        // The list manager uses negative values as its own context-menu action codes.
        for row in encode_page(0, &sample_page()).unwrap() {
            assert!(row.value >= 0 && row.default >= 0);
            if let RowWidget::Toggle { values, .. } = &row.widget {
                assert!(values.iter().all(|&v| v >= 0));
            }
        }
    }

    #[test]
    fn rejects_pages_outside_the_band() {
        assert_eq!(
            encode_page(0x100, &sample_page()),
            Err(EncodeError::IdOutOfBand {
                page: 0x100,
                item: 0
            })
        );
    }
}
