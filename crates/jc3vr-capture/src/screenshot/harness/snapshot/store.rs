//! The snapshot planes' GPU copies: a texture per captured source, with the views the diff shader
//! reads it through.

use anyhow::{Context as _, bail};
use windows::{
    Win32::Graphics::{
        Direct3D::D3D11_SRV_DIMENSION_TEXTURE2D,
        Direct3D11::{
            D3D11_BIND_SHADER_RESOURCE, D3D11_SHADER_RESOURCE_VIEW_DESC,
            D3D11_SHADER_RESOURCE_VIEW_DESC_0, D3D11_TEX2D_SRV, D3D11_TEXTURE2D_DESC,
            D3D11_USAGE_DEFAULT, ID3D11Device, ID3D11DeviceContext, ID3D11Resource,
            ID3D11ShaderResourceView, ID3D11Texture2D,
        },
        Dxgi::Common::{
            DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_TYPELESS, DXGI_FORMAT_B8G8R8A8_UNORM,
            DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, DXGI_FORMAT_D32_FLOAT,
            DXGI_FORMAT_D32_FLOAT_S8X24_UINT, DXGI_FORMAT_R8G8B8A8_SINT,
            DXGI_FORMAT_R8G8B8A8_SNORM, DXGI_FORMAT_R8G8B8A8_TYPELESS, DXGI_FORMAT_R8G8B8A8_UINT,
            DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
            DXGI_FORMAT_R10G10B10A2_TYPELESS, DXGI_FORMAT_R10G10B10A2_UINT,
            DXGI_FORMAT_R10G10B10A2_UNORM, DXGI_FORMAT_R11G11B10_FLOAT, DXGI_FORMAT_R16G16_FLOAT,
            DXGI_FORMAT_R16G16_TYPELESS, DXGI_FORMAT_R32_FLOAT,
            DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS, DXGI_FORMAT_R32_TYPELESS,
            DXGI_FORMAT_R32G8X24_TYPELESS, DXGI_FORMAT_X32_TYPELESS_G8X24_UINT, DXGI_SAMPLE_DESC,
        },
    },
    core::Interface as _,
};

use crate::screenshot::harness::snapshot::summary::Kind;

/// A GPU copy of one captured source (a whole texture, which may hold both eyes side by side).
pub(crate) struct PlaneTexture {
    pub texture: ID3D11Texture2D,
    pub format: Format,
    pub width: u32,
    pub height: u32,
    /// The view the shader reads values (or codes) through: `ValueA`/`ValueB` for the float kinds,
    /// `UintA`/`UintB` for [`Kind::Uint4`].
    pub values: ID3D11ShaderResourceView,
    /// The stencil view of a depth plane that carries one.
    pub stencil: Option<ID3D11ShaderResourceView>,
}

/// How a source format is copied and read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Format {
    pub kind: Kind,
    /// The copy's (typeless where one exists) storage format.
    pub storage: DXGI_FORMAT,
    pub values: DXGI_FORMAT,
    pub stencil: Option<DXGI_FORMAT>,
    /// The bytes a pixel takes, for a dump.
    pub(crate) pixel_bytes: u32,
    /// Whether the 8-bit channels are stored blue first, for a dump.
    pub bgra: bool,
}

impl Format {
    /// How `format` is copied and read, or an error for a format the comparison does not handle.
    pub(crate) fn of(format: DXGI_FORMAT) -> anyhow::Result<Self> {
        let plain = |kind, storage, values, pixel_bytes, bgra| Self {
            kind,
            storage,
            values,
            stencil: None,
            pixel_bytes,
            bgra,
        };
        Ok(match format {
            DXGI_FORMAT_R8G8B8A8_TYPELESS
            | DXGI_FORMAT_R8G8B8A8_UNORM
            | DXGI_FORMAT_R8G8B8A8_UNORM_SRGB
            | DXGI_FORMAT_R8G8B8A8_UINT
            | DXGI_FORMAT_R8G8B8A8_SNORM
            | DXGI_FORMAT_R8G8B8A8_SINT => plain(
                Kind::Unorm8,
                DXGI_FORMAT_R8G8B8A8_TYPELESS,
                DXGI_FORMAT_R8G8B8A8_UNORM,
                4,
                false,
            ),
            DXGI_FORMAT_B8G8R8A8_TYPELESS
            | DXGI_FORMAT_B8G8R8A8_UNORM
            | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB => plain(
                Kind::Unorm8,
                DXGI_FORMAT_B8G8R8A8_TYPELESS,
                DXGI_FORMAT_B8G8R8A8_UNORM,
                4,
                true,
            ),
            DXGI_FORMAT_R10G10B10A2_TYPELESS
            | DXGI_FORMAT_R10G10B10A2_UNORM
            | DXGI_FORMAT_R10G10B10A2_UINT => plain(
                Kind::Uint4,
                DXGI_FORMAT_R10G10B10A2_TYPELESS,
                DXGI_FORMAT_R10G10B10A2_UINT,
                4,
                false,
            ),
            DXGI_FORMAT_R11G11B10_FLOAT => plain(
                Kind::Float3,
                DXGI_FORMAT_R11G11B10_FLOAT,
                DXGI_FORMAT_R11G11B10_FLOAT,
                4,
                false,
            ),
            // FSR's motion vectors: compared as floats, any changed bit, with the third channel the
            // view reads as zero.
            DXGI_FORMAT_R16G16_TYPELESS | DXGI_FORMAT_R16G16_FLOAT => plain(
                Kind::Float3,
                DXGI_FORMAT_R16G16_TYPELESS,
                DXGI_FORMAT_R16G16_FLOAT,
                4,
                false,
            ),
            DXGI_FORMAT_R32G8X24_TYPELESS | DXGI_FORMAT_D32_FLOAT_S8X24_UINT => Self {
                kind: Kind::Depth,
                storage: DXGI_FORMAT_R32G8X24_TYPELESS,
                values: DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS,
                stencil: Some(DXGI_FORMAT_X32_TYPELESS_G8X24_UINT),
                pixel_bytes: 8,
                bgra: false,
            },
            DXGI_FORMAT_R32_TYPELESS | DXGI_FORMAT_D32_FLOAT | DXGI_FORMAT_R32_FLOAT => plain(
                Kind::Depth,
                DXGI_FORMAT_R32_TYPELESS,
                DXGI_FORMAT_R32_FLOAT,
                4,
                false,
            ),
            other => bail!("unsupported format {other:?}"),
        })
    }
}

impl PlaneTexture {
    /// Copy mip 0 of `source` into a new texture the shader can read.
    ///
    /// # Safety
    /// Render thread, with the engine's immediate context locked; `source` is a live texture whose
    /// writes for this frame are queued on `context`.
    pub(crate) unsafe fn copy(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        source: &ID3D11Resource,
        reuse: Option<Self>,
    ) -> anyhow::Result<Self> {
        let texture: ID3D11Texture2D = source.cast().context("the source is not a 2D texture")?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: a live texture.
        unsafe { texture.GetDesc(&mut desc) };
        if desc.SampleDesc.Count != 1 {
            bail!("multisampled sources are not compared");
        }
        let format = Format::of(desc.Format)?;
        let plane = match reuse {
            Some(plane)
                if plane.width == desc.Width
                    && plane.height == desc.Height
                    && plane.format == format =>
            {
                plane
            }
            _ => unsafe { Self::create(device, desc.Width, desc.Height, format)? },
        };
        // SAFETY: both textures are live and of one format family; mip 0 of each is the same size.
        unsafe {
            context.CopySubresourceRegion(&plane.texture, 0, 0, 0, 0, &texture, 0, None);
        }
        Ok(plane)
    }

    unsafe fn create(
        device: &ID3D11Device,
        width: u32,
        height: u32,
        format: Format,
    ) -> anyhow::Result<Self> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: format.storage,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut texture: Option<ID3D11Texture2D> = None;
        // SAFETY: the live device; the descriptors are valid for these calls.
        unsafe {
            device
                .CreateTexture2D(&desc, None, Some(&mut texture))
                .context("creating the snapshot texture")?;
            let texture = texture.context("the snapshot texture was not created")?;
            let values = view(device, &texture, format.values)?;
            let stencil = match format.stencil {
                Some(stencil) => Some(view(device, &texture, stencil)?),
                None => None,
            };
            Ok(Self {
                texture,
                format,
                width,
                height,
                values,
                stencil,
            })
        }
    }
}

unsafe fn view(
    device: &ID3D11Device,
    texture: &ID3D11Texture2D,
    format: DXGI_FORMAT,
) -> anyhow::Result<ID3D11ShaderResourceView> {
    let mut view: Option<ID3D11ShaderResourceView> = None;
    // SAFETY: the live device and texture; the view format is in the texture's family.
    unsafe {
        device
            .CreateShaderResourceView(
                texture,
                Some(&D3D11_SHADER_RESOURCE_VIEW_DESC {
                    Format: format,
                    ViewDimension: D3D11_SRV_DIMENSION_TEXTURE2D,
                    Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                        Texture2D: D3D11_TEX2D_SRV {
                            MostDetailedMip: 0,
                            MipLevels: 1,
                        },
                    },
                }),
                Some(&mut view),
            )
            .with_context(|| format!("creating a {format:?} snapshot view"))?;
    }
    view.context("the snapshot view was not created")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_map_to_their_rules() {
        assert_eq!(
            Format::of(DXGI_FORMAT_R8G8B8A8_UNORM_SRGB).unwrap().kind,
            Kind::Unorm8
        );
        let bgra = Format::of(DXGI_FORMAT_B8G8R8A8_UNORM).unwrap();
        assert!(bgra.bgra);
        assert_eq!(bgra.storage, DXGI_FORMAT_B8G8R8A8_TYPELESS);
        assert_eq!(
            Format::of(DXGI_FORMAT_R10G10B10A2_TYPELESS).unwrap().values,
            DXGI_FORMAT_R10G10B10A2_UINT
        );
        let depth = Format::of(DXGI_FORMAT_R32G8X24_TYPELESS).unwrap();
        assert_eq!(depth.kind, Kind::Depth);
        assert_eq!(depth.stencil, Some(DXGI_FORMAT_X32_TYPELESS_G8X24_UINT));
        assert_eq!(depth.pixel_bytes, 8);
        assert!(Format::of(DXGI_FORMAT_R32_FLOAT).unwrap().stencil.is_none());
        let motion = Format::of(DXGI_FORMAT_R16G16_FLOAT).unwrap();
        assert_eq!(motion.kind, Kind::Float3);
        assert_eq!(motion.storage, DXGI_FORMAT_R16G16_TYPELESS);
        assert!(Format::of(DXGI_FORMAT(2)).is_err());
    }
}
