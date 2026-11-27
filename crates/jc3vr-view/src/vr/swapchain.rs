//! The stereo swapchain: creation at the runtime's recommended per-eye resolution, format
//! negotiation, and the per-frame image acquire/release the per-eye blit renders into.

use anyhow::Context as _;
use openxr as xr;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
};

use crate::vr::{VIEW_COUNT, VIEW_TYPE, VrConfig, eye_resolution::scaled_eye_size};

/// The stereo swapchain: a single 2-slice texture array (one slice per eye), sized from the
/// runtime's recommended per-eye resolution scaled by `vr.resolution_scale`, in a negotiated format.
pub(super) struct Swapchain {
    pub(super) handle: xr::Swapchain<xr::D3D11>,
    pub(super) width: u32,
    pub(super) height: u32,
    /// The DXGI format chosen, an 8-bit sRGB one (recorded for the per-eye blit's view).
    pub(super) format: u32,
    /// The enumerated swapchain images (raw `ID3D11Texture2D` pointers as `usize`, so the state stays
    /// `Send`; runtime-owned). Cast back to a pointer at [`Swapchain::acquired_texture`].
    images: Vec<usize>,
    /// The index returned by the most recent `acquire_image`, valid until `release_image`.
    acquired_index: Option<u32>,
}

impl Swapchain {
    /// Create the swapchain from the recommended view-configuration resolution × `resolution_scale`,
    /// negotiating an sRGB 8-bit format from `enumerate_swapchain_formats`.
    pub(super) fn create(
        instance: &xr::Instance,
        system: xr::SystemId,
        session: &xr::Session<xr::D3D11>,
        cfg: &VrConfig,
    ) -> anyhow::Result<Self> {
        let views = instance
            .enumerate_view_configuration_views(system, VIEW_TYPE)
            .context("vr: enumerating view configuration views")?;
        let view = views
            .first()
            .context("vr: the runtime reported no view configuration views")?;

        // The same size the native-resolution driver renders each eye at, so the blit stays 1:1.
        let [recommended_width, recommended_height] = cfg.recommended_size_override.unwrap_or([
            view.recommended_image_rect_width,
            view.recommended_image_rect_height,
        ]);
        let (width, height) =
            scaled_eye_size(recommended_width, recommended_height, cfg.resolution_scale);

        let formats = session
            .enumerate_swapchain_formats()
            .context("vr: enumerating swapchain formats")?;
        let format = negotiate_format(&formats)?;

        let handle = session
            .create_swapchain(&xr::SwapchainCreateInfo {
                create_flags: xr::SwapchainCreateFlags::EMPTY,
                usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT
                    | xr::SwapchainUsageFlags::SAMPLED
                    | xr::SwapchainUsageFlags::TRANSFER_DST,
                format,
                sample_count: 1,
                width,
                height,
                face_count: 1,
                array_size: VIEW_COUNT,
                mip_count: 1,
            })
            .context("vr: create_swapchain failed")?;

        let images: Vec<usize> = handle
            .enumerate_images()
            .context("vr: enumerating swapchain images")?
            .into_iter()
            .map(|ptr| ptr as usize)
            .collect();

        tracing::info!(
            target: "vr",
            width,
            height,
            format,
            image_count = images.len(),
            "created the stereo swapchain",
        );

        Ok(Self {
            handle,
            width,
            height,
            format,
            images,
            acquired_index: None,
        })
    }

    pub(super) fn acquire(&mut self) -> anyhow::Result<()> {
        let index = self
            .handle
            .acquire_image()
            .context("vr: acquire_image failed")?;
        self.handle
            .wait_image(xr::Duration::INFINITE)
            .context("vr: wait_image failed")?;
        self.acquired_index = Some(index);
        Ok(())
    }

    pub(super) fn release(&mut self) -> anyhow::Result<()> {
        self.handle
            .release_image()
            .context("vr: release_image failed")?;
        self.acquired_index = None;
        Ok(())
    }

    /// The currently acquired texture, or `None` when no image is acquired.
    pub(super) fn acquired_texture(&self) -> Option<*mut std::ffi::c_void> {
        let index = self.acquired_index? as usize;
        self.images.get(index).map(|&p| p as *mut std::ffi::c_void)
    }
}

/// Negotiate a swapchain color format from the runtime's supported list: an 8-bit sRGB format, which
/// the per-eye blit's gamma bridge requires (`crate::vr::blit`). The blit writes the game's
/// display-referred bytes through an `_SRGB` view so the hardware encode reproduces them and the
/// compositor decodes them as sRGB. A runtime offering no such format has no target the bridge is
/// right for (a plain UNORM format would store the bytes undecoded, and compositors do not agree
/// on how they treat it), so the session fails to start instead of rendering a wrong image.
fn negotiate_format(formats: &[u32]) -> anyhow::Result<u32> {
    const PREFERRED: [u32; 2] = [
        DXGI_FORMAT_R8G8B8A8_UNORM_SRGB.0 as u32,
        DXGI_FORMAT_B8G8R8A8_UNORM_SRGB.0 as u32,
    ];

    let format = PREFERRED
        .iter()
        .copied()
        .find(|f| formats.contains(f))
        .with_context(|| {
            format!("vr: the runtime offers no 8-bit sRGB swapchain format (offered: {formats:?})")
        })?;
    tracing::info!(target: "vr", format, "negotiated an sRGB swapchain format");
    Ok(format)
}

#[cfg(test)]
mod tests {
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;

    use super::*;

    #[test]
    fn the_first_preferred_srgb_format_wins() {
        let formats = [
            DXGI_FORMAT_B8G8R8A8_UNORM_SRGB.0 as u32,
            DXGI_FORMAT_R8G8B8A8_UNORM_SRGB.0 as u32,
        ];
        assert_eq!(
            negotiate_format(&formats).unwrap(),
            DXGI_FORMAT_R8G8B8A8_UNORM_SRGB.0 as u32
        );
    }

    #[test]
    fn a_runtime_without_an_srgb_format_is_refused() {
        let error = negotiate_format(&[DXGI_FORMAT_R8G8B8A8_UNORM.0 as u32]).unwrap_err();
        assert!(error.to_string().starts_with("vr: the runtime offers no"));
    }
}
