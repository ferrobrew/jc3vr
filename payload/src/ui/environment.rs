//! Environment controls and read-only time-of-day and weather observations.
//!
//! Controls reads/writes the `WorldTime` and `Weather` singletons directly, and fires the named
//! weather events for pinned states. The engine's day-cycle and `WeatherController` keep running,
//! so each value is re-read every frame and the raw scalars drift back unless held. Status copies
//! the observed scalars before rendering and does not retain native references.

use std::ffi::CStr;

use ::jc3gi::{
    self as native_jc3gi,
    environment::{LandscapeManager, Weather, WeatherController, WorldTime},
};

/// Render the Environment tab body.
pub(crate) fn render(ui: &mut egui::Ui) {
    fn time_of_day_ui(ui: &mut egui::Ui) {
        #[cfg(test)]
        if test_game::installed() {
            ui.label("WorldTime singleton unavailable.");
            return;
        }
        let wt = unsafe { WorldTime::get() };
        let Some(wt) = wt else {
            ui.label("WorldTime singleton unavailable.");
            return;
        };
        let mut hour = wt.m_CurrentTimeOfDay;
        if ui
            .add(egui::Slider::new(&mut hour, 0.0..=24.0).text("Hour"))
            .changed()
        {
            // SetTimeOfDay fmods to 24, fires the per-hour event, and respects the pause field.
            unsafe { wt.SetTimeOfDay(hour) };
        }
        ui.add(
            egui::Slider::new(&mut wt.m_SpeedMultiplicator, 0.0..=20.0)
                .text("Day-cycle speed (0 = frozen)"),
        );
    }

    fn weather_ui(ui: &mut egui::Ui) {
        // The named events the WeatherController subscribes: the engine-sanctioned way to *pin* a
        // weather state, where the direct scalar writes below drift back each frame.
        ui.horizontal(|ui| {
            for (label, event) in WEATHER_EVENTS {
                if ui.button(*label).clicked() {
                    jc3gi::event::send_event_msg(event);
                }
            }
        });
        let Some(w) = weather() else {
            ui.label("Weather (LandscapeManager -> Atmosphere -> GetWeather) unavailable.");
            return;
        };
        let mut severity = w.m_Severity;
        if ui
            .add(
                egui::Slider::new(&mut severity, 0.0..=4.0)
                    .text("Severity (~0.1 clear .. ~4 storm)"),
            )
            .changed()
        {
            unsafe { w.SetSeverity(severity) };
        }
        ui.add(egui::Slider::new(&mut w.m_RainIntensity, 0.0..=1.0).text("Rain intensity"));
        ui.add(egui::Slider::new(&mut w.m_SnowRatio, 0.0..=1.0).text("Snow ratio"));
        ui.label(
            "The WeatherController pulls these back toward its own target each frame unless one \
             of the named weather events above holds the state.",
        );
    }

    egui::CollapsingHeader::new("Time of day")
        .default_open(true)
        .show(ui, time_of_day_ui);
    egui::CollapsingHeader::new("Weather")
        .default_open(true)
        .show(ui, weather_ui);
}

/// Render the observed native scalars without controls or commands.
pub(crate) fn status(ui: &mut egui::Ui) {
    let snapshot = observed_snapshot();
    match snapshot.time {
        Some(time) => {
            ui.label(format!("Live time of day: hour {:.2}", time.hour));
            ui.label(format!("Live day-cycle speed: {:.2}", time.speed));
        }
        None => {
            ui.label("WorldTime singleton unavailable.");
        }
    }
    match snapshot.weather {
        Some(weather) => {
            ui.label(format!("Live weather severity: {:.2}", weather.severity));
            ui.label(format!("Live rain intensity: {:.2}", weather.rain));
            ui.label(format!("Live snow ratio: {:.2}", weather.snow));
        }
        None => {
            ui.label("Weather (LandscapeManager -> Atmosphere -> GetWeather) unavailable.");
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct ObservedSnapshot {
    time: Option<TimeSnapshot>,
    weather: Option<WeatherSnapshot>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TimeSnapshot {
    hour: f32,
    speed: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct WeatherSnapshot {
    severity: f32,
    rain: f32,
    snow: f32,
}

fn observed_snapshot() -> ObservedSnapshot {
    #[cfg(test)]
    if let Some(snapshot) = test_game::observed_snapshot() {
        return snapshot;
    }
    let time = unsafe { WorldTime::get() }.map(|time| TimeSnapshot {
        hour: time.m_CurrentTimeOfDay,
        speed: time.m_SpeedMultiplicator,
    });
    let weather = weather().map(|weather| WeatherSnapshot {
        severity: weather.m_Severity,
        rain: weather.m_RainIntensity,
        snow: weather.m_SnowRatio,
    });
    ObservedSnapshot { time, weather }
}

/// The named weather events the [`WeatherController`] subscribes in its `Init`, as button label
/// and event name (see the `WeatherController::EVENT_*` docs for what each pins).
const WEATHER_EVENTS: &[(&str, &CStr)] = &[
    ("Sunny", WeatherController::EVENT_SUNNY),
    ("Rain", WeatherController::EVENT_RAIN),
    ("Snow", WeatherController::EVENT_SNOW),
    ("Restore", WeatherController::EVENT_RESTORE),
    ("Instant", WeatherController::EVENT_INSTANT),
];

/// Resolve the live `Weather`: `LandscapeManager` -> `m_Atmosphere` -> `GetWeather`.
fn weather() -> Option<&'static mut Weather> {
    #[cfg(test)]
    if test_game::installed() {
        return None;
    }
    let atmosphere = unsafe { LandscapeManager::get()? }.m_Atmosphere;
    let weather = unsafe { atmosphere.as_mut()?.GetWeather() };
    unsafe { weather.as_mut() }
}

// The wrapper retains the historical command expression and the native fallback in test builds.
mod jc3gi {
    pub(crate) mod event {
        use std::ffi::CStr;

        use crate::ui::environment::native_jc3gi;
        #[cfg(test)]
        use crate::ui::environment::test_game;

        pub(crate) fn send_event_msg(event: &CStr) {
            #[cfg(test)]
            if test_game::record_event(event) {
                return;
            }
            native_jc3gi::event::send_event_msg(event);
        }
    }
}

#[cfg(test)]
pub(crate) mod test_game {
    use std::{cell::RefCell, ffi::CStr};

    use crate::ui::environment::{ObservedSnapshot, TimeSnapshot, WeatherSnapshot};

    #[derive(Clone, Debug, Default, PartialEq)]
    pub(crate) struct Snapshot {
        pub time: Option<(f32, f32)>,
        pub weather: Option<(f32, f32, f32)>,
        pub events: Vec<String>,
    }

    pub(crate) struct Fixture {
        previous: Option<Snapshot>,
    }

    pub(crate) fn install(time: Option<(f32, f32)>, weather: Option<(f32, f32, f32)>) -> Fixture {
        let previous = INSTALLED.replace(Some(Snapshot {
            time,
            weather,
            events: Vec::new(),
        }));
        Fixture { previous }
    }

    pub(crate) fn snapshot() -> Option<Snapshot> {
        INSTALLED.with_borrow(Clone::clone)
    }

    pub(crate) fn installed() -> bool {
        INSTALLED.with_borrow(Option::is_some)
    }

    pub(super) fn observed_snapshot() -> Option<ObservedSnapshot> {
        INSTALLED.with_borrow(|state| {
            state.as_ref().map(|state| ObservedSnapshot {
                time: state.time.map(|(hour, speed)| TimeSnapshot { hour, speed }),
                weather: state.weather.map(|(severity, rain, snow)| WeatherSnapshot {
                    severity,
                    rain,
                    snow,
                }),
            })
        })
    }

    pub(crate) fn record_event(event: &CStr) -> bool {
        INSTALLED.with_borrow_mut(|state| {
            let Some(state) = state.as_mut() else {
                return false;
            };
            state.events.push(event.to_string_lossy().into_owned());
            true
        })
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            INSTALLED.replace(self.previous.take());
        }
    }

    thread_local! {
        static INSTALLED: RefCell<Option<Snapshot>> = const { RefCell::new(None) };
    }
}
