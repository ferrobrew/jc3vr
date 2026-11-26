use std::{cell::RefCell, ffi::c_void};

use jc3gi::graphics_engine::{post_effects::AAMode, render_engine::RenderPassId};
use jc3vr_view::stereo::draw_index;
use serde::Serialize;
use windows::{Win32::Graphics::Direct3D11::*, core::Interface};

use crate::{
    screenshot::{
        harness,
        harness::stages::{STATE, Stage},
    },
    stereo::single_pass::shader_identity,
};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Observation {
    id: String,
    frame: i64,
    eye: usize,
    mode: i32,
    draws: Vec<Draw>,
    overflow: bool,
    color_edge: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
struct Draw {
    pass: u8,
    vertices: u32,
    start: u32,
    ps: Option<shader_identity::IdentityResult>,
    resources: Vec<(usize, usize, u32, u32, i32)>,
    samplers: Vec<(usize, i32, i32, i32)>,
    target: Option<usize>,
}

thread_local! { static CURRENT: RefCell<Option<Observation>> = const { RefCell::new(None) }; }

pub(crate) struct Scope;

impl Scope {
    pub(crate) fn begin(mode: AAMode) -> Option<Self> {
        if !harness::screenshot_wants_eyes() {
            return None;
        }
        let frame = harness::frame_identity().filter(|&f| harness::aa_draws_requested(f))?;
        let id = harness::diagnostic_capture_id()?;
        CURRENT.replace(Some(Observation {
            id,
            frame,
            eye: draw_index(),
            mode: mode as i32,
            draws: Vec::new(),
            overflow: false,
            color_edge: None,
        }));
        Some(Self)
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        if let Some(obs) = CURRENT.take()
            && let Some(record) = STATE
                .lock()
                .records
                .iter_mut()
                .find(|r| r.stage == Stage::PostAa && r.frame == obs.frame && r.eye == obs.eye)
        {
            record.aa = Some(obs);
        }
    }
}

pub(crate) unsafe fn observe(context: *mut c_void, vertices: u32, start: u32) {
    CURRENT.with_borrow_mut(|current| {
        let Some(obs) = current else { return };
        if draw_index() != obs.eye
            || harness::frame_identity() != Some(obs.frame)
            || obs.draws.len() >= 12
        {
            obs.overflow = true;
            return;
        }
        let Some(ctx) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }) else {
            obs.overflow = true;
            return;
        };
        let (mut ps, mut target) = (None, None);
        let mut views: [Option<ID3D11ShaderResourceView>; 16] = Default::default();
        let mut samplers: [Option<ID3D11SamplerState>; 16] = Default::default();
        unsafe {
            ctx.PSGetShader(&mut ps, None, None);
            ctx.PSGetShaderResources(0, Some(&mut views));
            ctx.PSGetSamplers(0, Some(&mut samplers));
            ctx.OMGetRenderTargets(Some(std::slice::from_mut(&mut target)), None);
        }
        if harness::aa_weights_requested(obs.frame) && obs.mode == AAMode::AA_SMAA as i32 && (vertices,start)==(3,0) && crate::stereo::single_pass::current_pass_id()==RenderPassId::RP_POSTEFFECTS as u8 && obs.draws.len()==2 && matches!(ps.as_ref().map(shader_identity::lookup), Some(shader_identity::IdentityResult::Mapped {identity}) if identity.actual_sha256=="a4078e6bf737300492eaf5c154ac6b232a537d022396d8c0892635063f17d765") {
            let mut record=crate::screenshot::harness::stages::empty_stage_record(obs.frame,obs.eye,Stage::AaWeights);
            let result=(|| -> anyhow::Result<_> {
                let view=views[1].as_ref().ok_or_else(|| anyhow::anyhow!("missing weights"))?;
                let resource=unsafe{view.GetResource()?}; let texture=resource.cast::<ID3D11Texture2D>()?;
                let mut desc=D3D11_TEXTURE2D_DESC::default(); let mut srv=D3D11_SHADER_RESOURCE_VIEW_DESC::default();unsafe{texture.GetDesc(&mut desc);view.GetDesc(&mut srv)};
                anyhow::ensure!(obs.draws[1].target==Some(resource.as_raw() as usize) && matches!(desc.Width,320|640) && desc.Height==360 && desc.ArraySize==1 && desc.MipLevels==1 && desc.SampleDesc.Count==1 && srv.Format.0==28,"AA: unexpected weights producer or texture");
                crate::screenshot::harness::stages::capture_raster_state(ctx,&mut record);
                crate::screenshot::harness::stages::read_texture(ctx,&texture,&mut record)
            })();
            crate::screenshot::harness::stages::dof::store(record,result);
        }
        let resources = views
            .into_iter()
            .enumerate()
            .filter_map(|(slot, s)| {
                let s = s?;
                let r = unsafe { s.GetResource() }.ok()?;
                let texture = r.cast::<ID3D11Texture2D>().ok()?;
                let mut d = D3D11_TEXTURE2D_DESC::default();
                let mut v = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
                unsafe {
                    texture.GetDesc(&mut d);
                    s.GetDesc(&mut v)
                };
                Some((slot, r.as_raw() as usize, d.Width, d.Height, v.Format.0))
            })
            .collect();
        let samplers = samplers
            .into_iter()
            .enumerate()
            .filter_map(|(slot, s)| {
                let s = s?;
                let mut d = D3D11_SAMPLER_DESC::default();
                unsafe { s.GetDesc(&mut d) };
                Some((slot, d.Filter.0, d.AddressU.0, d.AddressV.0))
            })
            .collect();
        obs.draws.push(Draw {
            pass: crate::stereo::single_pass::current_pass_id(),
            vertices,
            start,
            ps: ps.as_ref().map(shader_identity::lookup),
            resources,
            samplers,
            target: target
                .and_then(|v| unsafe { v.GetResource() }.ok())
                .map(|r| r.as_raw() as usize),
        });
    });
}

pub(crate) unsafe fn with_color_edge(context: *mut c_void, submit: &dyn Fn()) -> bool {
    CURRENT.with_borrow_mut(|current| {
        let Some(obs) = current else { return false };
        let Some(treated) = harness::aa_color_edge(obs.frame) else {return false};
        if obs.draws.len()!=3 || obs.eye!=0 || obs.mode!=AAMode::AA_SMAA as i32 || obs.overflow {return false}
        let d=&obs.draws[2];
        if (d.pass,d.vertices,d.start)!=(RenderPassId::RP_POSTEFFECTS as u8,3,0) || !matches!(&d.ps,Some(shader_identity::IdentityResult::Mapped{identity}) if identity.actual_sha256==shader_identity::AA_PS && identity.pristine_sha256==shader_identity::AA_PS) || !d.resources.iter().any(|&(s,_,w,h,f)| (s,w,h,f)==(0,640,360,29)) || !d.resources.iter().any(|&(s,_,w,h,f)| (s,w,h,f)==(1,640,360,28)) || !d.samplers.contains(&(0,20,3,3)) {return false}
        let result=(|| -> anyhow::Result<()> {
            let ctx=unsafe {ID3D11DeviceContext::from_raw_borrowed(&context)}.ok_or_else(|| anyhow::anyhow!("AA: missing context"))?;
            let mut original=None;unsafe{ctx.PSGetShader(&mut original,None,None)};
            let original=original.ok_or_else(|| anyhow::anyhow!("AA: missing original shader"))?;
            let mut bytes=shader_identity::aa_bytecode().ok_or_else(|| anyhow::anyhow!("AA: missing original bytes"))?;
            if treated {bytes=dxbc_stereo::local_aa_neighbor(&bytes)?;}
            let mut ps=None;unsafe{ctx.GetDevice()?.CreatePixelShader(&bytes,None,Some(&mut ps))?};
            let replacement=ps.ok_or_else(|| anyhow::anyhow!("AA: missing shader"))?;
            let mut bound=None;unsafe{ctx.PSSetShader(&replacement,None);ctx.PSGetShader(&mut bound,None,None)};
            if bound.as_ref()!=Some(&replacement) { unsafe {ctx.PSSetShader(&original,None)}; anyhow::bail!("AA: binding mismatch"); }
            submit();
            unsafe{ctx.PSSetShader(&original,None);ctx.PSGetShader(&mut bound,None,None)};
            if bound.as_ref()==Some(&original) {obs.color_edge=Some(treated);} else {obs.overflow=true;}
            Ok(())
        })();
        if let Err(error)=result {tracing::warn!(%error,"AA color edge");obs.overflow=true;false} else {true}
    })
}
