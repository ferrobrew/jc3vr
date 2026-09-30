//! A demonstration page: one item of each kind, wired to state of its own that nothing else reads,
//! and a two-step flow, to prove the menu machinery end to end in game. The real pages replace it.

use std::{sync::OnceLock, time::Instant};

use parking_lot::Mutex;
use smol_str::{SmolStr, format_smolstr};

use crate::pause_menu::model::{
    flow::Flow,
    page::{Button, ButtonOutcome, Choice, Page, PageDef, Readout, Slider, Toggle},
    slider::{SliderFormat, SliderRange},
};

pub(super) const PAGE: PageDef = PageDef {
    entry: "VR: DEMO",
    build,
    on_close: Some(on_close),
};

/// The demonstration state.
struct Demo {
    toggle: bool,
    choice: usize,
    level: f32,
    presses: u32,
    flow: Flow,
}

static DEMO: Mutex<Demo> = Mutex::new(Demo {
    toggle: false,
    choice: 0,
    level: 5.0,
    presses: 0,
    flow: Flow::new(2),
});

const CHOICES: &[&str] = &["ALPHA", "BETA", "GAMMA"];

fn build() -> Page {
    let page = Page::new("VR demo").header("Widgets").item(Toggle {
        label: "Demo toggle".into(),
        get: || DEMO.lock().toggle,
        set: |value| DEMO.lock().toggle = value,
        default: false,
    });
    let page = page
        .item(Choice {
            label: "Demo choice".into(),
            options: CHOICES,
            get: || DEMO.lock().choice,
            set: |value| DEMO.lock().choice = value,
            default: 0,
        })
        .item(Slider {
            label: "Demo slider".into(),
            range: SliderRange::new(0.0, 10.0, 100),
            format: SliderFormat::new(1, ""),
            get: || DEMO.lock().level,
            set: |value| DEMO.lock().level = value,
            default: 5.0,
        })
        .item(Readout {
            label: "Open for".into(),
            read: uptime,
        })
        .item(Readout {
            label: "Button presses".into(),
            read: || format_smolstr!("{}", DEMO.lock().presses),
        })
        .header("Flow");
    let flow = DEMO.lock().flow;
    let page = page.text(format_smolstr!(
        "Step {} of {}",
        flow.step() + 1,
        flow.len()
    ));
    if flow.is_last() {
        page.item(Button {
            label: "Go back".into(),
            caption: "BACK".into(),
            action: || press(|flow| flow.back().moved()),
        })
        .item(Button {
            label: "Start over".into(),
            caption: "RESTART".into(),
            action: || press(|flow| flow.restart().moved()),
        })
    } else {
        page.item(Button {
            label: "Continue".into(),
            caption: "NEXT".into(),
            action: || press(|flow| flow.advance().moved()),
        })
    }
}

/// Count the press and move the flow; rebuild when the step changed.
fn press(step: fn(&mut Flow) -> bool) -> ButtonOutcome {
    let mut demo = DEMO.lock();
    demo.presses += 1;
    if step(&mut demo.flow) {
        ButtonOutcome::Rebuild
    } else {
        ButtonOutcome::Stay
    }
}

/// Seconds since the page was first built, as a value that changes every second.
fn uptime() -> SmolStr {
    static START: OnceLock<Instant> = OnceLock::new();
    let seconds = START.get_or_init(Instant::now).elapsed().as_secs();
    format_smolstr!("{seconds} S")
}

fn on_close() {
    let demo = DEMO.lock();
    tracing::info!(
        "pause menu: demo page closed (toggle {}, choice {}, slider {:.1}, {} presses, step {})",
        demo.toggle,
        CHOICES.get(demo.choice).copied().unwrap_or("?"),
        demo.level,
        demo.presses,
        demo.flow.step(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pause_menu::model::{page::Item, rows::encode_page};

    #[test]
    fn has_one_item_of_each_kind_and_encodes() {
        let page = build();
        let has = |kind: fn(&Item) -> bool| page.items.iter().any(kind);
        assert!(has(|i| matches!(i, Item::Header(_))));
        assert!(has(|i| matches!(i, Item::Text(_))));
        assert!(has(|i| matches!(i, Item::Toggle(_))));
        assert!(has(|i| matches!(i, Item::Choice(_))));
        assert!(has(|i| matches!(i, Item::Slider(_))));
        assert!(has(|i| matches!(i, Item::Readout(_))));
        assert!(has(|i| matches!(i, Item::Button(_))));
        assert_eq!(encode_page(0, &page).unwrap().len(), page.items.len());
    }
}
