//! Environment controls and read-only time-of-day and weather observations.
//!
//! Controls reads/writes the `WorldTime` and `Weather` singletons directly, and fires the named
//! weather events for pinned states. The engine's day-cycle and `WeatherController` keep running,
//! so each value is re-read every frame and the raw scalars drift back unless held. Status copies
//! the observed scalars before rendering and does not retain native references.

use std::ffi::CStr;

use jc3gi::environment::{LandscapeManager, Weather, WeatherController, WorldTime};

/// Render the Environment tab body.
pub(crate) fn render(ui: &mut egui::Ui) {
    fn time_of_day_ui(ui: &mut egui::Ui) {
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
    let atmosphere = unsafe { LandscapeManager::get()? }.m_Atmosphere;
    let weather = unsafe { atmosphere.as_mut()?.GetWeather() };
    unsafe { weather.as_mut() }
}
