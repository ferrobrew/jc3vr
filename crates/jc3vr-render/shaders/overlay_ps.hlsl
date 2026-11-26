// World-space overlay pixel shader: shades the discs, rings, and lines of overlay_vs, and tests them
// against the scene depth by hand.
//
// The scene depth is reverse-Z with a finite far plane, so a device depth `z` is at view distance
// `B / (z + A)` for the projection's z row `(A, B)`. A fragment farther than the scene there, by more
// than a tolerance in metres, is occluded: its alpha is scaled by the primitive's occluded alpha,
// which is zero for a depth-tested primitive, a fraction for an x-ray one, and one for a primitive
// drawn over everything. The tolerance lets a marker on a surface sit exactly on it.

cbuffer Eye : register(b0)
{
    row_major float4x4 OffsetViewProjection;
    float4 OriginToEye;
    float2 ViewportSize;
    float PixelScale;
    float MinLinePixels;
    float2 DepthScale;
    float2 DepthZRow;
    float2 DepthMax;
    float DepthToleranceAbs;
    float DepthToleranceRel;
};

Texture2D<float> SceneDepth : register(t1);

struct VSOut
{
    float4 position : SV_Position;
    float4 color : COLOR0;
    float2 uv : TEXCOORD0;
    noperspective float2 line_uv : TEXCOORD1;
    float view_dist : TEXCOORD2;
    nointerpolation float4 params : TEXCOORD3;
    noperspective float half_px : TEXCOORD4;
};

float4 main(VSOut i) : SV_Target
{
    // Derivatives before any divergent branch.
    float r = length(i.uv);
    float aa = max(fwidth(r), 1e-5);

    float alpha = i.color.a;
    if (i.params.x < 0.5)
    {
        alpha *= saturate((1.0 - r) / aa + 0.5);
        if (i.params.y > 0.0)
        {
            alpha *= saturate((r - i.params.y) / aa + 0.5);
        }
    }
    else
    {
        float edge = (1.0 - abs(i.line_uv.y)) * i.half_px;
        alpha *= saturate(edge + 0.5) * i.params.w;
    }

    if (i.params.z < 1.0)
    {
        uint2 texel = (uint2)min(i.position.xy * DepthScale, DepthMax);
        float device_depth = SceneDepth.Load(int3(texel, 0));
        float scene_dist = DepthZRow.y / (device_depth + DepthZRow.x);
        float tolerance = max(DepthToleranceAbs, DepthToleranceRel * i.view_dist);
        if (i.view_dist > scene_dist + tolerance)
        {
            alpha *= i.params.z;
        }
    }

    if (alpha <= 1.0 / 512.0)
    {
        discard;
    }
    return float4(i.color.rgb, alpha);
}
