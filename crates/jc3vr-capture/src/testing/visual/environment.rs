//! The visual diagnostic's environment override: a fixed hour with pinned weather (clear, rain, or
//! snow) and a frozen day cycle. The first override saves the original world time once, and the
//! saved values are restored when the visual control ends.

use jc3gi::environment::{LandscapeManager, Weather, WeatherController, WorldTime};
use jc3vr_core::hooks::in_gameplay;
use serde::{Deserialize, Serialize};

/// How far the observed hour may differ from the requested hour after `SetTimeOfDay`.
pub(crate) const HOUR_TOLERANCE: f32 = 1e-4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WeatherControl {
    Sunny,
    Rain,
    Snow,
    Ambient,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct EnvironmentMetadata {
    pub hour: f32,
    pub(crate) day_cycle_speed: f32,
    pub(crate) weather_severity: f32,
    pub(crate) weather_control: WeatherControl,
}

impl EnvironmentMetadata {
    #[cfg(test)]
    pub(crate) fn new(
        hour: f32,
        day_cycle_speed: f32,
        weather_severity: f32,
        weather_control: WeatherControl,
    ) -> Self {
        Self {
            hour,
            day_cycle_speed,
            weather_severity,
            weather_control,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EnvironmentWeather {
    Sunny,
    Rain,
    Snow,
}

impl EnvironmentWeather {
    /// The named weather event that pins this weather.
    fn event(self) -> &'static std::ffi::CStr {
        match self {
            Self::Sunny => WeatherController::EVENT_SUNNY,
            Self::Rain => WeatherController::EVENT_RAIN,
            Self::Snow => WeatherController::EVENT_SNOW,
        }
    }

    fn control(self) -> WeatherControl {
        match self {
            Self::Sunny => WeatherControl::Sunny,
            Self::Rain => WeatherControl::Rain,
            Self::Snow => WeatherControl::Snow,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentTable {
    hour: f32,
    weather: EnvironmentWeather,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnvironmentRequest {
    hour: f32,
    weather: EnvironmentWeather,
}

impl EnvironmentRequest {
    pub(crate) fn new(hour: f32, weather: EnvironmentWeather) -> Result<Self, EnvironmentError> {
        if !hour.is_finite() {
            return Err(EnvironmentError::NonFiniteHour);
        }
        if !(0.0..24.0).contains(&hour) {
            return Err(EnvironmentError::HourOutOfRange);
        }
        Ok(Self { hour, weather })
    }
}

impl TryFrom<EnvironmentTable> for EnvironmentRequest {
    type Error = EnvironmentError;

    fn try_from(table: EnvironmentTable) -> Result<Self, Self::Error> {
        Self::new(table.hour, table.weather)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum EnvironmentError {
    NotInGameplay,
    WorldTimeUnavailable,
    WeatherUnavailable,
    NonFiniteHour,
    HourOutOfRange,
    HourNotApplied { requested: f32, observed: f32 },
}

impl std::fmt::Display for EnvironmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotInGameplay => write!(f, "visual environment: the game is not in gameplay"),
            Self::WorldTimeUnavailable => {
                write!(
                    f,
                    "visual environment: the WorldTime singleton is unavailable"
                )
            }
            Self::WeatherUnavailable => {
                write!(
                    f,
                    "visual environment: the Weather singleton is unavailable"
                )
            }
            Self::NonFiniteHour => write!(f, "visual environment: the hour is not finite"),
            Self::HourOutOfRange => {
                write!(f, "visual environment: the hour is outside [0, 24)")
            }
            Self::HourNotApplied {
                requested,
                observed,
            } => write!(
                f,
                "visual environment: SetTimeOfDay applied {observed} h, not the requested \
                 {requested} h"
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldTimeSnapshot {
    hour: f32,
    speed: f32,
    pause: f32,
}

impl WorldTimeSnapshot {
    #[cfg(test)]
    pub(crate) fn new(hour: f32, speed: f32, pause: f32) -> Self {
        Self { hour, speed, pause }
    }

    pub(crate) fn capture() -> Result<Self, EnvironmentError> {
        let time = unsafe { WorldTime::get() }.ok_or(EnvironmentError::WorldTimeUnavailable)?;
        Ok(Self {
            hour: time.m_CurrentTimeOfDay,
            speed: time.m_SpeedMultiplicator,
            pause: time.m_PauseTimeOfDay,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnvironmentControl {
    saved: WorldTimeSnapshot,
    observed: Option<EnvironmentMetadata>,
}

impl EnvironmentControl {
    pub(crate) fn new(saved: WorldTimeSnapshot, observed: EnvironmentMetadata) -> Self {
        Self {
            saved,
            observed: Some(observed),
        }
    }

    pub(crate) fn pending(saved: WorldTimeSnapshot) -> Self {
        Self {
            saved,
            observed: None,
        }
    }

    pub(crate) fn saved(&self) -> WorldTimeSnapshot {
        self.saved
    }

    pub(crate) fn observed(&self) -> Option<EnvironmentMetadata> {
        self.observed
    }

    pub(crate) fn update(&mut self, observed: EnvironmentMetadata) {
        self.observed = Some(observed);
    }

    pub(crate) fn restored(&mut self, observed: EnvironmentMetadata) {
        self.observed = Some(observed);
    }

    pub(crate) fn needs_restore(&self) -> bool {
        self.observed
            .is_none_or(|observed| observed.weather_control != WeatherControl::Ambient)
    }
}

pub(crate) fn available() -> Result<(), EnvironmentError> {
    if !in_gameplay() {
        return Err(EnvironmentError::NotInGameplay);
    }
    if unsafe { WorldTime::get() }.is_none() {
        return Err(EnvironmentError::WorldTimeUnavailable);
    }
    if weather().is_none() {
        return Err(EnvironmentError::WeatherUnavailable);
    }
    Ok(())
}

pub(crate) fn apply(request: &EnvironmentRequest) -> Result<EnvironmentMetadata, EnvironmentError> {
    available()?;
    let time = unsafe { WorldTime::get() }.ok_or(EnvironmentError::WorldTimeUnavailable)?;
    time.m_SpeedMultiplicator = 0.0;
    unsafe { time.SetTimeOfDay(request.hour) };
    let observed_hour = time.m_CurrentTimeOfDay;
    if (observed_hour - request.hour).abs() > HOUR_TOLERANCE {
        return Err(EnvironmentError::HourNotApplied {
            requested: request.hour,
            observed: observed_hour,
        });
    }
    jc3gi::event::send_event_msg(request.weather.event());
    jc3gi::event::send_event_msg(WeatherController::EVENT_INSTANT);
    observe(request.weather.control()).ok_or(EnvironmentError::WeatherUnavailable)
}

/// Set the hour and pin the weather without holding the day cycle or saving anything to restore:
/// for a scene being staged with the clock running, so the weather and the time of day have run
/// for a while before a configuration holds them. The configuration that follows applies the same
/// request with [`apply`], and its restore returns the weather to the ambient system.
pub fn preview(request: &EnvironmentRequest) -> Result<(), EnvironmentError> {
    available()?;
    let time = unsafe { WorldTime::get() }.ok_or(EnvironmentError::WorldTimeUnavailable)?;
    unsafe { time.SetTimeOfDay(request.hour) };
    jc3gi::event::send_event_msg(request.weather.event());
    jc3gi::event::send_event_msg(WeatherController::EVENT_INSTANT);
    Ok(())
}

pub(crate) fn restore(
    snapshot: WorldTimeSnapshot,
) -> Result<EnvironmentMetadata, EnvironmentError> {
    let time = unsafe { WorldTime::get() }.ok_or(EnvironmentError::WorldTimeUnavailable)?;
    if weather().is_none() {
        return Err(EnvironmentError::WeatherUnavailable);
    }
    time.m_PauseTimeOfDay = snapshot.pause;
    time.m_SpeedMultiplicator = snapshot.speed;
    unsafe { time.SetTimeOfDay(snapshot.hour) };
    let observed_hour = time.m_CurrentTimeOfDay;
    if (observed_hour - snapshot.hour).abs() > HOUR_TOLERANCE {
        return Err(EnvironmentError::HourNotApplied {
            requested: snapshot.hour,
            observed: observed_hour,
        });
    }
    jc3gi::event::send_event_msg(WeatherController::EVENT_RESTORE);
    observe(WeatherControl::Ambient).ok_or(EnvironmentError::WeatherUnavailable)
}

pub(crate) fn observe(weather_control: WeatherControl) -> Option<EnvironmentMetadata> {
    let time = unsafe { WorldTime::get() }?;
    let weather = weather()?;
    Some(EnvironmentMetadata {
        hour: time.m_CurrentTimeOfDay,
        day_cycle_speed: time.m_SpeedMultiplicator,
        weather_severity: weather.m_Severity,
        weather_control,
    })
}

fn weather() -> Option<&'static mut Weather> {
    let atmosphere = unsafe { LandscapeManager::get()? }.m_Atmosphere;
    let weather = unsafe { atmosphere.as_mut()?.GetWeather() };
    unsafe { weather.as_mut() }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn hours_are_finite_and_within_the_day() {
        for hour in [0.0, 0.5, 12.0, 23.999] {
            assert!(EnvironmentRequest::new(hour, EnvironmentWeather::Sunny).is_ok());
        }
        for hour in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(
                EnvironmentRequest::new(hour, EnvironmentWeather::Sunny),
                Err(EnvironmentError::NonFiniteHour)
            );
        }
        for hour in [-0.001, 24.0, 25.0] {
            assert_eq!(
                EnvironmentRequest::new(hour, EnvironmentWeather::Sunny),
                Err(EnvironmentError::HourOutOfRange)
            );
        }
    }

    #[test]
    fn tables_require_hour_and_a_pinned_weather_and_reject_unknown_fields() {
        for (text, weather) in [
            ("hour = 1.0\nweather = 'rain'", EnvironmentWeather::Rain),
            ("hour = 1.0\nweather = 'snow'", EnvironmentWeather::Snow),
        ] {
            let table: EnvironmentTable = toml::from_str(text).unwrap();
            assert_eq!(
                EnvironmentRequest::try_from(table),
                Ok(EnvironmentRequest::new(1.0, weather).unwrap())
            );
        }
        let table: EnvironmentTable = toml::from_str("hour = 13.5\nweather = 'sunny'").unwrap();
        assert_eq!(
            EnvironmentRequest::try_from(table),
            Ok(EnvironmentRequest::new(13.5, EnvironmentWeather::Sunny).unwrap())
        );
        for text in [
            "weather = 'sunny'",
            "hour = 13.5",
            "hour = 13.5\nweather = 'storm'",
            "hour = 13.5\nweather = 'sunny'\nday_cycle_speed = 0.0",
        ] {
            assert!(toml::from_str::<EnvironmentTable>(text).is_err(), "{text}");
        }
    }

    #[test]
    fn repeated_requests_keep_the_first_saved_snapshot() {
        let saved = WorldTimeSnapshot::new(8.25, 1.5, 0.0);
        let mut control = EnvironmentControl::new(
            saved,
            EnvironmentMetadata::new(13.5, 0.0, 0.1, WeatherControl::Sunny),
        );
        control.update(EnvironmentMetadata::new(
            18.0,
            0.0,
            0.2,
            WeatherControl::Sunny,
        ));
        assert_eq!(control.saved(), saved);
        assert_eq!(control.observed().unwrap().hour, 18.0);
    }

    #[test]
    fn a_pending_override_needs_restoration_and_an_ambient_one_does_not() {
        let saved = WorldTimeSnapshot::new(8.25, 1.5, 0.0);
        let pending = EnvironmentControl::pending(saved);
        assert!(pending.needs_restore());
        let mut ambient = EnvironmentControl::new(
            saved,
            EnvironmentMetadata::new(8.25, 1.5, 0.4, WeatherControl::Ambient),
        );
        assert!(!ambient.needs_restore());
        ambient.update(EnvironmentMetadata::new(
            9.0,
            0.0,
            0.1,
            WeatherControl::Sunny,
        ));
        assert!(ambient.needs_restore());
    }
}
