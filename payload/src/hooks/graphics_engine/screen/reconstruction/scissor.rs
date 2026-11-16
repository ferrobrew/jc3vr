use std::cell::RefCell;

use windows::{
    Win32::Graphics::Direct3D11::{
        D3D11_CULL_BACK, D3D11_FILL_SOLID, D3D11_RASTERIZER_DESC, ID3D11Device,
        ID3D11DeviceContext, ID3D11RasterizerState,
    },
    core::{Error, Result},
};

pub(super) fn apply(context: &ID3D11DeviceContext) -> bool {
    OVERRIDE.with_borrow_mut(|state| state.apply(&D3d(context)))
}

pub(super) fn restore(context: &ID3D11DeviceContext) {
    OVERRIDE.with_borrow_mut(|state| state.restore(&D3d(context)));
}

const CACHE_SIZE: usize = 32;

thread_local! {
    static OVERRIDE: RefCell<ScissorOverride<D3d<'static>>> = RefCell::new(ScissorOverride::new());
}

trait Rasterizer {
    type State: Clone + PartialEq;
    type Device: PartialEq;

    fn device(&self) -> Result<Self::Device>;
    fn bound(&self) -> Option<Self::State>;
    fn description(&self, state: &Self::State) -> D3D11_RASTERIZER_DESC;
    fn create(&self, desc: &D3D11_RASTERIZER_DESC) -> Result<Self::State>;
    fn bind(&self, state: Option<&Self::State>);
}

struct ScissorOverride<R: Rasterizer> {
    // The inner `None` is the D3D default state; the outer `None` means no override is bound.
    underlying: Option<Option<R::State>>,
    device: Option<R::Device>,
    cache: [Option<CacheEntry<R::State>>; CACHE_SIZE],
    next: usize,
    warned: bool,
}

impl<R: Rasterizer> ScissorOverride<R> {
    fn new() -> Self {
        Self {
            underlying: None,
            device: None,
            cache: std::array::from_fn(|_| None),
            next: 0,
            warned: false,
        }
    }

    fn apply(
        &mut self,
        rasterizer: &impl Rasterizer<State = R::State, Device = R::Device>,
    ) -> bool {
        if self.underlying.is_some() {
            return true;
        }
        match self.try_apply(rasterizer) {
            Ok(()) => true,
            Err(error) => {
                if !self.warned {
                    self.warned = true;
                    tracing::warn!(%error, "reconstruction: cannot enable the D3D scissor override");
                }
                false
            }
        }
    }

    fn try_apply(
        &mut self,
        rasterizer: &impl Rasterizer<State = R::State, Device = R::Device>,
    ) -> Result<()> {
        let device = rasterizer.device()?;
        if self.device.as_ref() != Some(&device) {
            self.cache.fill_with(|| None);
            self.next = 0;
            self.device = Some(device);
        }
        let base = rasterizer.bound();
        let mut desc = base
            .as_ref()
            .map_or_else(default_description, |state| rasterizer.description(state));
        if !desc.ScissorEnable.as_bool() {
            let index = self
                .cache
                .iter()
                .position(|entry| entry.as_ref().is_some_and(|entry| entry.base == base));
            let index = match index {
                Some(index) => index,
                None => {
                    desc.ScissorEnable = true.into();
                    let enabled = rasterizer.create(&desc)?;
                    let index = self.next;
                    self.cache[index] = Some(CacheEntry {
                        base: base.clone(),
                        enabled,
                    });
                    self.next = (index + 1) % CACHE_SIZE;
                    index
                }
            };
            rasterizer.bind(Some(&self.cache[index].as_ref().unwrap().enabled));
        }
        self.underlying = Some(base);
        Ok(())
    }

    fn restore(&mut self, rasterizer: &impl Rasterizer<State = R::State, Device = R::Device>) {
        if let Some(base) = self.underlying.take() {
            rasterizer.bind(base.as_ref());
        }
    }
}

struct CacheEntry<S> {
    base: Option<S>,
    enabled: S,
}

struct D3d<'a>(&'a ID3D11DeviceContext);

impl Rasterizer for D3d<'_> {
    type State = ID3D11RasterizerState;
    type Device = ID3D11Device;

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
    }

    impl FakeRasterizer {
        fn new(bound: Option<State>) -> Self {
            Self {
                bound: RefCell::new(bound),
                staged: Cell::new(None),
                device: Cell::new(0),
                creations: Cell::new(0),
                fail_creation: Cell::new(false),
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
