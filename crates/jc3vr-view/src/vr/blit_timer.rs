//! GPU timing of the frame tail's swapchain blit, for a benchmark run that times passes.
//!
//! The submit's blit (`crate::vr::blit`) runs on the frame tail, outside every dispatch, so the
//! dispatch timestamp layer (`jc3vr_render::profiler::gpu`) cannot bracket it. While a benchmark run
//! asks for per-pass times, [`TailBlitTimer`] brackets it with a timestamp pair inside a disjoint query of
//! its own, reads the result back a few frames later without waiting, and files it into the run's
//! totals (`crate::gpu_passes`) as the detail `frame tail: swapchain blit` on
//! lane [`TAIL_LANE`].

use std::iter;

use windows::{
    Win32::{
        Foundation::S_OK,
        Graphics::Direct3D11::{
            D3D11_QUERY_DATA_TIMESTAMP_DISJOINT, D3D11_QUERY_DESC, D3D11_QUERY_TIMESTAMP,
            D3D11_QUERY_TIMESTAMP_DISJOINT, ID3D11Device, ID3D11DeviceContext, ID3D11Query,
        },
    },
    core::Interface as _,
};

use crate::gpu_passes::{self, GpuPassLabel};

/// The lane the tail blit's time is filed under, past the dispatch lanes.
pub(super) const TAIL_LANE: u8 = 3;

/// A small ring of query sets, so a frame's read-back can trail its submit by a few frames.
#[derive(Default)]
pub(super) struct TailBlitTimer {
    sets: Vec<QuerySet>,
}

impl TailBlitTimer {
    /// File every resolved measurement, then, with `measure`, start one around the work that
    /// follows, when a benchmark run times passes. Returns the set to [`end`](Self::end) after the
    /// work, or `None` when nothing is measured or no set is free.
    ///
    /// # Safety
    /// `context` is the engine's immediate context, held under its mutex; `device` created it.
    pub(super) unsafe fn begin(
        &mut self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        measure: bool,
    ) -> Option<usize> {
        // SAFETY: as the caller guarantees.
        unsafe { self.collect(context) };
        if !measure || !gpu_passes::armed() {
            return None;
        }
        let index = match self.sets.iter().position(|set| !set.pending) {
            Some(index) => index,
            None if self.sets.len() < RING => {
                // SAFETY: `device` is live.
                self.sets.push(unsafe { QuerySet::new(device) }?);
                self.sets.len() - 1
            }
            None => return None,
        };
        let set = &mut self.sets[index];
        // SAFETY: the queries belong to `device`, whose immediate context this is.
        unsafe {
            context.Begin(&set.disjoint);
            context.End(&set.begin);
        }
        set.pending = true;
        Some(index)
    }

    /// Close the measurement [`begin`](Self::begin) started.
    ///
    /// # Safety
    /// As [`begin`](Self::begin).
    pub(super) unsafe fn end(&mut self, context: &ID3D11DeviceContext, index: usize) {
        if let Some(set) = self.sets.get(index) {
            // SAFETY: as the caller guarantees.
            unsafe {
                context.End(&set.end);
                context.End(&set.disjoint);
            }
        }
    }

    /// File every pending measurement the GPU has resolved, without waiting on the rest.
    unsafe fn collect(&mut self, context: &ID3D11DeviceContext) {
        for set in self.sets.iter_mut().filter(|set| set.pending) {
            // SAFETY: the queries were ended on this context.
            let Some(ns) = (unsafe { set.read(context) }) else {
                continue;
            };
            set.pending = false;
            if let Some(ns) = ns {
                gpu_passes::record(
                    TAIL_LANE,
                    iter::once((GpuPassLabel::Detail("frame tail: swapchain blit"), ns)),
                );
            }
        }
    }
}

/// How many measurements may be in flight at once.
const RING: usize = 4;

/// One measurement's queries.
struct QuerySet {
    disjoint: ID3D11Query,
    begin: ID3D11Query,
    end: ID3D11Query,
    pending: bool,
}

impl QuerySet {
    unsafe fn new(device: &ID3D11Device) -> Option<Self> {
        let create = |query| {
            let mut out: Option<ID3D11Query> = None;
            let desc = D3D11_QUERY_DESC {
                Query: query,
                MiscFlags: 0,
            };
            // SAFETY: `device` is live and `desc` well-formed.
            unsafe { device.CreateQuery(&desc, Some(&mut out)) }.ok()?;
            out
        };
        Some(Self {
            disjoint: create(D3D11_QUERY_TIMESTAMP_DISJOINT)?,
            begin: create(D3D11_QUERY_TIMESTAMP)?,
            end: create(D3D11_QUERY_TIMESTAMP)?,
            pending: false,
        })
    }

    /// `None` while the GPU has not resolved the set; `Some(None)` when it has but the clock was
    /// disjoint; `Some(Some(ns))` otherwise.
    unsafe fn read(&self, context: &ID3D11DeviceContext) -> Option<Option<i64>> {
        let mut disjoint = D3D11_QUERY_DATA_TIMESTAMP_DISJOINT::default();
        let mut begin = 0u64;
        let mut end = 0u64;
        // SAFETY: each out-pointer is sized for its query's data. The raw vtable call keeps
        // `S_FALSE` (not yet resolved) apart from `S_OK`, which the safe wrapper folds together.
        unsafe {
            if !get(context, &self.disjoint, &mut disjoint) {
                return None;
            }
            if !get(context, &self.begin, &mut begin) || !get(context, &self.end, &mut end) {
                return None;
            }
        }
        if disjoint.Disjoint.as_bool() || disjoint.Frequency == 0 || end < begin {
            return Some(None);
        }
        let ns = (u128::from(end - begin) * 1_000_000_000 / u128::from(disjoint.Frequency)) as i64;
        Some(Some(ns))
    }
}

/// Read a query's data without flushing; whether it was resolved.
unsafe fn get<T>(context: &ID3D11DeviceContext, query: &ID3D11Query, out: &mut T) -> bool {
    // `D3D11_ASYNC_GETDATA_DONOTFLUSH`.
    const DO_NOT_FLUSH: u32 = 1;
    // SAFETY: as the caller guarantees.
    let hr = unsafe {
        (context.vtable().GetData)(
            context.as_raw(),
            query.as_raw(),
            (out as *mut T).cast(),
            std::mem::size_of::<T>() as u32,
            DO_NOT_FLUSH,
        )
    };
    hr == S_OK
}
