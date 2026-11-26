// Scope lens pixel shader: the magnified view around the sight line, resampled from an eye's image.
//
// A point of the lens at `q` (the unit disc, +x right, +y up) shows the scene at a tangent of
// `q * Tangent` off the sight line, which is the tangent the eye sees that point at divided by the
// zoom. The scene point is taken at the focus distance along that direction, relative to the source
// eye, and projected into the source eye's image. Outside that image, or behind the source eye, the
// lens is black. The rim darkens from `VignetteStart` of the radius, and a thin crosshair marks the
// sight line.

cbuffer Lens : register(b0)
{
    row_major float4x4 ViewProjection;
    row_major float4x4 SourceViewProjection;
    float3 EyepieceFromEye;
    float Radius;
    float3 Right;
    float Tangent;
    float3 Up;
    float Focus;
    float3 Direction;
    float VignetteStart;
    float3 EyepieceFromSource;
    float Reticle;
};

Texture2D<float4> Source : register(t0);
SamplerState Linear : register(s0);

struct VSOut
{
    float4 position : SV_Position;
    float2 q : TEXCOORD0;
};

static const float RETICLE_WIDTH = 0.012;

float4 main(VSOut i) : SV_Target
{
    // Every derivative is taken before the discard, where all pixels of a quad are still running.
    float r = length(i.q);
    float edge = max(fwidth(r), 1e-4);
    float2 aa = max(fwidth(i.q), 1e-4);
    float coverage = saturate((1.0 - r) / edge);
    if (coverage <= 0.0)
    {
        discard;
    }

    float3 direction = normalize(Direction + (Right * i.q.x + Up * i.q.y) * Tangent);
    float3 p = EyepieceFromSource + direction * Focus;
    float4 clip = mul(float4(p, 1.0), SourceViewProjection);
    float3 color = float3(0.0, 0.0, 0.0);
    if (clip.w > 1e-4)
    {
        float2 ndc = clip.xy / clip.w;
        float2 uv = float2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        if (all(uv >= 0.0) && all(uv <= 1.0))
        {
            color = Source.SampleLevel(Linear, uv, 0.0).rgb;
        }
    }

    color *= 1.0 - smoothstep(VignetteStart, 1.0, r);

    if (Reticle > 0.5)
    {
        float2 d = abs(i.q);
        float2 bars = 1.0 - saturate((d - RETICLE_WIDTH) / aa);
        color *= 1.0 - max(bars.x, bars.y);
    }

    return float4(color, coverage);
}
