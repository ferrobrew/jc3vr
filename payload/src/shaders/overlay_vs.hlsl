// World-space overlay vertex shader: expands each primitive of a structured buffer into a quad, one
// instance per primitive, drawn as a four-vertex triangle strip.
//
// Positions arrive relative to a per-frame origin and are moved to be relative to this eye before
// projecting through a translation-free view-projection, so every magnitude stays small and the
// projection keeps its precision at the game's world coordinates. `row_major` matches the engine's
// matrix layout, so `mul(v, M)` is `v * M`.
//
// A sprite is a square spanned by its two axes. A line is a screen-aligned ribbon: its ends are
// clipped against a plane just in front of the eye, then offset sideways in pixels by the projected
// half-width, which never falls below half the minimum pixel width. A line thinner than that minimum
// passes its true fraction of it on as coverage, so a distant line fades rather than aliasing.

struct Primitive
{
    float4 a;     // xyz: sprite centre or line start; w: sprite radius or line half-width (m)
    float4 b;     // xyz: line end; w: ring inner radius as a fraction of the radius
    float4 u;     // xyz: sprite right axis; w: alpha scale of occluded pixels
    float4 v;     // xyz: sprite up axis
    float4 color; // straight alpha
    uint4 kind;   // x: 0 sprite, 1 line
};

StructuredBuffer<Primitive> Prims : register(t0);

cbuffer Eye : register(b0)
{
    row_major float4x4 OffsetViewProjection;
    float4 OriginToEye;       // xyz: the frame origin relative to this eye
    float2 ViewportSize;      // pixels
    float PixelScale;         // pixels per metre at unit distance, horizontally
    float MinLinePixels;
    float2 DepthScale;        // depth texels per target pixel
    float2 DepthZRow;         // the projection's z row (A, B)
    float2 DepthMax;          // the last depth texel
    float DepthToleranceAbs;  // metres
    float DepthToleranceRel;  // fraction of the view distance
};

struct VSOut
{
    float4 position : SV_Position;
    float4 color : COLOR0;
    float2 uv : TEXCOORD0;                     // sprite: -1..1 across the square
    noperspective float2 line_uv : TEXCOORD1;  // line: y is -1..1 across the ribbon's true width
    float view_dist : TEXCOORD2;
    nointerpolation float4 params : TEXCOORD3; // x: kind, y: ring inner radius, z: occluded alpha, w: coverage
    noperspective float half_px : TEXCOORD4;   // line: half-width in pixels
};

static const float NearW = 0.01;

static const float2 Corners[4] =
{
    float2(-1.0, 1.0),
    float2(1.0, 1.0),
    float2(-1.0, -1.0),
    float2(1.0, -1.0),
};

VSOut main(uint vertex_id : SV_VertexID, uint instance_id : SV_InstanceID)
{
    Primitive p = Prims[instance_id];
    float2 corner = Corners[vertex_id];

    VSOut o;
    o.color = p.color;
    o.uv = corner;
    o.line_uv = float2(0.0, 0.0);
    o.half_px = 0.0;

    if (p.kind.x == 0)
    {
        float3 pos = p.a.xyz + OriginToEye.xyz + (p.u.xyz * corner.x + p.v.xyz * corner.y) * p.a.w;
        o.position = mul(float4(pos, 1.0), OffsetViewProjection);
        o.view_dist = o.position.w;
        o.params = float4(0.0, p.b.w, p.u.w, 1.0);
        return o;
    }

    float4 ca = mul(float4(p.a.xyz + OriginToEye.xyz, 1.0), OffsetViewProjection);
    float4 cb = mul(float4(p.b.xyz + OriginToEye.xyz, 1.0), OffsetViewProjection);
    o.params = float4(1.0, 0.0, p.u.w, 1.0);
    if (ca.w < NearW && cb.w < NearW)
    {
        // Entirely behind the eye: a degenerate position the rasterizer discards.
        o.position = float4(0.0, 0.0, 0.0, 0.0);
        o.view_dist = 0.0;
        return o;
    }
    if (ca.w < NearW)
    {
        ca = lerp(ca, cb, (NearW - ca.w) / (cb.w - ca.w));
    }
    else if (cb.w < NearW)
    {
        cb = lerp(cb, ca, (NearW - cb.w) / (ca.w - cb.w));
    }

    float2 half_viewport = ViewportSize * 0.5;
    float2 sa = ca.xy / ca.w * half_viewport;
    float2 sb = cb.xy / cb.w * half_viewport;
    float2 d = sb - sa;
    float len = length(d);
    float2 dir = len > 1e-4 ? d / len : float2(1.0, 0.0);
    float2 normal = float2(-dir.y, dir.x);

    bool at_end = vertex_id >= 2;
    float along = at_end ? 1.0 : -1.0;
    float side = corner.x;
    float4 c = at_end ? cb : ca;

    float raw_px = p.a.w * PixelScale / c.w;
    float half_px = max(raw_px, MinLinePixels * 0.5);
    // One pixel beyond the true edge on each side, so the edge's antialiasing has room.
    float extent = half_px + 1.0;
    float2 offset_px = normal * side * extent + dir * along * half_px;
    c.xy += offset_px / half_viewport * c.w;

    o.position = c;
    o.view_dist = c.w;
    o.line_uv = float2(along, side * extent / half_px);
    o.half_px = half_px;
    o.params.w = saturate(raw_px / half_px);
    return o;
}
