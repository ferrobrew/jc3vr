//! The events the render hooks hand the capture probes: the pipeline points, the scoped blocks and
//! passes, and the draws and their submissions.
//!
//! An event says where in the frame it fires and carries what the hooks know there. It names no
//! diagnostic: which events a capture reads, and what it does at them, is the capture side's.

use std::{any::Any, ffi::c_void};

use jc3gi::{
    graphics_engine::{
        graphics_engine::RenderContext,
        post_effects::{AAMode, PostEffectContext},
        render_block::RenderBlockWaterReflectionPlane,
        render_engine::RenderEngine,
        render_pass::RenderPass,
    },
    types::math::Vector4,
};
use windows::Win32::Graphics::Direct3D11::ID3D11Resource;

use crate::stereo::single_pass::STEREO_CB_TOTAL_ROWS;

/// A point in the frame where the capture side may keep what the pipeline holds.
#[derive(Clone, Copy)]
pub enum PipelinePoint<'a> {
    /// The main G-buffer range has completed on a near dispatch.
    Geometry,
    /// The sun's shadow pass has drawn.
    AfterSun,
    /// The clustered deferred lighting has resolved.
    AfterResolve,
    /// The scene's aerial-perspective composite is about to draw.
    BeforeAtmosphere,
    /// The scene's aerial-perspective composite has drawn.
    AfterAtmosphere,
    /// The water surface is about to draw.
    BeforeWater,
    /// The water surface has drawn for the eye this carries.
    AfterWater(usize),
    /// The world post-effects block is about to draw, with the lit scene in the main colour target.
    MainColor,
    /// The engine's anti-aliasing is about to read post-chain slot `slot` of the post-effects
    /// manager `mgr`.
    BeforeAntiAliasing { mgr: *mut c_void, slot: u32 },
    /// The engine's anti-aliasing has written post-chain slot `slot` of `mgr`.
    AfterAntiAliasing { mgr: *mut c_void, slot: u32 },
    /// The upscaler is about to read eye `eye`'s motion vectors and depth.
    FsrInputs {
        eye: usize,
        motion: &'a ID3D11Resource,
        depth: &'a ID3D11Resource,
    },
    /// The render engine has uploaded its global shader constants for `ctx`; `cb13_uploaded` says
    /// whether the dual-eye constant buffer went with them.
    GlobalConstants {
        engine: &'a RenderEngine,
        ctx: &'a RenderContext,
        cb13_uploaded: bool,
    },
    /// An engine `Draw` call has returned.
    EngineDraw,
}

/// A block of the pipeline a probe may bracket.
#[derive(Clone, Copy)]
pub enum BlockScope {
    /// The engine's anti-aliasing draw, in this mode.
    AntiAliasing(AAMode),
    /// The clustered lighting's `DrawClustered`; `deferred_lights` says whether it runs in the
    /// deferred-lights pass.
    DeferredResolve { deferred_lights: bool },
    /// The ambient-occlusion block's draw.
    AmbientOcclusion,
    /// The screen-space reflection block's draw.
    ScreenSpaceReflection,
    /// A water reflection plane's proxy draw, of this block in this render context.
    WaterReflectionProxy {
        block: *const RenderBlockWaterReflectionPlane,
        rc: *const RenderContext,
    },
    /// One water surface draw in pass `pass`, for eye `eye`.
    WaterSurface { pass: i32, eye: usize },
}

/// A post effect a probe may bracket. Its apply returns the post-chain slot it wrote.
#[derive(Clone, Copy)]
pub enum PostEffect {
    /// The depth of field, applied by the post-effects manager `mgr` on chain slot `input`.
    DepthOfField {
        pec: *const PostEffectContext,
        input: u32,
        mgr: *mut c_void,
    },
}

/// A render pass a probe may bracket.
#[derive(Clone, Copy)]
pub enum PassScope {
    /// One `RenderPass::Draw`, of pass `id` when the pass is known.
    Draw { id: Option<i16> },
    /// One `RenderPass::DoDraw` walk of pass `id`, from the point the pass is published to the end
    /// of the walk.
    Walk {
        pass: *mut RenderPass,
        ctx: *mut RenderContext,
        id: i16,
    },
}

/// The `cb13` rows of a pass re-issued once per eye.
pub type Cb13Rows = [Vector4; STEREO_CB_TOTAL_ROWS];

/// A probe's bracket of one render pass the stereo path re-issues once per eye.
pub trait PerEyePassProbe {
    /// Eye `eye`'s run is about to draw, with the `live` rows and the `pinned` rows it draws with.
    fn begin_eye(&mut self, eye: usize, live: &Cb13Rows, pinned: &Cb13Rows);
    /// Eye `eye`'s run has drawn.
    fn end_eye(&mut self, eye: usize);
}

/// The device-context draw entry a draw arrived at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum RawDrawKind {
    Indexed,
    NonIndexed,
    IndexedInstanced,
    IndexedInstancedIndirect,
    InstancedIndirect,
}

impl RawDrawKind {
    pub const COUNT: usize = 5;

    pub fn name(self) -> &'static str {
        match self {
            Self::Indexed => "indexed",
            Self::NonIndexed => "non_indexed",
            Self::IndexedInstanced => "indexed_instanced",
            Self::IndexedInstancedIndirect => "indexed_instanced_indirect",
            Self::InstancedIndirect => "instanced_indirect",
        }
    }
}

/// A draw's arguments.
#[derive(Clone, Copy, Debug)]
pub enum DrawArgs {
    /// The entry's own arguments in their declared order, zero-padded.
    Direct([i64; 5]),
    /// A GPU-indirect draw's argument buffer and the byte offset of its arguments in it.
    Indirect { buffer: *mut c_void, offset: u32 },
}

/// A draw the game issued, as it arrives at a detoured entry on the immediate context `context`.
#[derive(Clone, Copy, Debug)]
pub struct GameDraw {
    pub context: *mut c_void,
    pub entry: RawDrawKind,
    pub args: DrawArgs,
}

/// What a probe holds across one game draw.
pub struct DrawScope {
    /// Released when the draw returns.
    pub guard: ProbeGuard,
    /// Whether the probe binds the draw's pixel shader itself, so the stereo path binds no variant
    /// of its own.
    pub binds_pixel_shader: bool,
}

/// A probe's state held across a draw, released on drop.
pub type ProbeGuard = Box<dyn Any>;

/// How a submission relates to the game draw it serves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// The draw as the game issued it, on the path no stereo treatment of its own took.
    AsIssued,
    /// The draw re-issued as one draw with its instance count doubled, so each instance pair
    /// covers both eyes.
    Promoted,
    /// The draw re-issued for this eye alone.
    PerEye(usize),
}

/// One submission the stereo draw path makes to the device.
#[derive(Clone, Copy, Debug)]
pub struct Submission {
    /// The immediate context the draw arrived on.
    pub context: *mut c_void,
    /// The entry the game draw arrived at.
    pub entry: RawDrawKind,
    pub route: Route,
    /// The arguments submitted, which differ from the game's on a promoted route.
    pub args: DrawArgs,
    /// The render pass being walked.
    pub pass: u8,
    /// The eye the submission draws alone, on a per-eye route or inside a block re-issued per eye.
    pub eye: Option<usize>,
}
