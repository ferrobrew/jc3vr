//! The D3D rasterizer override behind a masked per-eye run's scissor rectangles. [`apply`] binds a
//! scissor-enabled twin of the bound rasterizer state, cached per base state and device; [`reapply`]
//! re-binds it after the engine flushes its own staged rasterizer state, and when no twin can be made
//! it empties the viewports instead so the run draws nothing; [`restore`] puts back what it replaced.

use std::cell::RefCell;

use windows::{
    Win32::Graphics::Direct3D11::{
        D3D11_CULL_BACK, D3D11_FILL_SOLID, D3D11_RASTERIZER_DESC, D3D11_VIEWPORT,
        D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE, ID3D11Device,
        ID3D11DeviceContext, ID3D11RasterizerState,
    },
    core::{Error, Interface, Result},
};

pub(super) fn apply(context: &ID3D11DeviceContext) -> bool {
    OVERRIDE.with_borrow_mut(|state| state.apply(&D3d(context)))
}

pub(super) fn reapply(context: &ID3D11DeviceContext) -> bool {
    OVERRIDE.with_borrow_mut(|state| state.reapply(&D3d(context)))
}

pub(super) fn restore(context: &ID3D11DeviceContext) {
    OVERRIDE.with_borrow_mut(|state| state.restore(&D3d(context)));
}

const CACHE_SIZE: usize = 32;

thread_local! {
    static OVERRIDE: RefCell<ScissorOverride<D3d<'static>>> = RefCell::new(ScissorOverride::new());
}

trait Rasterizer {
    type State: Clone;
    type Device;
    type StateKey: Copy + PartialEq;
    type DeviceKey: Copy + PartialEq;

    fn device(&self) -> Result<Self::Device>;
    fn device_key(device: &Self::Device) -> Self::DeviceKey;
    fn state_key(state: &Self::State) -> Self::StateKey;
    fn bound(&self) -> Option<Self::State>;
    fn description(&self, state: &Self::State) -> D3D11_RASTERIZER_DESC;
    fn create(&self, desc: &D3D11_RASTERIZER_DESC) -> Result<Self::State>;
    fn bind(&self, state: Option<&Self::State>);
    fn viewports(&self) -> Viewports;
    fn set_viewports(&self, viewports: &Viewports);
}

struct ScissorOverride<R: Rasterizer> {
    // The inner `None` is the D3D default state; the outer `None` means no override is bound.
    underlying: Option<Option<R::State>>,
    device: Option<(R::DeviceKey, R::Device)>,
    cache: [Option<CacheEntry<R::State, R::StateKey>>; CACHE_SIZE],
    next: usize,
    warned: bool,
    suppressed: Option<Viewports>,
}

impl<R: Rasterizer> ScissorOverride<R> {
    fn new() -> Self {
        Self {
            underlying: None,
            device: None,
            cache: std::array::from_fn(|_| None),
            next: 0,
            warned: false,
            suppressed: None,
        }
    }

    fn apply(
        &mut self,
        rasterizer: &impl Rasterizer<
            State = R::State,
            Device = R::Device,
            StateKey = R::StateKey,
            DeviceKey = R::DeviceKey,
        >,
    ) -> bool {
        if self.underlying.is_some() {
            return self.suppressed.is_none();
        }
        match self.try_apply(rasterizer) {
            Ok(()) => {
                self.suppressed = None;
                true
            }
            Err(error) => {
                if !self.warned {
                    self.warned = true;
                    tracing::warn!(%error, "reconstruction: cannot enable the D3D scissor override");
                }
                false
            }
        }
    }

    fn reapply(
        &mut self,
        rasterizer: &impl Rasterizer<
            State = R::State,
            Device = R::Device,
            StateKey = R::StateKey,
            DeviceKey = R::DeviceKey,
        >,
    ) -> bool {
        if self.suppressed.is_some() {
            return false;
        }
        if self.apply(rasterizer) {
            return true;
        }
        self.underlying = Some(rasterizer.bound());
        // No rasterizer allocation is needed to suppress pixels when an enabled variant is unavailable.
        self.suppressed = Some(rasterizer.viewports());
        rasterizer.set_viewports(&Viewports::default());
        false
    }

    fn try_apply(
        &mut self,
        rasterizer: &impl Rasterizer<
            State = R::State,
            Device = R::Device,
            StateKey = R::StateKey,
            DeviceKey = R::DeviceKey,
        >,
    ) -> Result<()> {
        let device = rasterizer.device()?;
        let device_key = R::device_key(&device);
        if self.device.as_ref().map(|(key, _)| *key) != Some(device_key) {
            self.cache.fill_with(|| None);
            self.next = 0;
            self.device = Some((device_key, device));
        }
        let base = rasterizer.bound();
        let base_key = base.as_ref().map(R::state_key);
        let mut desc = base
            .as_ref()
            .map_or_else(default_description, |state| rasterizer.description(state));
        if !desc.ScissorEnable.as_bool() {
            let index = self.cache.iter().position(|entry| {
                entry
                    .as_ref()
                    .is_some_and(|entry| entry.base_key == base_key)
            });
            let index = match index {
                Some(index) => index,
                None => {
                    desc.ScissorEnable = true.into();
                    let enabled = rasterizer.create(&desc)?;
                    let index = self.next;
                    self.cache[index] = Some(CacheEntry {
                        _base: base.clone(),
                        base_key,
                        enabled,
                    });
                    self.next = (index + 1) % CACHE_SIZE;
                    index
                }
            };
            let enabled = &self.cache[index].as_ref().unwrap().enabled;
            rasterizer.bind(Some(enabled));
        }
        self.underlying = Some(base);
        Ok(())
    }

    fn restore(
        &mut self,
        rasterizer: &impl Rasterizer<
            State = R::State,
            Device = R::Device,
            StateKey = R::StateKey,
            DeviceKey = R::DeviceKey,
        >,
    ) {
        if let Some(base) = self.underlying.take() {
            rasterizer.bind(base.as_ref());
        }
        if let Some(viewports) = self.suppressed.take()
            && rasterizer
                .viewports()
                .slots
                .iter()
                .all(|slot| slot.Width == 0.0 && slot.Height == 0.0)
        {
            rasterizer.set_viewports(&viewports);
        }
    }
}

#[derive(Clone, Default, Debug, PartialEq)]
struct Viewports {
    slots: [D3D11_VIEWPORT; D3D11_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE as usize],
    count: u32,
}

struct CacheEntry<S, K> {
    // Retaining the owner prevents pointer reuse while its identity key is cached.
    _base: Option<S>,
    base_key: Option<K>,
    enabled: S,
}

struct D3d<'a>(&'a ID3D11DeviceContext);

impl Rasterizer for D3d<'_> {
    type State = ID3D11RasterizerState;
    type Device = ID3D11Device;
    type StateKey = usize;
    type DeviceKey = usize;

    fn device_key(device: &Self::Device) -> usize {
        device.as_raw() as usize
    }

    fn state_key(state: &Self::State) -> usize {
        state.as_raw() as usize
    }

    fn device(&self) -> Result<Self::Device> {
        unsafe { self.0.GetDevice() }
    }

    fn bound(&self) -> Option<Self::State> {
        unsafe { self.0.RSGetState() }.ok()
    }

    fn description(&self, state: &Self::State) -> D3D11_RASTERIZER_DESC {
        let mut desc = D3D11_RASTERIZER_DESC::default();
        unsafe { state.GetDesc(&mut desc) };
        desc
    }

    fn create(&self, desc: &D3D11_RASTERIZER_DESC) -> Result<Self::State> {
        let mut state = None;
        unsafe {
            self.0
                .GetDevice()?
                .CreateRasterizerState(desc, Some(&mut state))?
        };
        state.ok_or_else(Error::empty)
    }

    fn bind(&self, state: Option<&Self::State>) {
        unsafe { self.0.RSSetState(state) };
    }

    fn viewports(&self) -> Viewports {
        let mut viewports = Viewports::default();
        viewports.count = viewports.slots.len() as u32;
        unsafe {
            self.0
                .RSGetViewports(&mut viewports.count, Some(viewports.slots.as_mut_ptr()))
        };
        viewports.count = viewports.count.min(viewports.slots.len() as u32);
        viewports
    }

    fn set_viewports(&self, viewports: &Viewports) {
        unsafe {
            self.0
                .RSSetViewports(Some(&viewports.slots[..viewports.count as usize]))
        };
    }
}

fn default_description() -> D3D11_RASTERIZER_DESC {
    D3D11_RASTERIZER_DESC {
        FillMode: D3D11_FILL_SOLID,
        CullMode: D3D11_CULL_BACK,
        DepthClipEnable: true.into(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use windows::Win32::Graphics::Direct3D11::{D3D11_CULL_FRONT, D3D11_FILL_WIREFRAME};

    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct State(D3D11_RASTERIZER_DESC);

    struct FakeRasterizer {
        bound: RefCell<Option<State>>,
        staged: Cell<Option<State>>,
        device: Cell<usize>,
        creations: Cell<usize>,
        fail_creation: Cell<bool>,
        viewports: RefCell<Viewports>,
    }

    impl FakeRasterizer {
        fn new(bound: Option<State>) -> Self {
            Self {
                bound: RefCell::new(bound),
                staged: Cell::new(None),
                device: Cell::new(0),
                creations: Cell::new(0),
                fail_creation: Cell::new(false),
                viewports: RefCell::new(test_viewports(1920.0)),
            }
        }

        fn flush(&self) {
            if let Some(staged) = self.staged.take() {
                self.bind(Some(&staged));
            }
        }
    }

    impl Rasterizer for FakeRasterizer {
        type State = State;
        type Device = usize;
        type DeviceKey = usize;
        type StateKey = (i32, bool);

        fn device_key(device: &usize) -> usize {
            *device
        }

        fn state_key(state: &State) -> Self::StateKey {
            (state.0.DepthBias, state.0.ScissorEnable.as_bool())
        }

        fn device(&self) -> Result<usize> {
            Ok(self.device.get())
        }

        fn bound(&self) -> Option<State> {
            self.bound.borrow().clone()
        }

        fn description(&self, state: &State) -> D3D11_RASTERIZER_DESC {
            state.0
        }

        fn create(&self, desc: &D3D11_RASTERIZER_DESC) -> Result<State> {
            if self.fail_creation.get() {
                return Err(Error::empty());
            }
            self.creations.set(self.creations.get() + 1);
            Ok(State(*desc))
        }

        fn bind(&self, state: Option<&State>) {
            *self.bound.borrow_mut() = state.cloned();
        }

        fn viewports(&self) -> Viewports {
            self.viewports.borrow().clone()
        }

        fn set_viewports(&self, viewports: &Viewports) {
            *self.viewports.borrow_mut() = viewports.clone();
        }
    }

    fn test_viewports(width: f32) -> Viewports {
        let mut viewports = Viewports {
            count: 2,
            ..Default::default()
        };
        viewports.slots[0] = D3D11_VIEWPORT {
            Width: width,
            Height: 1080.0,
            MaxDepth: 1.0,
            ..Default::default()
        };
        viewports.slots[1] = viewports.slots[0];
        viewports.slots[1].TopLeftX = width;
        viewports
    }

    fn custom_state(enabled: bool) -> State {
        State(D3D11_RASTERIZER_DESC {
            FillMode: D3D11_FILL_WIREFRAME,
            CullMode: D3D11_CULL_FRONT,
            FrontCounterClockwise: true.into(),
            DepthBias: 7,
            DepthBiasClamp: 0.75,
            SlopeScaledDepthBias: 1.25,
            DepthClipEnable: false.into(),
            ScissorEnable: enabled.into(),
            MultisampleEnable: true.into(),
            AntialiasedLineEnable: true.into(),
        })
    }

    #[test]
    fn sequential_eye_scopes_preserve_enabled_disabled_and_default_states() {
        for initial in [Some(custom_state(false)), Some(custom_state(true)), None] {
            let rasterizer = FakeRasterizer::new(initial.clone());
            let mut state = ScissorOverride::<FakeRasterizer>::new();
            let mut expected = initial.as_ref().map_or_else(default_description, |s| s.0);
            expected.ScissorEnable = true.into();
            for _eye in 0..2 {
                assert!(state.apply(&rasterizer));
                for _draw in 0..3 {
                    state.restore(&rasterizer);
                    assert_eq!(rasterizer.bound(), initial);
                    rasterizer.flush();
                    assert!(state.apply(&rasterizer));
                    assert_eq!(rasterizer.bound(), Some(State(expected)));
                    assert!(rasterizer.staged.take().is_none());
                }
                state.restore(&rasterizer);
                assert_eq!(rasterizer.bound(), initial);
            }
            assert_eq!(
                rasterizer.creations.get(),
                usize::from(initial != Some(custom_state(true)))
            );
        }
    }

    #[test]
    fn flush_captures_new_engine_state_and_drop_restores_it() {
        let rasterizer = FakeRasterizer::new(Some(custom_state(false)));
        let mut state = ScissorOverride::<FakeRasterizer>::new();
        assert!(state.apply(&rasterizer));
        let mut next = custom_state(false);
        next.0.DepthBias = 19;
        rasterizer.staged.set(Some(next.clone()));
        state.restore(&rasterizer);
        rasterizer.flush();
        assert!(state.apply(&rasterizer));
        let mut expected = next.clone();
        expected.0.ScissorEnable = true.into();
        assert_eq!(rasterizer.bound(), Some(expected));
        state.restore(&rasterizer);
        assert_eq!(rasterizer.bound(), Some(next));
    }

    #[test]
    fn cache_is_bounded_and_invalidated_on_device_change() {
        let rasterizer = FakeRasterizer::new(None);
        let mut state = ScissorOverride::<FakeRasterizer>::new();
        for i in 0..CACHE_SIZE + 3 {
            let mut base = custom_state(false);
            base.0.DepthBias = i as i32;
            rasterizer.bind(Some(&base));
            assert!(state.apply(&rasterizer));
            state.restore(&rasterizer);
        }
        assert_eq!(state.cache.iter().flatten().count(), CACHE_SIZE);
        let creations = rasterizer.creations.get();
        assert!(state.apply(&rasterizer));
        state.restore(&rasterizer);
        assert_eq!(rasterizer.creations.get(), creations);
        rasterizer.device.set(1);
        assert!(state.apply(&rasterizer));
        state.restore(&rasterizer);
        assert_eq!(rasterizer.creations.get(), creations + 1);
        assert_eq!(state.cache.iter().flatten().count(), 1);
    }

    #[test]
    fn failed_new_state_after_success_suppresses_and_recovers() {
        let rasterizer = FakeRasterizer::new(Some(custom_state(false)));
        let mut state = ScissorOverride::<FakeRasterizer>::new();
        assert!(state.apply(&rasterizer));
        let mut next = custom_state(false);
        next.0.DepthBias = 19;
        rasterizer.staged.set(Some(next.clone()));
        state.restore(&rasterizer);
        rasterizer.flush();
        rasterizer.fail_creation.set(true);
        assert!(!state.reapply(&rasterizer));
        assert!(state.suppressed.is_some());
        assert_eq!(rasterizer.viewports().count, 0);
        assert_eq!(rasterizer.bound(), Some(next.clone()));
        state.restore(&rasterizer);
        assert_eq!(rasterizer.bound(), Some(next.clone()));
        assert_eq!(rasterizer.viewports(), test_viewports(1920.0));
        assert!(!state.reapply(&rasterizer));
        state.restore(&rasterizer);
        assert_eq!(rasterizer.bound(), Some(next.clone()));
        rasterizer.fail_creation.set(false);
        assert!(state.reapply(&rasterizer));
        assert!(state.suppressed.is_none());
        state.restore(&rasterizer);
        assert_eq!(rasterizer.bound(), Some(next));
    }

    #[test]
    fn device_change_creation_failure_suppresses_without_old_device_state() {
        let rasterizer = FakeRasterizer::new(Some(custom_state(false)));
        let mut state = ScissorOverride::<FakeRasterizer>::new();
        assert!(state.apply(&rasterizer));
        state.restore(&rasterizer);
        rasterizer.device.set(1);
        rasterizer.fail_creation.set(true);
        assert!(!state.reapply(&rasterizer));
        assert_eq!(state.cache.iter().flatten().count(), 0);
        assert_eq!(rasterizer.bound(), Some(custom_state(false)));
        assert_eq!(rasterizer.viewports().count, 0);
        assert!(!state.reapply(&rasterizer));
        state.restore(&rasterizer);
        assert_eq!(rasterizer.viewports(), test_viewports(1920.0));
    }

    #[test]
    fn suppression_restore_preserves_a_new_target_viewport() {
        let rasterizer = FakeRasterizer::new(Some(custom_state(false)));
        let mut state = ScissorOverride::<FakeRasterizer>::new();
        assert!(state.apply(&rasterizer));
        state.restore(&rasterizer);
        let mut next = custom_state(false);
        next.0.DepthBias = 19;
        rasterizer.bind(Some(&next));
        rasterizer.fail_creation.set(true);
        assert!(!state.reapply(&rasterizer));
        let target = test_viewports(960.0);
        rasterizer.set_viewports(&target);
        state.restore(&rasterizer);
        assert_eq!(rasterizer.viewports(), target);
    }

    #[test]
    fn failed_creation_does_not_capture_or_change_underlying_state() {
        let base = Some(custom_state(false));
        let rasterizer = FakeRasterizer::new(base.clone());
        let mut state = ScissorOverride::<FakeRasterizer>::new();
        rasterizer.fail_creation.set(true);
        assert!(!state.apply(&rasterizer));
        assert!(state.underlying.is_none());
        assert_eq!(rasterizer.bound(), base);
        state.restore(&rasterizer);
        assert_eq!(rasterizer.bound(), base);
        rasterizer.fail_creation.set(false);
        assert!(state.apply(&rasterizer));
        state.restore(&rasterizer);
        assert_eq!(rasterizer.bound(), base);
    }
}
