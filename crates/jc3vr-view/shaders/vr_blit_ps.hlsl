// VR eye-blit pixel shader: samples the game's captured eye back buffer and writes it into one
// array slice of the OpenXR stereo swapchain. Paired with the fullscreen-triangle vertex shader
// (capture_vs), which the blit reuses.
//
// The captured eye texture is a copy of the game's display-referred (sRGB-encoded) back buffer,
// stored as R8G8B8A8_UNORM. The OpenXR swapchain is negotiated as _SRGB, so writing through its
// render-target view applies a hardware linear->sRGB encode. To reproduce the original bytes the
// shader linearizes the sampled colour first (g_linearize != 0), so the hardware re-encode cancels
// it out. g_linearize == 0 passes the colour through unchanged (for a genuine-linear source or a
// non-sRGB target). See vr::config::BlitGamma for the reasoning and the runtime toggle.
//
// g_brightness scales the colour for the comfort fade (1 leaves the image as it is, 0 is black).
//
// g_source maps the fullscreen UV's u onto the part of the source the slice shows: u * x + y,
// clamped to [z, w]. The whole source is (1, 0, 0, 1); one eye's half of the single-pass double-wide
// back buffer is scaled by a half, offset to the half, and clamped to its outermost texel centres.

cbuffer BlitParams : register(b0)
{
    uint g_linearize;
    float g_brightness;
    uint2 _pad;
    float4 g_source;
};

Texture2D EyeTexture : register(t0);
SamplerState EyeSampler : register(s0);

float3 srgb_to_linear(float3 c)
{
    return c <= 0.04045 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4);
}

float4 main(float4 position : SV_Position, float2 uv : TEXCOORD0) : SV_Target
{
    float2 source_uv = float2(clamp(uv.x * g_source.x + g_source.y, g_source.z, g_source.w), uv.y);
    float4 colour = EyeTexture.Sample(EyeSampler, source_uv);
    if (g_linearize != 0)
    {
        colour.rgb = srgb_to_linear(colour.rgb);
    }
    colour.rgb *= g_brightness;
    return colour;
}
