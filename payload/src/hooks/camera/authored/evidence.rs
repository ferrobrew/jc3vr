//! Owned animation evidence copied at synchronous completion and checked at the camera seam.

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

const MAX_LATEST_PUBLICATIONS: usize = 64;

#[derive(Default)]
struct Publication {
    serial: u64,
    latest: Vec<Finalized>,
}

impl Publication {
    fn record(&mut self, source: Source, pose_buffer: u64, animation: Animation) {
        self.serial = self.serial.wrapping_add(1);
        let serial = self.serial;
        self.latest
            .retain(|entry| entry.source.controller != source.controller);
        if self.latest.len() == MAX_LATEST_PUBLICATIONS {
            self.latest.remove(0);
        }
        self.latest.push(Finalized {
            source,
            pose_buffer,
            serial,
            animation,
        });
    }

    fn invalidate(&mut self, controller: u64) {
        self.latest
            .retain(|entry| entry.source.controller != controller);
    }
}

/// Called only at the existing serialized controller seam; no native pointer is stored.
pub(super) fn publish(source: Source, pose_buffer: u64, animation: Animation) {
    PUBLICATION.lock().record(source, pose_buffer, animation);
}

pub(super) fn invalidate(controller: u64) {
    PUBLICATION.lock().invalidate(controller);
}

pub(super) fn matching(
    source: Source,
    pose_buffer: u64,
    animation: &Animation,
) -> Result<Finalized, Refusal> {
    let publication = PUBLICATION.lock();
    let latest = publication
        .latest
        .iter()
        .find(|entry| entry.source.controller == source.controller)
        .ok_or(Refusal::MissingFinalizedAnimation)?;
    if latest.source != source
        || latest.pose_buffer != pose_buffer
        || &latest.animation != animation
    {
        return Err(Refusal::IncoherentAnimation);
    }
    Ok(latest.clone())
}

static PUBLICATION: Mutex<Publication> = Mutex::new(Publication {
    serial: 0,
    latest: Vec::new(),
});

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
    fn publication_is_bounded_replaces_controllers_and_invalidates_locally() {
        let source = Source {
            controller: 0,
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

        for controller in 1..=65 {
            publication.record(
                Source {
                    controller,
                    ..source
                },
                controller,
                animation.clone(),
            );
        }

        assert_eq!(publication.latest.len(), MAX_LATEST_PUBLICATIONS);
        assert_eq!(publication.latest[0].source.controller, 2);
        assert!(
            !publication
                .latest
                .iter()
                .any(|entry| entry.source.controller == 1)
        );

        let previous_serial = publication.latest[63].serial;
        publication.record(
            Source {
                controller: 65,
                ..source
            },
            650,
            animation.clone(),
        );
        assert_eq!(publication.latest.len(), MAX_LATEST_PUBLICATIONS);
        assert_eq!(publication.latest[63].source.controller, 65);
        assert_eq!(publication.latest[63].pose_buffer, 650);
        assert!(publication.latest[63].serial > previous_serial);

        publication.record(
            Source {
                controller: 66,
                ..source
            },
            66,
            animation,
        );
        assert_eq!(publication.latest.len(), MAX_LATEST_PUBLICATIONS);
        assert_eq!(publication.latest[0].source.controller, 3);

        publication.invalidate(40);
        assert!(
            !publication
                .latest
                .iter()
                .any(|entry| entry.source.controller == 40)
        );
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
