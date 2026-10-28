//! Failure injection at the production resource transaction boundary; no GPU is used.

use super::*;

struct Fake {
    fail_create: bool,
    fail_bind: bool,
    fail_at: Option<usize>,
    matches: bool,
    creates: usize,
    restores: usize,
    bound: Option<AppliedRedirect>,
}
impl Adapter for Fake {
    type Target = (u32, u32);
    fn size(target: &Self::Target) -> (u32, u32) {
        *target
    }
    fn create(&mut self, size: (u32, u32)) -> anyhow::Result<Self::Target> {
        self.creates += 1;
        anyhow::ensure!(
            !self.fail_create && self.fail_at != Some(self.creates),
            "injected allocation"
        );
        Ok(size)
    }
    fn matches(&mut self, _: (u32, u32)) -> bool {
        self.matches
    }
    fn bind(&mut self, _: &Self::Target, request: AppliedRedirect) -> bool {
        self.bound = Some(request);
        !self.fail_bind
    }
    fn restore(&mut self, _: (u32, u32)) {
        self.restores += 1;
        self.bound = None;
    }
}

#[test]
fn allocation_failure_preserves_the_applied_surface_and_bounds_retry() {
    let old = (100, 80);
    let mut target = Some(old);
    let mut redirected = true;
    let mut render = Some((800, 600));
    let mut stage = false;
    let mut retry = Retry::new();
    let mut fake = Fake {
        fail_create: true,
        fail_bind: false,
        fail_at: None,
        matches: true,
        creates: 0,
        restores: 0,
        bound: Some(AppliedRedirect {
            texture: old,
            stage_safe_area: false,
        }),
    };
    let request = AppliedRedirect {
        texture: (200, 160),
        stage_safe_area: true,
    };
    let now = Instant::now();
    for step in 0..100 {
        assert!(!redirect(
            Surface {
                target: &mut target,
                redirected: &mut redirected,
                render_size: &mut render,
                stage_safe_area: &mut stage,
                retry: &mut retry
            },
            &mut fake,
            request,
            (800, 600),
            now + Duration::from_millis(step)
        ));
        assert_eq!(target, Some(old));
        assert!(redirected);
        assert!(!stage);
        assert_eq!(fake.bound.unwrap().texture, old);
    }
    assert_eq!(fake.creates, 1);
    fake.fail_create = false;
    assert!(redirect(
        Surface {
            target: &mut target,
            redirected: &mut redirected,
            render_size: &mut render,
            stage_safe_area: &mut stage,
            retry: &mut retry
        },
        &mut fake,
        request,
        (800, 600),
        now + Duration::from_secs(1)
    ));
    assert_eq!(target, Some(request.texture));
    assert!(redirected && stage);
    assert_eq!(fake.bound, Some(request));
}

#[test]
fn partial_binding_failure_restores_native_state_and_keeps_old_ownership() {
    let mut target = Some((100, 80));
    let mut redirected = true;
    let mut render = Some((800, 600));
    let mut stage = false;
    let mut retry = Retry::new();
    let mut fake = Fake {
        fail_create: false,
        fail_bind: true,
        fail_at: None,
        matches: true,
        creates: 0,
        restores: 0,
        bound: None,
    };
    assert!(!redirect(
        Surface {
            target: &mut target,
            redirected: &mut redirected,
            render_size: &mut render,
            stage_safe_area: &mut stage,
            retry: &mut retry
        },
        &mut fake,
        AppliedRedirect {
            texture: (200, 160),
            stage_safe_area: true
        },
        (800, 600),
        Instant::now()
    ));
    assert_eq!(target, Some((100, 80)));
    assert!(!redirected);
    assert_eq!(fake.bound, None);
    assert_eq!(fake.restores, 1);
}

#[test]
fn split_failure_preserves_both_layers_until_complete_retry() {
    let old = [Some((100, 80)), Some((100, 80))];
    let mut targets = old;
    let mut retry = Retry::new();
    let mut fake = Fake {
        fail_create: false,
        fail_bind: false,
        fail_at: Some(2),
        matches: true,
        creates: 0,
        restores: 0,
        bound: None,
    };
    let now = Instant::now();
    assert_eq!(
        layers(&mut targets, &mut fake, (200, 160), &mut retry, now),
        [false; 2]
    );
    assert_eq!(targets, old);
    assert_eq!(
        layers(&mut targets, &mut fake, (200, 160), &mut retry, now),
        [false; 2]
    );
    assert_eq!(fake.creates, 2);
    fake.fail_at = None;
    assert_eq!(
        layers(
            &mut targets,
            &mut fake,
            (200, 160),
            &mut retry,
            now + Duration::from_secs(1)
        ),
        [true; 2]
    );
    assert_eq!(targets, [Some((200, 160)); 2]);
}

#[test]
fn preview_retirement_keeps_ids_until_the_owner_drains_them_once() {
    let mut state = crate::hud::state::HudState::new();
    state.preview_id = Some(egui::TextureId::User(1));
    state.layer_preview_ids = [
        Some(egui::TextureId::User(2)),
        Some(egui::TextureId::User(3)),
    ];
    retire(&mut state.preview_id, &mut state.pending_unregisters);
    for id in &mut state.layer_preview_ids {
        retire(id, &mut state.pending_unregisters);
    }
    retire(&mut state.preview_id, &mut state.pending_unregisters);
    let mut ids = Vec::new();
    drain(&mut state.pending_unregisters, |id| ids.push(id));
    drain(&mut state.pending_unregisters, |id| ids.push(id));
    assert_eq!(
        ids,
        [
            egui::TextureId::User(1),
            egui::TextureId::User(2),
            egui::TextureId::User(3)
        ]
    );
    assert!(state.pending_unregisters.is_empty());
}
