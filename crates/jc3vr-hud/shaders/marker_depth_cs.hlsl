// Scene depth at the world markers' pixels, for placing each marker on the nearest visible surface
// along its line of sight.
//
// One thread per point: read the main depth buffer at the point's pixel and at four neighbours
// Spread pixels away, linearize each through the projection's z-row (works for standard and
// reversed-Z alike, as the depth histogram does), and keep the farthest. Taking the farthest makes
// a thin occluder (a pole, a leaf) count only when it covers the whole footprint, so the placement
// does not flicker on single pixels. Depth outside the buffer or at the clear value counts as far.
//
// The consumer reads the results back asynchronously.

#define MAX_POINTS 32
#define FAR_METERS 100000.0

cbuffer Params : register(b0)
{
    // Depth texture dimensions in pixels.
    float2 DepthDims;
    // Projection z-row terms: device depth d = ProjA + ProjB / view_z.
    float ProjA;
    float ProjB;
    uint Count;
    float Spread;
    float2 _pad;
    // xy: the point's pixel in the depth texture.
    float4 Points[MAX_POINTS];
}

Texture2D<float> Depth : register(t0);
RWBuffer<float> Result : register(u0);

float view_depth(float2 pixel)
{
    if (any(pixel < 0.0) || any(pixel >= DepthDims))
    {
        return FAR_METERS;
    }
    const float device_depth = Depth[(uint2) pixel];
    const float view_z = ProjB / (device_depth - ProjA);
    return view_z > 0.0 ? min(view_z, FAR_METERS) : FAR_METERS;
}

[numthreads(MAX_POINTS, 1, 1)]
void main(uint3 id : SV_DispatchThreadID)
{
    if (id.x >= Count)
    {
        return;
    }
    const float2 pixel = Points[id.x].xy;
    float farthest = view_depth(pixel);
    farthest = max(farthest, view_depth(pixel + float2(Spread, 0.0)));
    farthest = max(farthest, view_depth(pixel - float2(Spread, 0.0)));
    farthest = max(farthest, view_depth(pixel + float2(0.0, Spread)));
    farthest = max(farthest, view_depth(pixel - float2(0.0, Spread)));
    Result[id.x] = farthest;
}
