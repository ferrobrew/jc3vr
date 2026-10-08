//! The per-hand haptic mixer: every cue the hands are sent between two rendered frames, folded
//! into the one vibration each controller can play.
//!
//! OpenXR plays one vibration per controller at a time, and a new one replaces whatever is
//! playing, so the mixer decides what each hand plays at every flush:
//!
//! - The sustained sources (hums and tensions) each hold a level per hand until refreshed or until
//!   their hold runs out. Their levels sum, and the sum passes a soft limiter whose ceiling sits
//!   below full strength, so stacked sources never saturate the hand and a one-off cue still stands
//!   out above them.
//! - A one-off cue arriving plays at once. The strongest of the frame's arrivals by priority leads
//!   and sets the shape; the others and a cue still playing add a share of their amplitude, and the
//!   mod's sustained level a share beneath it, and the total passes the soft limiter at full
//!   strength. The game's rumble is left out of that onset, so it ducks under the mod's cues.
//! - While a one-off cue plays, the sustained level is held off the hand, and resumes at the first
//!   flush after it ends. An arrival weaker than the cue playing and of a lower priority is masked
//!   rather than cutting it short.

use std::{
    cmp::Ordering,
    mem,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use crate::controllers::{frame::Hand, haptics::cues::Sustain};

/// A vibration for one controller.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HapticPulse {
    pub duration: Duration,
    /// The vibration frequency, hertz, or `None` for the runtime's default.
    pub frequency_hz: Option<f32>,
    /// `0..=1`.
    pub amplitude: f32,
}

/// A one-off cue as the mixer sees it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Transient {
    pub amplitude: f32,
    pub duration: Duration,
    pub frequency_hz: Option<f32>,
    pub priority: u8,
}

/// A sustained source's level as the mixer sees it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Sustained {
    pub source: Sustain,
    pub amplitude: f32,
    pub frequency_hz: Option<f32>,
    /// How long the level holds without being sent again.
    pub hold: Duration,
}

/// How the mixer combines what arrives.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MixConfig {
    /// The sustained sum passes unchanged up to this amplitude, then bends toward
    /// [`MixConfig::sustain_ceiling`].
    pub sustain_knee: f32,
    /// The most the sustained sources together reach.
    pub sustain_ceiling: f32,
    /// A one-off cue's onset passes unchanged up to this amplitude, then bends toward full
    /// strength.
    pub knee: f32,
    /// The share of each other one-off cue's amplitude (arriving in the same frame, or still
    /// playing) added to the lead's.
    pub stack_share: f32,
    /// The share of the mod's sustained level added beneath a one-off cue's onset.
    pub duck_share: f32,
    /// How long each sustained vibration plays before the next flush refreshes it, seconds: long
    /// enough to bridge a frame hitch, short enough to stop soon after the sources do.
    pub refresh_s: f32,
}

impl MixConfig {
    pub const fn new() -> Self {
        Self {
            sustain_knee: 0.45,
            sustain_ceiling: 0.7,
            knee: 0.85,
            stack_share: 0.35,
            duck_share: 0.5,
            refresh_s: 0.1,
        }
    }
}

impl Default for MixConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// What the mixer did, since it was created or cleared.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct MixStats {
    /// One-off cues that played, as leads.
    pub played: u64,
    /// One-off cues folded into another's onset.
    pub stacked: u64,
    /// One-off cues masked by a stronger one playing.
    pub masked: u64,
    /// Flushes whose output the soft limiter bent.
    pub limited: u64,
}

/// The mixer for both hands.
#[derive(Debug, Default)]
pub struct Mixer {
    hands: [HandMix; 2],
    stats: MixStats,
}

impl Mixer {
    pub const fn new() -> Self {
        Self {
            hands: [HandMix::new(), HandMix::new()],
            stats: MixStats {
                played: 0,
                stacked: 0,
                masked: 0,
                limited: 0,
            },
        }
    }

    /// Queue a one-off cue on `hand` for the next flush.
    pub fn transient(&mut self, hand: Hand, transient: Transient) {
        if transient.amplitude > 0.0 && !transient.duration.is_zero() {
            self.hands[hand.index()].incoming.push(transient);
        }
    }

    /// Set a sustained source's level on `hand`, holding from `now`. A level of zero ends it.
    pub fn sustain(&mut self, hand: Hand, sustained: Sustained, now: Instant) {
        let held = &mut self.hands[hand.index()].held;
        held.retain(|h| h.source != sustained.source);
        if sustained.amplitude > MIN_AMPLITUDE && !sustained.hold.is_zero() {
            held.push(Held {
                source: sustained.source,
                amplitude: sustained.amplitude.min(1.0),
                frequency_hz: sustained.frequency_hz,
                until: now + sustained.hold,
            });
        }
    }

    /// What each hand plays from `now`, indexed by [`Hand::index`]: `None` leaves the hand as it
    /// is, playing out what it was last sent.
    pub fn mix(&mut self, now: Instant, config: &MixConfig) -> [Option<HapticPulse>; 2] {
        let mut out = [None; 2];
        for hand in Hand::BOTH {
            out[hand.index()] = self.hands[hand.index()].mix(now, config, &mut self.stats);
        }
        out
    }

    /// Forget everything queued, held, and playing.
    pub fn clear(&mut self) {
        for hand in &mut self.hands {
            hand.incoming.clear();
            hand.held.clear();
            hand.playing = None;
        }
    }

    pub fn stats(&self) -> MixStats {
        self.stats
    }
}

/// `x` passed unchanged up to `knee` and bent smoothly toward `ceiling` above it.
pub fn soft_limit(x: f32, knee: f32, ceiling: f32) -> f32 {
    if !x.is_finite() || x <= 0.0 {
        return 0.0;
    }
    let ceiling = ceiling.clamp(0.0, 1.0);
    let knee = knee.clamp(0.0, ceiling);
    if x <= knee {
        return x;
    }
    let headroom = ceiling - knee;
    if headroom <= 0.0 {
        return knee;
    }
    knee + headroom * ((x - knee) / headroom).tanh()
}

/// Below this, a level counts as still.
const MIN_AMPLITUDE: f32 = 0.01;

#[derive(Debug, Default)]
struct HandMix {
    incoming: Vec<Transient>,
    held: Vec<Held>,
    playing: Option<Playing>,
}

#[derive(Debug, Copy, Clone)]
struct Held {
    source: Sustain,
    amplitude: f32,
    frequency_hz: Option<f32>,
    until: Instant,
}

#[derive(Debug, Copy, Clone)]
struct Playing {
    amplitude: f32,
    priority: u8,
    until: Instant,
}

/// A hand's sustained sources, summed and limited.
#[derive(Debug, Copy, Clone, PartialEq)]
struct SustainedMix {
    /// Every source, limited.
    level: f32,
    /// The mod's sources only, limited: what a one-off cue's onset mixes in.
    under_cue: f32,
    /// The strongest source's frequency.
    frequency_hz: Option<f32>,
    /// Whether the limiter bent the level.
    limited: bool,
}

impl HandMix {
    const fn new() -> Self {
        Self {
            incoming: Vec::new(),
            held: Vec::new(),
            playing: None,
        }
    }

    fn mix(
        &mut self,
        now: Instant,
        config: &MixConfig,
        stats: &mut MixStats,
    ) -> Option<HapticPulse> {
        self.held.retain(|h| h.until > now);
        let playing = self.playing.filter(|p| p.until > now);
        self.playing = playing;
        let sustained = self.sustained(now, config);
        let incoming = mem::take(&mut self.incoming);
        if let Some(lead) = incoming.iter().copied().max_by(|a, b| {
            (a.priority, a.amplitude)
                .partial_cmp(&(b.priority, b.amplitude))
                .unwrap_or(Ordering::Equal)
        }) {
            if let Some(p) = playing
                && p.priority > lead.priority
                && p.amplitude >= lead.amplitude
            {
                stats.masked += incoming.len() as u64;
                return None;
            }
            let others = incoming.iter().map(|t| t.amplitude).sum::<f32>() - lead.amplitude
                + playing.map_or(0.0, |p| p.amplitude);
            let raw = lead.amplitude
                + config.stack_share.max(0.0) * others
                + config.duck_share.max(0.0) * sustained.under_cue;
            let amplitude = soft_limit(raw, config.knee, 1.0);
            let remaining =
                playing.map_or(Duration::ZERO, |p| p.until.saturating_duration_since(now));
            let duration = lead.duration.max(remaining);
            stats.played += 1;
            stats.stacked += incoming.len() as u64 - 1 + u64::from(playing.is_some());
            stats.limited += u64::from(raw > config.knee.clamp(0.0, 1.0));
            self.playing = Some(Playing {
                amplitude,
                priority: lead.priority.max(playing.map_or(0, |p| p.priority)),
                until: now + duration,
            });
            return Some(HapticPulse {
                duration,
                frequency_hz: lead.frequency_hz,
                amplitude,
            });
        }
        if playing.is_some() || sustained.level <= MIN_AMPLITUDE {
            return None;
        }
        stats.limited += u64::from(sustained.limited);
        let refresh =
            Duration::try_from_secs_f32(config.refresh_s.max(0.0)).unwrap_or(Duration::ZERO);
        (!refresh.is_zero()).then_some(HapticPulse {
            duration: refresh,
            frequency_hz: sustained.frequency_hz,
            amplitude: sustained.level,
        })
    }

    fn sustained(&self, now: Instant, config: &MixConfig) -> SustainedMix {
        let live = self.held.iter().filter(|h| h.until > now);
        let total: f32 = live.clone().map(|h| h.amplitude).sum();
        let mods: f32 = live
            .clone()
            .filter(|h| !h.source.ducks_fully())
            .map(|h| h.amplitude)
            .sum();
        let strongest = live.max_by(|a, b| {
            a.amplitude
                .partial_cmp(&b.amplitude)
                .unwrap_or(Ordering::Equal)
        });
        let limit = |x: f32| soft_limit(x, config.sustain_knee, config.sustain_ceiling);
        SustainedMix {
            level: limit(total),
            under_cue: limit(mods),
            frequency_hz: strongest.and_then(|h| h.frequency_hz),
            limited: total > config.sustain_knee,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: MixConfig = MixConfig::new();

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn transient(amplitude: f32, duration_ms: u64, priority: u8) -> Transient {
        Transient {
            amplitude,
            duration: ms(duration_ms),
            frequency_hz: Some(100.0 + f32::from(priority)),
            priority,
        }
    }

    fn sustained(source: Sustain, amplitude: f32) -> Sustained {
        Sustained {
            source,
            amplitude,
            frequency_hz: Some(70.0),
            hold: ms(100),
        }
    }

    #[test]
    fn a_lone_cue_plays_as_sent() {
        let mut mixer = Mixer::new();
        let t0 = Instant::now();
        mixer.transient(Hand::Left, transient(0.4, 30, 2));
        let out = mixer.mix(t0, &CONFIG);
        assert_eq!(
            out[0],
            Some(HapticPulse {
                duration: ms(30),
                frequency_hz: Some(102.0),
                amplitude: 0.4
            })
        );
        assert_eq!(out[1], None);
        assert_eq!(mixer.stats().played, 1);
    }

    #[test]
    fn nothing_queued_plays_nothing() {
        let mut mixer = Mixer::new();
        assert_eq!(mixer.mix(Instant::now(), &CONFIG), [None, None]);
    }

    #[test]
    fn the_higher_priority_cue_leads_and_the_rest_stack_beneath() {
        let mut mixer = Mixer::new();
        let t0 = Instant::now();
        mixer.transient(Hand::Right, transient(0.3, 20, 1));
        mixer.transient(Hand::Right, transient(0.5, 70, 4));
        let out = mixer.mix(t0, &CONFIG)[1].unwrap();
        assert_eq!(out.duration, ms(70));
        assert_eq!(out.frequency_hz, Some(104.0));
        let expected = 0.5 + CONFIG.stack_share * 0.3;
        assert!((out.amplitude - expected).abs() < 1e-6);
        assert_eq!(mixer.stats().stacked, 1);
    }

    #[test]
    fn stacked_cues_never_saturate() {
        let mut mixer = Mixer::new();
        for _ in 0..20 {
            mixer.transient(Hand::Left, transient(0.9, 40, 2));
        }
        let out = mixer.mix(Instant::now(), &CONFIG)[0].unwrap();
        assert!(out.amplitude <= 1.0);
        assert!(out.amplitude > CONFIG.knee);
        assert_eq!(mixer.stats().limited, 1);
    }

    #[test]
    fn a_sustained_level_refreshes_each_flush_until_its_hold_runs_out() {
        let mut mixer = Mixer::new();
        let t0 = Instant::now();
        mixer.sustain(Hand::Left, sustained(Sustain::ClimbStrain, 0.3), t0);
        let out = mixer.mix(t0, &CONFIG)[0].unwrap();
        assert_eq!(out.amplitude, 0.3);
        assert!(out.duration.abs_diff(ms(100)) < Duration::from_micros(1));
        assert!(mixer.mix(t0 + ms(50), &CONFIG)[0].is_some());
        assert_eq!(mixer.mix(t0 + ms(150), &CONFIG)[0], None);
    }

    #[test]
    fn a_zero_level_ends_a_sustained_source_at_once() {
        let mut mixer = Mixer::new();
        let t0 = Instant::now();
        mixer.sustain(Hand::Left, sustained(Sustain::GameRumble, 0.5), t0);
        mixer.sustain(Hand::Left, sustained(Sustain::GameRumble, 0.0), t0);
        assert_eq!(mixer.mix(t0, &CONFIG)[0], None);
    }

    #[test]
    fn sustained_sources_sum_under_their_ceiling() {
        let mut mixer = Mixer::new();
        let t0 = Instant::now();
        for source in Sustain::ALL {
            mixer.sustain(Hand::Right, sustained(source, 0.5), t0);
        }
        let out = mixer.mix(t0, &CONFIG)[1].unwrap();
        assert!(out.amplitude <= CONFIG.sustain_ceiling);
        assert!(out.amplitude > CONFIG.sustain_knee);
        let mut two = Mixer::new();
        two.sustain(Hand::Right, sustained(Sustain::ClimbStrain, 0.2), t0);
        two.sustain(Hand::Right, sustained(Sustain::GrappleReel, 0.2), t0);
        assert!((two.mix(t0, &CONFIG)[1].unwrap().amplitude - 0.4).abs() < 1e-6);
    }

    #[test]
    fn a_cue_holds_the_sustained_level_off_until_it_ends() {
        let mut mixer = Mixer::new();
        let t0 = Instant::now();
        mixer.sustain(Hand::Left, sustained(Sustain::ClimbStrain, 0.3), t0);
        mixer.transient(Hand::Left, transient(0.5, 40, 2));
        let onset = mixer.mix(t0, &CONFIG)[0].unwrap();
        let expected = 0.5 + CONFIG.duck_share * 0.3;
        assert!((onset.amplitude - expected).abs() < 1e-6);
        mixer.sustain(
            Hand::Left,
            sustained(Sustain::ClimbStrain, 0.3),
            t0 + ms(20),
        );
        assert_eq!(mixer.mix(t0 + ms(20), &CONFIG)[0], None);
        mixer.sustain(
            Hand::Left,
            sustained(Sustain::ClimbStrain, 0.3),
            t0 + ms(45),
        );
        let resumed = mixer.mix(t0 + ms(45), &CONFIG)[0].unwrap();
        assert_eq!(resumed.amplitude, 0.3);
    }

    #[test]
    fn the_game_rumble_ducks_under_a_cue() {
        let mut mixer = Mixer::new();
        let t0 = Instant::now();
        mixer.sustain(Hand::Right, sustained(Sustain::GameRumble, 0.4), t0);
        mixer.transient(Hand::Right, transient(0.5, 70, 4));
        assert_eq!(mixer.mix(t0, &CONFIG)[1].unwrap().amplitude, 0.5);
    }

    #[test]
    fn a_weaker_lower_cue_is_masked_by_one_playing() {
        let mut mixer = Mixer::new();
        let t0 = Instant::now();
        mixer.transient(Hand::Left, transient(0.8, 70, 4));
        mixer.mix(t0, &CONFIG);
        mixer.transient(Hand::Left, transient(0.2, 8, 0));
        assert_eq!(mixer.mix(t0 + ms(10), &CONFIG)[0], None);
        assert_eq!(mixer.stats().masked, 1);
    }

    #[test]
    fn a_stronger_cue_cuts_in_and_keeps_the_longer_end() {
        let mut mixer = Mixer::new();
        let t0 = Instant::now();
        mixer.transient(Hand::Left, transient(0.3, 100, 3));
        mixer.mix(t0, &CONFIG);
        mixer.transient(Hand::Left, transient(0.6, 20, 2));
        let out = mixer.mix(t0 + ms(10), &CONFIG)[0].unwrap();
        assert_eq!(out.duration, ms(90));
        let expected = soft_limit(0.6 + CONFIG.stack_share * 0.3, CONFIG.knee, 1.0);
        assert!((out.amplitude - expected).abs() < 1e-6);
        assert_eq!(mixer.stats().stacked, 1);
    }

    #[test]
    fn the_hands_mix_apart() {
        let mut mixer = Mixer::new();
        let t0 = Instant::now();
        mixer.transient(Hand::Left, transient(0.5, 40, 2));
        mixer.sustain(Hand::Right, sustained(Sustain::GrappleReel, 0.2), t0);
        let out = mixer.mix(t0, &CONFIG);
        assert_eq!(out[0].unwrap().amplitude, 0.5);
        assert_eq!(out[1].unwrap().amplitude, 0.2);
    }

    #[test]
    fn clearing_forgets_everything() {
        let mut mixer = Mixer::new();
        let t0 = Instant::now();
        mixer.transient(Hand::Left, transient(0.5, 40, 2));
        mixer.sustain(Hand::Left, sustained(Sustain::GrappleReel, 0.2), t0);
        mixer.clear();
        assert_eq!(mixer.mix(t0, &CONFIG), [None, None]);
    }

    #[test]
    fn soft_limit_passes_below_the_knee_and_never_passes_the_ceiling() {
        assert_eq!(soft_limit(0.3, 0.5, 0.8), 0.3);
        assert_eq!(soft_limit(-1.0, 0.5, 0.8), 0.0);
        assert_eq!(soft_limit(f32::NAN, 0.5, 0.8), 0.0);
        assert!(soft_limit(100.0, 0.5, 0.8) <= 0.8);
        assert_eq!(soft_limit(2.0, 0.8, 0.8), 0.8);
    }

    #[test]
    fn soft_limit_is_monotone_and_bounded() {
        let grid = (0..=40).map(|i| i as f32 * 0.1);
        for (knee, ceiling) in [
            (0.0, 1.0),
            (0.45, 0.7),
            (0.85, 1.0),
            (0.7, 0.45),
            (1.0, 1.0),
        ] {
            let mut previous = 0.0;
            for x in grid.clone() {
                let y = soft_limit(x, knee, ceiling);
                assert!(y + 1e-6 >= previous, "knee {knee} ceiling {ceiling} at {x}");
                assert!(y <= ceiling + 1e-6, "knee {knee} ceiling {ceiling} at {x}");
                previous = y;
            }
        }
    }

    #[test]
    fn every_output_is_a_playable_amplitude() {
        let t0 = Instant::now();
        for count in 0..12u8 {
            let mut mixer = Mixer::new();
            for (i, source) in Sustain::ALL.into_iter().enumerate() {
                mixer.sustain(Hand::Left, sustained(source, (i as f32 * 0.37) % 1.0), t0);
            }
            for i in 0..count {
                let amplitude = (f32::from(i) * 0.29 + 0.1) % 1.0;
                mixer.transient(
                    Hand::Left,
                    transient(amplitude, 1 + u64::from(i) * 25, i % 5),
                );
            }
            if let Some(out) = mixer.mix(t0, &CONFIG)[0] {
                assert!((0.0..=1.0).contains(&out.amplitude));
                assert!(!out.duration.is_zero());
            }
        }
    }
}
