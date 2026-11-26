// HUD quad pixel shader with an opacity: samples a premultiplied HUD-style texture and scales all
// four channels by the draw's opacity, so a faded quad stays premultiplied. Shares hud_quad_vs and
// its constant buffer, which carries the opacity after the corners.

cbuffer Quad : register(b0)
{
    row_major float4x4 ViewProjection;
    float4 Corners[4];
    float4 Tint; // .a = opacity
};

Texture2D HudTexture : register(t0);
SamplerState HudSampler : register(s0);

float4 main(float4 position : SV_Position, float2 uv : TEXCOORD0) : SV_Target
{
    return HudTexture.Sample(HudSampler, uv) * Tint.a;
}
