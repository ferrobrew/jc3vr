use std::{ffi::c_void, time::Duration};

use windows::{
    Win32::{
        Foundation::{S_FALSE, S_OK},
        Graphics::Direct3D11::{
            D3D11_QUERY, D3D11_QUERY_DATA_PIPELINE_STATISTICS, D3D11_QUERY_DESC,
            D3D11_QUERY_OCCLUSION, D3D11_QUERY_PIPELINE_STATISTICS, ID3D11Device,
            ID3D11DeviceContext, ID3D11Query,
        },
    },
    core::{HRESULT, Interface},
};

use crate::{
    screenshot::harness::water_gpu_probe::{ProbeContext, ProbeError},
    stereo::single_pass::{EngineContext, RawDrawKind},
};

const POLLS: u32 = 20;
const POLL_INTERVAL: Duration = Duration::from_micros(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SceneKind {
    Tess,
    NonTessDsv,
    Simulation,
}

impl SceneKind {
    const ALL: [Self; 3] = [Self::Tess, Self::NonTessDsv, Self::Simulation];

    fn name(self) -> &'static str {
        match self {
            Self::Tess => "tess",
            Self::NonTessDsv => "non_tess_dsv",
            Self::Simulation => "simulation",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SegmentMeta {
    pub(super) index: usize,
    pub(super) kind: RawDrawKind,
    pub(super) scene: SceneKind,
}

pub(super) struct BoundState {
    pub(super) scene: SceneKind,
    pub(super) dsv_bound: bool,
}

pub(super) struct QueryPair {
    pipeline: ID3D11Query,
    occlusion: ID3D11Query,
}

pub(super) struct Segment {
    pub(super) meta: SegmentMeta,
    pair: QueryPair,
    context: *mut c_void,
}

impl Segment {
    pub(super) fn begin(ctx: &ID3D11DeviceContext, meta: SegmentMeta, pair: QueryPair) -> Self {
        // SAFETY: the pair was created on this context's device. Both queries are ended by the one
        // `Segment::end` call the owning capture makes.
        unsafe {
            ctx.Begin(&pair.pipeline);
            ctx.Begin(&pair.occlusion);
        }
        Self {
            meta,
            pair,
            context: ctx.as_raw(),
        }
    }

    pub(super) fn end(&self) {
        // SAFETY: `context` is the live immediate context this segment was begun on, on this thread,
        // and the owning capture has not ended it yet.
        unsafe { end_pair(self.context, &self.pair) };
    }
}

pub(super) fn create_pairs(
    device: &ID3D11Device,
    count: usize,
) -> Result<Vec<QueryPair>, ProbeError> {
    (0..count).map(|_| QueryPair::create(device)).collect()
}

/// # Safety
///
/// `ctx` must be the live immediate context the detoured entry was called on.
pub(super) unsafe fn bound_state(ctx: &ID3D11DeviceContext) -> BoundState {
    let mut ds = None;
    let mut dsv = None;
    // SAFETY: per this function's contract, the getters only read the bound state.
    unsafe {
        ctx.DSGetShader(&mut ds, None, None);
        ctx.OMGetRenderTargets(None, Some(&mut dsv));
    }
    BoundState {
        scene: classify_scene(ds.is_some(), dsv.is_some()),
        dsv_bound: dsv.is_some(),
    }
}

pub(super) fn resolve(d3d: EngineContext, segments: &[Segment]) -> Vec<SegmentWork> {
    let mut works: Vec<SegmentWork> = segments
        .iter()
        .map(|segment| SegmentWork::pending(segment.meta))
        .collect();
    if works.is_empty() {
        return works;
    }
    let mut polls = 0;
    loop {
        let settled = d3d.with_lock(|ctx| {
            for (segment, work) in segments.iter().zip(works.iter_mut()) {
                work.poll(ctx, segment);
            }
            works.iter().all(SegmentWork::is_settled)
        });
        let Some(wait) = poll_wait(polls, settled) else {
            break;
        };
        std::thread::sleep(wait);
        polls += 1;
    }
    for work in &mut works {
        work.polls = polls;
    }
    works
}

pub(super) fn log_aggregate(context: ProbeContext, works: &[SegmentWork], draws: usize) {
    let polls = works.first().map_or(0, |work| work.polls);
    if works.is_empty() {
        tracing::info!(
            target: "water_probe",
            "water probe: gpu work pass={} eye={} invocation={} predicted_display_time_ns={} \
             generation={} status=empty segments=0 draws={draws} (no raw draw entry was captured)",
            context.pass,
            context.eye,
            context.invocation,
            context.frame.display_time(),
            context.frame.visual_generation,
        );
        return;
    }
    match Aggregate::sum(works.iter()) {
        Some(aggregate) => aggregate.log(context, works.len(), draws, polls),
        None => {
            let ready = works.iter().filter(|work| work.is_ready()).count();
            tracing::info!(
                target: "water_probe",
                "water probe: gpu work pass={} eye={} invocation={} predicted_display_time_ns={} \
                 generation={} status=inconclusive segments={} ready_segments={ready} draws={draws} \
                 polls={polls} (unresolved segments are not summed)",
                context.pass,
                context.eye,
                context.invocation,
                context.frame.display_time(),
                context.frame.visual_generation,
                works.len(),
            );
        }
    }
}

pub(super) fn log_scene_aggregates(context: ProbeContext, works: &[SegmentWork]) {
    for scene in SceneKind::ALL {
        let group: Vec<&SegmentWork> = scene_group(works, scene).collect();
        if group.is_empty() {
            continue;
        }
        let ready = group.iter().filter(|work| work.is_ready()).count();
        match Aggregate::sum(group.iter().copied()) {
            Some(aggregate) => tracing::info!(
                target: "water_probe",
                "water probe: gpu scene pass={} eye={} invocation={} predicted_display_time_ns={} \
                 generation={} scene={} status=ready segments={} \
                 attribution=between_raw_entries ia_vertices={} ia_primitives={} \
                 vs_invocations={} hs_invocations={} ds_invocations={} ps_invocations={} \
                 occlusion_samples={}",
                context.pass,
                context.eye,
                context.invocation,
                context.frame.display_time(),
                context.frame.visual_generation,
                scene.name(),
                group.len(),
                aggregate.pipeline.IAVertices,
                aggregate.pipeline.IAPrimitives,
                aggregate.pipeline.VSInvocations,
                aggregate.pipeline.HSInvocations,
                aggregate.pipeline.DSInvocations,
                aggregate.pipeline.PSInvocations,
                aggregate.occlusion_samples,
            ),
            None => tracing::info!(
                target: "water_probe",
                "water probe: gpu scene pass={} eye={} invocation={} predicted_display_time_ns={} \
                 generation={} scene={} status=inconclusive segments={} ready_segments={ready}",
                context.pass,
                context.eye,
                context.invocation,
                context.frame.display_time(),
                context.frame.visual_generation,
                scene.name(),
                group.len(),
            ),
        }
    }
}

impl QueryPair {
    fn create(device: &ID3D11Device) -> Result<Self, ProbeError> {
        let pipeline = create_query(
            device,
            D3D11_QUERY_PIPELINE_STATISTICS,
            "pipeline statistics",
        )?;
        let occlusion = create_query(device, D3D11_QUERY_OCCLUSION, "occlusion")?;
        Ok(Self {
            pipeline,
            occlusion,
        })
    }
}

pub(super) struct SegmentWork {
    meta: SegmentMeta,
    pipeline: Option<D3D11_QUERY_DATA_PIPELINE_STATISTICS>,
    occlusion_samples: Option<u64>,
    pipeline_error: Option<ProbeError>,
    occlusion_error: Option<ProbeError>,
    polls: u32,
}

impl SegmentWork {
    fn pending(meta: SegmentMeta) -> Self {
        Self {
            meta,
            pipeline: None,
            occlusion_samples: None,
            pipeline_error: None,
            occlusion_error: None,
            polls: 0,
        }
    }

    fn poll(&mut self, ctx: &ID3D11DeviceContext, segment: &Segment) {
        if self.pipeline.is_none() && self.pipeline_error.is_none() {
            let mut statistics = D3D11_QUERY_DATA_PIPELINE_STATISTICS::default();
            match get_data(ctx, &segment.pair.pipeline, &mut statistics) {
                Ok(QueryStatus::Ready) => self.pipeline = Some(statistics),
                Ok(QueryStatus::Pending) => {}
                Err(error) => self.pipeline_error = Some(error),
            }
        }
        if self.occlusion_samples.is_none() && self.occlusion_error.is_none() {
            let mut samples = 0u64;
            match get_data(ctx, &segment.pair.occlusion, &mut samples) {
                Ok(QueryStatus::Ready) => self.occlusion_samples = Some(samples),
                Ok(QueryStatus::Pending) => {}
                Err(error) => self.occlusion_error = Some(error),
            }
        }
    }

    fn is_settled(&self) -> bool {
        (self.pipeline.is_some() || self.pipeline_error.is_some())
            && (self.occlusion_samples.is_some() || self.occlusion_error.is_some())
    }

    fn is_ready(&self) -> bool {
        self.pipeline.is_some() && self.occlusion_samples.is_some()
    }

    fn error(&self) -> Option<&ProbeError> {
        self.pipeline_error
            .as_ref()
            .or(self.occlusion_error.as_ref())
    }

    pub(super) fn log(&self, context: ProbeContext) {
        let pipeline = self.pipeline;
        tracing::info!(
            target: "water_probe",
            "water probe: gpu segment pass={} eye={} invocation={} predicted_display_time_ns={} \
             generation={} draw={} kind={} scene={} status={} polls={} \
             attribution=between_raw_entries error={} ia_vertices={} ia_primitives={} \
             vs_invocations={} hs_invocations={} ds_invocations={} ps_invocations={} \
             occlusion_samples={}",
            context.pass,
            context.eye,
            context.invocation,
            context.frame.display_time(),
            context.frame.visual_generation,
            self.meta.index,
            self.meta.kind.name(),
            self.meta.scene.name(),
            if self.is_ready() { "ready" } else { "inconclusive" },
            self.polls,
            self.error()
                .map_or_else(|| "none".to_owned(), |error| error.to_string()),
            count(pipeline.map(|statistics| statistics.IAVertices)),
            count(pipeline.map(|statistics| statistics.IAPrimitives)),
            count(pipeline.map(|statistics| statistics.VSInvocations)),
            count(pipeline.map(|statistics| statistics.HSInvocations)),
            count(pipeline.map(|statistics| statistics.DSInvocations)),
            count(pipeline.map(|statistics| statistics.PSInvocations)),
            count(self.occlusion_samples),
        );
    }
}

struct Aggregate {
    pipeline: D3D11_QUERY_DATA_PIPELINE_STATISTICS,
    occlusion_samples: u64,
}

impl Aggregate {
    fn sum<'a>(works: impl IntoIterator<Item = &'a SegmentWork>) -> Option<Self> {
        let mut pipeline = D3D11_QUERY_DATA_PIPELINE_STATISTICS::default();
        let mut occlusion_samples = 0u64;
        let mut any = false;
        for work in works {
            any = true;
            let segment = work.pipeline?;
            add(&mut pipeline.IAVertices, segment.IAVertices)?;
            add(&mut pipeline.IAPrimitives, segment.IAPrimitives)?;
            add(&mut pipeline.VSInvocations, segment.VSInvocations)?;
            add(&mut pipeline.GSInvocations, segment.GSInvocations)?;
            add(&mut pipeline.GSPrimitives, segment.GSPrimitives)?;
            add(&mut pipeline.CInvocations, segment.CInvocations)?;
            add(&mut pipeline.CPrimitives, segment.CPrimitives)?;
            add(&mut pipeline.PSInvocations, segment.PSInvocations)?;
            add(&mut pipeline.HSInvocations, segment.HSInvocations)?;
            add(&mut pipeline.DSInvocations, segment.DSInvocations)?;
            add(&mut pipeline.CSInvocations, segment.CSInvocations)?;
            occlusion_samples = occlusion_samples.checked_add(work.occlusion_samples?)?;
        }
        any.then_some(Self {
            pipeline,
            occlusion_samples,
        })
    }

    fn log(&self, context: ProbeContext, segments: usize, draws: usize, polls: u32) {
        let pipeline = self.pipeline;
        tracing::info!(
            target: "water_probe",
            "water probe: gpu work pass={} eye={} invocation={} predicted_display_time_ns={} \
             generation={} status=ready segments={segments} draws={draws} polls={polls} \
             attribution=between_raw_entries pipeline_stats=ready occlusion=ready ia_vertices={} \
             ia_primitives={} vs_invocations={} \
             hs_invocations={} ds_invocations={} gs_invocations={} clipper_invocations={} \
             clipper_primitives={} ps_invocations={} cs_invocations={} occlusion_samples={}",
            context.pass,
            context.eye,
            context.invocation,
            context.frame.display_time(),
            context.frame.visual_generation,
            pipeline.IAVertices,
            pipeline.IAPrimitives,
            pipeline.VSInvocations,
            pipeline.HSInvocations,
            pipeline.DSInvocations,
            pipeline.GSInvocations,
            pipeline.CInvocations,
            pipeline.CPrimitives,
            pipeline.PSInvocations,
            pipeline.CSInvocations,
            self.occlusion_samples,
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QueryStatus {
    Ready,
    Pending,
}

fn classify_scene(domain_shader_bound: bool, depth_stencil_view_bound: bool) -> SceneKind {
    if domain_shader_bound {
        SceneKind::Tess
    } else if depth_stencil_view_bound {
        SceneKind::NonTessDsv
    } else {
        SceneKind::Simulation
    }
}

fn scene_group(works: &[SegmentWork], scene: SceneKind) -> impl Iterator<Item = &SegmentWork> {
    works.iter().filter(move |work| work.meta.scene == scene)
}

fn count(value: Option<u64>) -> String {
    value.map_or_else(|| "pending".to_owned(), |value| value.to_string())
}

fn add(total: &mut u64, value: u64) -> Option<()> {
    *total = total.checked_add(value)?;
    Some(())
}

fn create_query(
    device: &ID3D11Device,
    kind: D3D11_QUERY,
    name: &'static str,
) -> Result<ID3D11Query, ProbeError> {
    let desc = D3D11_QUERY_DESC {
        Query: kind,
        MiscFlags: 0,
    };
    let mut query = None;
    // SAFETY: `desc` is a live descriptor for the requested query type.
    unsafe { device.CreateQuery(&desc, Some(&mut query)) }
        .map_err(|error| ProbeError::CreateQuery { query: name, error })?;
    query.ok_or(ProbeError::QueryMissing { query: name })
}

unsafe fn end_pair(context: *mut c_void, pair: &QueryPair) {
    // SAFETY: per this function's contract, the pointer is the live COM interface pointer; the
    // borrowed reference ends with this call and takes no reference count.
    let Some(ctx) = (unsafe { <ID3D11DeviceContext as Interface>::from_raw_borrowed(&context) })
    else {
        return;
    };
    // SAFETY: the pair was begun on this context and has not been ended.
    unsafe {
        ctx.End(&pair.pipeline);
        ctx.End(&pair.occlusion);
    }
}

fn get_data<T>(
    ctx: &ID3D11DeviceContext,
    query: &ID3D11Query,
    data: &mut T,
) -> Result<QueryStatus, ProbeError> {
    let vtable = ctx.vtable();
    // SAFETY: `query` is live and `data` is a buffer of the size the query type writes.
    let hr = unsafe {
        (vtable.GetData)(
            ctx.as_raw(),
            query.as_raw(),
            std::ptr::from_mut(data).cast(),
            std::mem::size_of::<T>() as u32,
            0,
        )
    };
    classify(hr)
}

fn classify(hr: HRESULT) -> Result<QueryStatus, ProbeError> {
    if hr == S_OK {
        Ok(QueryStatus::Ready)
    } else if hr == S_FALSE {
        Ok(QueryStatus::Pending)
    } else {
        Err(ProbeError::GetData(hr))
    }
}

fn poll_wait(polls: u32, resolved: bool) -> Option<Duration> {
    (!resolved && polls < POLLS).then_some(POLL_INTERVAL)
}

#[cfg(test)]
mod tests {
    use windows::Win32::Foundation::E_FAIL;

    use super::*;

    fn meta(index: usize, scene: SceneKind) -> SegmentMeta {
        SegmentMeta {
            index,
            kind: RawDrawKind::Indexed,
            scene,
        }
    }

    fn ready(index: usize, scene: SceneKind, ds_invocations: u64, occlusion: u64) -> SegmentWork {
        let mut work = SegmentWork::pending(meta(index, scene));
        work.pipeline = Some(D3D11_QUERY_DATA_PIPELINE_STATISTICS {
            DSInvocations: ds_invocations,
            PSInvocations: ds_invocations,
            ..Default::default()
        });
        work.occlusion_samples = Some(occlusion);
        work
    }

    #[test]
    fn query_status_separates_ready_pending_and_error() {
        assert_eq!(classify(S_OK).ok(), Some(QueryStatus::Ready));
        assert_eq!(classify(S_FALSE).ok(), Some(QueryStatus::Pending));
        assert!(matches!(classify(E_FAIL), Err(ProbeError::GetData(hr)) if hr == E_FAIL));
    }

    #[test]
    fn polling_stops_at_the_cap_and_reports_pending() {
        assert_eq!(poll_wait(0, false), Some(POLL_INTERVAL));
        assert_eq!(poll_wait(POLLS - 1, false), Some(POLL_INTERVAL));
        assert_eq!(poll_wait(POLLS, false), None);
        assert_eq!(poll_wait(0, true), None);
        assert_eq!(poll_wait(POLLS, true), None);
    }

    #[test]
    fn scene_classification_prefers_the_domain_shader() {
        assert_eq!(classify_scene(true, true), SceneKind::Tess);
        assert_eq!(classify_scene(true, false), SceneKind::Tess);
        assert_eq!(classify_scene(false, true), SceneKind::NonTessDsv);
        assert_eq!(classify_scene(false, false), SceneKind::Simulation);
    }

    #[test]
    fn aggregate_sums_every_ready_segment() {
        let works = [
            ready(0, SceneKind::Tess, 10, 100),
            ready(1, SceneKind::NonTessDsv, 5, 50),
        ];
        let aggregate = Aggregate::sum(works.iter()).expect("both segments ready");
        assert_eq!(aggregate.pipeline.DSInvocations, 15);
        assert_eq!(aggregate.pipeline.PSInvocations, 15);
        assert_eq!(aggregate.occlusion_samples, 150);
    }

    #[test]
    fn aggregate_is_absent_when_a_segment_is_unresolved() {
        let works = [
            ready(0, SceneKind::Tess, 10, 100),
            SegmentWork::pending(meta(1, SceneKind::Tess)),
        ];
        assert!(Aggregate::sum(works.iter()).is_none());
    }

    #[test]
    fn aggregate_is_absent_for_no_segments() {
        assert!(Aggregate::sum(std::iter::empty()).is_none());
    }

    #[test]
    fn scene_groups_only_their_own_kind() {
        let works = [
            ready(0, SceneKind::Tess, 1, 10),
            ready(1, SceneKind::Simulation, 2, 20),
            SegmentWork::pending(meta(2, SceneKind::Tess)),
        ];
        assert_eq!(scene_group(&works, SceneKind::Tess).count(), 2);
        assert_eq!(scene_group(&works, SceneKind::NonTessDsv).count(), 0);
        let simulation: Vec<&SegmentWork> = scene_group(&works, SceneKind::Simulation).collect();
        assert!(Aggregate::sum(simulation).is_some());
        let tess: Vec<&SegmentWork> = scene_group(&works, SceneKind::Tess).collect();
        assert!(Aggregate::sum(tess).is_none());
    }
}
