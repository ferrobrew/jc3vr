//! The GPU resources the redirected HUD renders into, and the textures the mod paints on the CPU
//! for its own HUD quads.

use std::{collections::HashMap, hash::Hash};

use anyhow::Context as _;
use jc3gi::graphics_engine::device::Device;
use windows::Win32::Graphics::{
    Direct3D11::{
        D3D11_BIND_DEPTH_STENCIL, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE,
        D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, ID3D11DepthStencilView, ID3D11DeviceContext,
        ID3D11RenderTargetView, ID3D11ShaderResourceView, ID3D11Texture2D,
    },
    Dxgi::Common::{DXGI_FORMAT_D24_UNORM_S8_UINT, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC},
};

/// Our offscreen HUD render target: a color texture (with an RTV to render into and an SRV to preview)
/// and a matching depth-stencil for the UI's depth clears, plus the size they were built for.
pub(crate) struct HudTarget {
    /// The colour texture; region copies (the world markers' crops) address it directly.
    color: ID3D11Texture2D,
    color_srv: ID3D11ShaderResourceView,
    color_rtv: ID3D11RenderTargetView,
    _depth: ID3D11Texture2D,
    depth_dsv: ID3D11DepthStencilView,
    size: (u32, u32),
}

impl HudTarget {
    /// Build the color texture (with RTV + SRV) and a matching depth-stencil at `width` x `height`.
    pub(crate) fn new(device: &Device, width: u32, height: u32) -> anyhow::Result<Self> {
        crate::config::validate::hud::dimension("HUD target width", width)?;
        crate::config::validate::hud::dimension("HUD target height", height)?;
        let d3d = &device.m_Device;
        // SAFETY: `d3d` is the live engine device; the descriptors below are valid for these calls.
        unsafe {
            let mut color: Option<ID3D11Texture2D> = None;
            d3d.CreateTexture2D(
                &D3D11_TEXTURE2D_DESC {
                    Width: width,
                    Height: height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                },
                None,
                Some(&mut color),
            )
            .context("creating the HUD color texture")?;
            let color = color.context("the HUD color texture was not created")?;

            let mut color_rtv: Option<ID3D11RenderTargetView> = None;
            d3d.CreateRenderTargetView(&color, None, Some(&mut color_rtv))
                .context("creating the HUD render-target view")?;
            let color_rtv = color_rtv.context("the HUD render-target view was not created")?;

            let mut color_srv: Option<ID3D11ShaderResourceView> = None;
            d3d.CreateShaderResourceView(&color, None, Some(&mut color_srv))
                .context("creating the HUD shader-resource view")?;
            let color_srv = color_srv.context("the HUD shader-resource view was not created")?;

            let mut depth: Option<ID3D11Texture2D> = None;
            d3d.CreateTexture2D(
                &D3D11_TEXTURE2D_DESC {
                    Width: width,
                    Height: height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_D24_UNORM_S8_UINT,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_DEPTH_STENCIL.0 as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                },
                None,
                Some(&mut depth),
            )
            .context("creating the HUD depth texture")?;
            let depth = depth.context("the HUD depth texture was not created")?;

            let mut depth_dsv: Option<ID3D11DepthStencilView> = None;
            d3d.CreateDepthStencilView(&depth, None, Some(&mut depth_dsv))
                .context("creating the HUD depth-stencil view")?;
            let depth_dsv = depth_dsv.context("the HUD depth-stencil view was not created")?;

            Ok(HudTarget {
                color,
                color_srv,
                color_rtv,
                _depth: depth,
                depth_dsv,
                size: (width, height),
            })
        }
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        self.size
    }

    pub(crate) fn color_texture(&self) -> &ID3D11Texture2D {
        &self.color
    }

    pub(crate) fn color_srv(&self) -> &ID3D11ShaderResourceView {
        &self.color_srv
    }

    pub(crate) fn color_rtv(&self) -> &ID3D11RenderTargetView {
        &self.color_rtv
    }

    pub(crate) fn depth_dsv(&self) -> &ID3D11DepthStencilView {
        &self.depth_dsv
    }

    /// A texture of `width` x `height` texels holding `texels` ([`Self::upload`]), for `what`, a
    /// name for the error's context.
    ///
    /// The caller holds the engine context mutex.
    pub(crate) fn with_texels(
        device: &Device,
        context: &ID3D11DeviceContext,
        (width, height): (u32, u32),
        texels: &[u8],
        what: &str,
    ) -> anyhow::Result<Self> {
        let target = Self::new(device, width, height)
            .with_context(|| format!("creating {what}'s texture"))?;
        target.upload(context, texels);
        Ok(target)
    }

    /// Replace the colour texture's texels with `texels`: straight rows of RGBA, top first, one
    /// texel per texel of the target. A buffer of any other length is not uploaded.
    ///
    /// The caller holds the engine context mutex.
    pub(crate) fn upload(&self, context: &ID3D11DeviceContext, texels: &[u8]) {
        let (width, height) = self.size;
        let expected = width as usize * height as usize * 4;
        debug_assert_eq!(texels.len(), expected, "a HUD texture's texel count");
        if texels.len() != expected {
            return;
        }
        // SAFETY: the texture is ours, of the default usage, and `texels` holds its every RGBA
        // texel, checked above; the caller holds the engine context mutex.
        unsafe {
            context.UpdateSubresource(&self.color, 0, None, texels.as_ptr().cast(), width * 4, 0);
        }
    }
}

/// A texture painted on the CPU and repainted only when what it shows, its key, changes.
pub(crate) struct KeyedTexture<K> {
    target: HudTarget,
    key: Option<K>,
}

impl<K: Copy + PartialEq> KeyedTexture<K> {
    /// A texture of `width` x `height` texels, not yet painted.
    pub(crate) fn new(device: &Device, (width, height): (u32, u32)) -> anyhow::Result<Self> {
        Ok(Self {
            target: HudTarget::new(device, width, height)?,
            key: None,
        })
    }

    pub(crate) fn target(&self) -> &HudTarget {
        &self.target
    }

    /// Repaint the texture for `key` with `paint`'s texels ([`HudTarget::upload`]) unless it
    /// already shows `key`.
    ///
    /// The caller holds the engine context mutex.
    pub(crate) fn update(
        &mut self,
        context: &ID3D11DeviceContext,
        key: K,
        paint: impl FnOnce(K) -> Vec<u8>,
    ) {
        if self.key == Some(key) {
            return;
        }
        self.target.upload(context, &paint(key));
        self.key = Some(key);
    }
}

/// Painted textures kept by what they show, at most a fixed number: a full cache makes room for a
/// new one by dropping one no longer in use.
pub(crate) struct TextureCache<K, V = HudTarget> {
    textures: HashMap<K, V>,
    capacity: usize,
}

impl<K: Copy + Eq + Hash, V> TextureCache<K, V> {
    /// An empty cache of at most `capacity` textures.
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            textures: HashMap::new(),
            capacity,
        }
    }

    pub(crate) fn get(&self, key: &K) -> Option<&V> {
        self.textures.get(key)
    }

    pub(crate) fn contains(&self, key: &K) -> bool {
        self.textures.contains_key(key)
    }

    /// Make room for one more texture, dropping one whose key `in_use` rejects when the cache is
    /// full. `false` when it is full of textures in use.
    pub(crate) fn make_room(&mut self, in_use: impl Fn(&K) -> bool) -> bool {
        if self.textures.len() < self.capacity {
            return true;
        }
        let Some(unused) = self.textures.keys().copied().find(|key| !in_use(key)) else {
            return false;
        };
        self.textures.remove(&unused);
        true
    }

    /// Keep `texture` for `key`; the caller made room for it first ([`Self::make_room`]).
    pub(crate) fn insert(&mut self, key: K, texture: V) {
        debug_assert!(self.textures.len() < self.capacity || self.contains(&key));
        self.textures.insert(key, texture);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_cache_drops_a_texture_no_longer_in_use() {
        let mut cache = TextureCache::<&str, u32>::new(2);
        for (key, texture) in [("a", 1), ("b", 2)] {
            assert!(cache.make_room(|_| true));
            cache.insert(key, texture);
        }
        assert!(!cache.make_room(|_| true), "every texture is in use");
        assert!(cache.make_room(|&key| key == "b"));
        cache.insert("c", 3);
        assert_eq!(
            ["a", "b", "c"].map(|key| cache.get(&key).copied()),
            [None, Some(2), Some(3)]
        );
    }
}
