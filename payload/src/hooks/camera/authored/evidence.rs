//! Owned animation evidence copied at synchronous completion and checked at the camera seam.

#[cfg(test)]
use std::cell::RefCell;

#[cfg(not(test))]
use parking_lot::Mutex;

use crate::hooks::camera::authored::{Refusal, native::Source};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ClipSource {
    pub instance: u64,
    pub owner: u64,
    pub animation: u64,
    pub binding: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Clip {
    pub source: ClipSource,
    pub time: f32,
    pub duration: f32,
    pub looping: bool,
    pub overflows: u32,
    pub underflows: u32,
    pub weight: f32,
    pub camera_track_weight: f32,
}

impl Clip {
    pub fn validate(self) -> Result<(), Refusal> {
        if self.source.instance == 0
            || self.source.owner == 0
            || self.source.animation == 0
            || self.source.binding == 0
        {
            return Err(Refusal::MissingClip);
        }
        if !self.time.is_finite()
            || self.time < 0.0
            || !self.duration.is_finite()
            || self.duration <= 0.0
            || self.time > self.duration
        {
            return Err(Refusal::InvalidClipTime);
        }
        if !self.weight.is_finite()
            || !(0.0..=1.0).contains(&self.weight)
            || !self.camera_track_weight.is_finite()
            || !(0.0..=1.0).contains(&self.camera_track_weight)
        {
            return Err(Refusal::InvalidNativeWeight);
        }
        Ok(())
    }

    /// A backwards time is a wrap only with native forward-cycle evidence on the same binding.
    pub fn restarted(self, previous: Self) -> bool {
        if self.source != previous.source
            || self.duration != previous.duration
            || self.looping != previous.looping
        {
            return true;
        }
        let wrapped = self.looping
            && self.overflows > previous.overflows
            && self.underflows == previous.underflows;
        self.overflows < previous.overflows
            || self.underflows != previous.underflows
            || (self.time < previous.time && !wrapped)
    }

    pub fn contributes(self) -> bool {
        self.weight > 0.0 && self.camera_track_weight > 0.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Animation {
    pub state: Option<u32>,
    pub playing: Vec<Clip>,
    pub samplers: Vec<Clip>,
}

impl Animation {
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.playing.is_empty() || self.samplers.is_empty() {
            return Err(Refusal::MissingClip);
        }
        for clip in &self.samplers {
            clip.validate()?;
        }
        for clip in &self.playing {
            clip.validate()?;
            if !self.samplers.contains(clip) {
                return Err(Refusal::IncoherentAnimation);
            }
        }
        for (index, clip) in self.samplers.iter().enumerate() {
            if self.samplers[..index]
                .iter()
                .any(|other| other.source == clip.source)
            {
                return Err(Refusal::IncoherentAnimation);
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(super) struct Finalized {
    pub source: Source,
    pub pose_buffer: u64,
    pub serial: u64,
    pub animation: Animation,
}

#[derive(Default)]
struct Publication {
    serial: u64,
    selected: Option<u64>,
    latest: Option<Finalized>,
}

impl Publication {
    fn select(&mut self, controller: Option<u64>) {
        if self.selected != controller {
            self.latest = None;
            self.selected = controller;
        }
    }

    fn record(&mut self, source: Source, pose_buffer: u64, animation: Animation) {
        if self.selected != Some(source.controller) {
            return;
        }
        self.serial = self.serial.wrapping_add(1);
        self.latest = Some(Finalized {
            source,
            pose_buffer,
            serial: self.serial,
            animation,
        });
    }

    fn invalidate(&mut self, controller: u64) {
        if self.selected == Some(controller) {
            self.latest = None;
        }
    }
}

pub(crate) fn select_controller(controller: Option<u64>) {
    with_publication(|publication| publication.select(controller));
}

/// Called only at the existing serialized controller seam; no native pointer is stored.
pub(super) fn publish(source: Source, pose_buffer: u64, animation: Animation) {
    with_publication(|publication| publication.record(source, pose_buffer, animation));
}

pub(super) fn invalidate(controller: u64) {
    with_publication(|publication| publication.invalidate(controller));
}

pub(super) fn matching(
    source: Source,
    pose_buffer: u64,
    animation: &Animation,
) -> Result<Finalized, Refusal> {
    with_publication(|publication| {
        let latest = publication
            .latest
            .as_ref()
            .filter(|entry| entry.source.controller == source.controller)
            .ok_or(Refusal::MissingFinalizedAnimation)?;
        if latest.source != source
            || latest.pose_buffer != pose_buffer
            || &latest.animation != animation
        {
            return Err(Refusal::IncoherentAnimation);
        }
        Ok(latest.clone())
    })
}

/// Run `f` on the publication. The game has one, shared by its controller seam; each unit test has
/// its own, so that tests running side by side do not select controllers out from under each
/// other.
#[cfg(not(test))]
fn with_publication<R>(f: impl FnOnce(&mut Publication) -> R) -> R {
    f(&mut PUBLICATION.lock())
}

#[cfg(test)]
fn with_publication<R>(f: impl FnOnce(&mut Publication) -> R) -> R {
    PUBLICATION.with_borrow_mut(f)
}

#[cfg(not(test))]
static PUBLICATION: Mutex<Publication> = Mutex::new(Publication {
    serial: 0,
    selected: None,
    latest: None,
});

#[cfg(test)]
thread_local! {
    static PUBLICATION: RefCell<Publication> = RefCell::new(Publication::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn clip(time: f32) -> Clip {
        Clip {
            source: ClipSource {
                instance: 1,
                owner: 2,
                animation: 3,
                binding: 4,
            },
            time,
            duration: 2.0,
            looping: false,
            overflows: 0,
            underflows: 0,
            weight: 1.0,
            camera_track_weight: 1.0,
        }
    }

    #[test]
    fn native_cycles_distinguish_forward_wrap_from_reset_and_reverse_playback() {
        let previous = Clip {
            looping: true,
            ..clip(1.9)
        };
        let wrapped = Clip {
            time: 0.1,
            overflows: 1,
            ..previous
        };
        assert!(!wrapped.restarted(previous));
        assert!(
            Clip {
                overflows: 0,
                ..wrapped
            }
            .restarted(previous)
        );
        assert!(
            Clip {
                underflows: 1,
                ..wrapped
            }
            .restarted(previous)
        );
        assert!(
            Clip {
                looping: false,
                ..wrapped
            }
            .restarted(previous)
        );
    }

    #[test]
    fn publication_rejects_post_evaluation_time_weight_and_source_changes() {
        let source = Source {
            controller: 900,
            producer: 901,
            producer_owner: 902,
            skeleton_resource: 903,
            skeleton: 904,
            driver: 905,
            rule: 906,
            blender: 907,
            camera_bone: 2,
        };
        let animation = Animation {
            state: Some(1),
            playing: vec![clip(0.1)],
            samplers: vec![clip(0.1)],
        };
        select_controller(Some(source.controller));
        publish(source, 100, animation.clone());
        let first = matching(source, 100, &animation).unwrap();
        let mut changed = animation.clone();
        changed.samplers[0].time = 0.2;
        assert!(matching(source, 100, &changed).is_err());
        changed = animation.clone();
        changed.samplers[0].weight = 0.5;
        assert!(matching(source, 100, &changed).is_err());
        assert!(
            matching(
                Source {
                    producer_owner: 999,
                    ..source
                },
                100,
                &animation
            )
            .is_err()
        );
        publish(source, 100, animation.clone());
        assert!(matching(source, 100, &animation).unwrap().serial > first.serial);
        invalidate(source.controller);
        assert!(matches!(
            matching(source, 100, &animation),
            Err(Refusal::MissingFinalizedAnimation)
        ));
    }

    #[test]
    fn only_selected_local_publication_survives_npc_updates_and_selection_replacement() {
        let source = Source {
            controller: 1,
            producer: 901,
            producer_owner: 902,
            skeleton_resource: 903,
            skeleton: 904,
            driver: 905,
            rule: 906,
            blender: 907,
            camera_bone: 2,
        };
        let animation = Animation {
            state: Some(1),
            playing: vec![clip(0.1)],
            samplers: vec![clip(0.1)],
        };
        let mut publication = Publication::default();
        publication.select(Some(source.controller));
        publication.record(source, 100, animation.clone());
        let serial = publication.latest.as_ref().unwrap().serial;
        for controller in 2..=100 {
            publication.select(Some(source.controller));
            publication.record(
                Source {
                    controller,
                    ..source
                },
                controller,
                animation.clone(),
            );
            publication.invalidate(controller);
        }
        let local = publication.latest.as_ref().unwrap();
        assert_eq!(local.source, source);
        assert_eq!(local.pose_buffer, 100);
        assert_eq!(local.serial, serial);
        publication.select(Some(2));
        assert!(publication.latest.is_none());
        publication.record(source, 100, animation.clone());
        assert!(publication.latest.is_none());
        let replacement = Source {
            controller: 2,
            ..source
        };
        publication.record(replacement, 200, animation);
        assert_eq!(publication.latest.as_ref().unwrap().source, replacement);
        assert!(publication.latest.as_ref().unwrap().serial > serial);
        publication.select(None);
        assert!(publication.latest.is_none());
    }

    #[test]
    fn missing_invalid_or_unregistered_native_inputs_are_not_evidence() {
        for invalid in [
            Clip {
                time: f32::NAN,
                ..clip(0.0)
            },
            Clip {
                time: -0.1,
                ..clip(0.0)
            },
            Clip {
                duration: f32::INFINITY,
                ..clip(0.0)
            },
            Clip {
                weight: f32::NAN,
                ..clip(0.0)
            },
            Clip {
                weight: 1.1,
                ..clip(0.0)
            },
            Clip {
                camera_track_weight: -0.1,
                ..clip(0.0)
            },
        ] {
            assert!(invalid.validate().is_err());
        }
        assert!(
            Animation {
                state: Some(1),
                playing: vec![clip(0.1)],
                samplers: vec![clip(0.0)],
            }
            .validate()
            .is_err()
        );
    }
}
