//! Receipts for the selected controller's synchronous pose evaluation only.
//!
//! The controller's native initialized guard calls PoseProducer::Update inline. Its blender pass
//! invalidates model-space data and copies evaluated local transforms before the call returns.
//! Time-only advancement and asynchronous producer jobs do not publish receipts here.

use jc3gi::{
    animation::pose_producer::Blender, character::character::AnimationController,
    types::shared_ptr::SharedPtr,
};
use parking_lot::Mutex;

use crate::{controllers::body::procedural::skeleton::SkeletonKey, hooks::character::pose};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Receipt {
    pub serial: u64,
    pub skeleton: SkeletonKey,
    pub producer: usize,
    pub bones: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Evaluation {
    skeleton: SkeletonKey,
    producer: usize,
    bones: u32,
}

/// Snapshot the exact source admitted by the synchronous controller call, without reading joints.
pub(super) unsafe fn begin(controller: &AnimationController, bones: i32) -> Option<Evaluation> {
    if !controller.m_Initialized {
        return None;
    }
    let (skeleton, producer) = unsafe { pose::identity(controller) }?;
    let layers = &unsafe { controller.m_PoseProducer.as_ref() }?.m_Blenders;
    if layers.size <= 0
        || layers.data.is_null()
        || !(layers.data as usize).is_multiple_of(std::mem::align_of::<SharedPtr<Blender>>())
    {
        return None;
    }
    let layers = unsafe { std::slice::from_raw_parts(layers.data, layers.size as usize) };
    if layers.iter().any(|layer| layer.px.is_null()) {
        return None;
    }
    let bones = u32::try_from(bones)
        .ok()
        .filter(|&count| count > 0 && count as usize <= skeleton.bones())?;
    Some(Evaluation {
        skeleton,
        producer,
        bones,
    })
}

/// Run after UpdateAnimations returns, before camera-specific evidence can reject its channel.
pub(super) unsafe fn complete(controller: &AnimationController, before: Option<Evaluation>) {
    let mut store = STORE.lock();
    let after = before.and_then(|before| {
        unsafe { begin(controller, before.bones as i32) }.filter(|after| *after == before)
    });
    store.complete(after);
}

pub(super) fn select(controller: Option<usize>) {
    STORE.lock().select(controller);
}

pub(super) fn invalidate_producer(producer: usize) {
    STORE.lock().invalidate_producer(producer);
}

pub(crate) fn current(skeleton: SkeletonKey, producer: usize, bones: [u32; 4]) -> Option<Receipt> {
    STORE.lock().latest.filter(|receipt| {
        receipt.skeleton == skeleton
            && receipt.producer == producer
            && bones.into_iter().all(|index| index < receipt.bones)
    })
}

struct Store {
    selected: Option<usize>,
    serial: u64,
    latest: Option<Receipt>,
}

impl Store {
    fn select(&mut self, controller: Option<usize>) {
        if self.selected != controller {
            self.selected = controller;
            self.latest = None;
        }
    }

    fn invalidate_producer(&mut self, producer: usize) {
        if self
            .latest
            .is_some_and(|receipt| receipt.producer == producer)
        {
            self.latest = None;
        }
    }

    fn complete(&mut self, evaluation: Option<Evaluation>) {
        self.latest = evaluation.and_then(|evaluation| {
            self.serial = self.serial.checked_add(1)?;
            Some(Receipt {
                serial: self.serial,
                skeleton: evaluation.skeleton,
                producer: evaluation.producer,
                bones: evaluation.bones,
            })
        });
    }
}

static STORE: Mutex<Store> = Mutex::new(Store {
    selected: None,
    serial: 0,
    latest: None,
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::headpose::anchors::tests::identity_fixture;

    fn fixture() -> (Store, Evaluation) {
        let identity = identity_fixture();
        (
            Store {
                selected: Some(1),
                serial: 0,
                latest: None,
            },
            Evaluation {
                skeleton: identity.skeleton,
                producer: identity.producer,
                bones: 5,
            },
        )
    }

    #[test]
    fn only_completed_evaluations_advance_serial_and_time_only_leaves_it_unchanged() {
        let (mut store, evaluation) = fixture();
        store.complete(Some(evaluation));
        let first = store.latest.unwrap();
        store.select(Some(1));
        assert_eq!(store.latest, Some(first));
        assert_eq!(store.serial, 1);
        store.complete(Some(evaluation));
        assert_eq!(store.latest.unwrap().serial, 2);
        store.complete(None);
        assert!(store.latest.is_none());
        assert_eq!(store.serial, 2);
    }

    #[test]
    fn async_dispatch_and_source_replacement_withdraw_without_publication() {
        let (mut store, evaluation) = fixture();
        store.complete(Some(evaluation));
        store.invalidate_producer(evaluation.producer + 1);
        assert!(store.latest.is_some());
        store.invalidate_producer(evaluation.producer);
        assert!(store.latest.is_none());
        assert_eq!(store.serial, 1);
        store.complete(Some(evaluation));
        store.select(Some(2));
        assert!(store.latest.is_none());
        assert_eq!(store.serial, 2);
    }

    #[test]
    fn exhausted_serial_never_reuses_a_receipt() {
        let (mut store, evaluation) = fixture();
        store.serial = u64::MAX;
        store.complete(Some(evaluation));
        assert!(store.latest.is_none());
        assert_eq!(store.serial, u64::MAX);
    }
}
