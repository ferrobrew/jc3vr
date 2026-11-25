// The capture harness's on-GPU comparison of two snapshot planes (an eye of the final image, the
// depth, a G-buffer, or the velocity buffer of two renders of one frozen frame).
//
// One thread group per cell of a Grid.x by Grid.y heat map over the compared region. Each group
// walks its cell, classifies every pixel as differing or not by the plane's rule, and reduces the
// cell to eight words: the differing pixel count, the sum of the per-pixel mean absolute channel
// difference, the largest channel difference, the bounding box of the differing pixels, and the
// count of pixels whose stored values change at all, under the rule or not. The CPU reads the cells
// back and totals them; nothing larger than the cells ever leaves the GPU.
//
// The rules mirror the Python comparison (`tools/rendering/comparison/images.py` for the final
// image, `tools/rendering/comparison/stages.py` for the stages):
// - KIND_UNORM8, the final image and the 8-bit stages: channel codes (value times 255, rounded); a
//   pixel differs when its largest channel difference exceeds Threshold (three codes for the final
//   image, zero for a stage, so any changed code).
// - KIND_UINT4, the 10-bit stages read as integers: a pixel differs when any channel exceeds
//   Threshold (zero, so any changed bit).
// - KIND_FLOAT3, the R11G11B10 float stages: a pixel differs when any channel's bits change.
// - KIND_DEPTH, the depth buffer: a pixel differs when the two depths are more than Threshold
//   representable floats apart (zero, so any changed bit, unless the request sets a tolerance), or
//   its stencil changes when the plane carries one.

#define KIND_UNORM8 0
#define KIND_UINT4 1
#define KIND_FLOAT3 2
#define KIND_DEPTH 3

#define GROUP_SIDE 16
#define GROUP_THREADS (GROUP_SIDE * GROUP_SIDE)
#define CELL_WORDS 8

cbuffer Params : register(b0)
{
    // The top-left pixel of the compared region in each plane's texture.
    uint2 OriginA;
    uint2 OriginB;
    // The compared region's size in pixels.
    uint2 Size;
    uint Kind;
    float Threshold;
    // The heat map's cells across and down.
    uint2 Grid;
    // The first cell this dispatch writes in Cells, in cells.
    uint CellBase;
    // KIND_UNORM8: how many channels count (three for the final image, four for a stage).
    uint Channels;
    // KIND_DEPTH: whether StencilA and StencilB hold the planes' stencil.
    uint HasStencil;
    uint3 _pad;
};

Texture2D<float4> ValueA : register(t0);
Texture2D<float4> ValueB : register(t1);
Texture2D<uint4> UintA : register(t2);
Texture2D<uint4> UintB : register(t3);
Texture2D<uint2> StencilA : register(t4);
Texture2D<uint2> StencilB : register(t5);

// CELL_WORDS words per cell: count, sum (float bits), max (float bits), min x, min y, max x,
// max y, and the count of pixels with any change.
RWBuffer<uint> Cells : register(u0);

groupshared uint g_count;
groupshared uint g_changed;
groupshared uint g_min_x;
groupshared uint g_min_y;
groupshared uint g_max_x;
groupshared uint g_max_y;
groupshared float g_sum[GROUP_THREADS];
groupshared float g_max[GROUP_THREADS];

// The distance between two depths in representable floats, saturating when their signs differ
// (a depth of either zero matches the other zero).
uint depth_ulps(float a, float b)
{
    const int ia = asint(a);
    const int ib = asint(b);
    if ((ia ^ ib) < 0)
    {
        return ((ia | ib) & 0x7fffffff) == 0 ? 0 : 0xffffffff;
    }
    return (uint)abs(ia - ib);
}

// The pixel's mean absolute channel difference (x), its largest channel difference (y), whether
// it differs by the plane's rule (z, as 0 or 1), and whether its stored values change at all (w,
// as 0 or 1).
float4 compare_pixel(uint2 a, uint2 b)
{
    if (Kind == KIND_UNORM8)
    {
        const float4 ca = round(saturate(ValueA[a]) * 255.0);
        const float4 cb = round(saturate(ValueB[b]) * 255.0);
        const float4 d = abs(ca - cb);
        float largest = max(max(d.r, d.g), d.b);
        float sum = d.r + d.g + d.b;
        if (Channels == 4)
        {
            largest = max(largest, d.a);
            sum += d.a;
        }
        return float4(sum / (float)Channels, largest, largest > Threshold ? 1.0 : 0.0,
            largest > 0.0 ? 1.0 : 0.0);
    }
    if (Kind == KIND_UINT4)
    {
        const int4 d = abs((int4)UintA[a] - (int4)UintB[b]);
        const float4 f = (float4)d;
        const float largest = max(max(f.r, f.g), max(f.b, f.a));
        return float4((f.r + f.g + f.b + f.a) / 4.0, largest, largest > Threshold ? 1.0 : 0.0,
            largest > 0.0 ? 1.0 : 0.0);
    }
    if (Kind == KIND_FLOAT3)
    {
        const float3 va = ValueA[a].rgb;
        const float3 vb = ValueB[b].rgb;
        const bool differs = any(asuint(va) != asuint(vb));
        float3 d = abs(va - vb);
        // A non-finite difference still counts the pixel, but stays out of the sums.
        d = (d == d && d < 3.0e38) ? d : 0.0;
        return float4((d.r + d.g + d.b) / 3.0, max(max(d.r, d.g), d.b), differs ? 1.0 : 0.0,
            differs ? 1.0 : 0.0);
    }
    const float da = ValueA[a].r;
    const float db = ValueB[b].r;
    const bool stencil_differs = HasStencil != 0 && StencilA[a].g != StencilB[b].g;
    const bool differs = (float)depth_ulps(da, db) > Threshold || stencil_differs;
    const bool changed = asuint(da) != asuint(db) || stencil_differs;
    float d = abs(da - db);
    d = (d == d && d < 3.0e38) ? d : 0.0;
    return float4(d, d, differs ? 1.0 : 0.0, changed ? 1.0 : 0.0);
}

[numthreads(GROUP_SIDE, GROUP_SIDE, 1)]
void main(uint3 group : SV_GroupID, uint3 thread : SV_GroupThreadID, uint index : SV_GroupIndex)
{
    if (index == 0)
    {
        g_count = 0;
        g_changed = 0;
        g_min_x = 0xffffffff;
        g_min_y = 0xffffffff;
        g_max_x = 0;
        g_max_y = 0;
    }
    GroupMemoryBarrierWithGroupSync();

    const uint2 lo = group.xy * Size / Grid;
    const uint2 hi = (group.xy + 1) * Size / Grid;
    uint count = 0;
    uint changed = 0;
    float sum = 0.0;
    float largest = 0.0;
    uint2 box_lo = uint2(0xffffffff, 0xffffffff);
    uint2 box_hi = uint2(0, 0);
    for (uint y = lo.y + thread.y; y < hi.y; y += GROUP_SIDE)
    {
        for (uint x = lo.x + thread.x; x < hi.x; x += GROUP_SIDE)
        {
            const uint2 pixel = uint2(x, y);
            const float4 result = compare_pixel(OriginA + pixel, OriginB + pixel);
            sum += result.x;
            changed += result.w != 0.0 ? 1 : 0;
            largest = max(largest, result.y);
            if (result.z != 0.0)
            {
                count += 1;
                box_lo = min(box_lo, pixel);
                box_hi = max(box_hi, pixel);
            }
        }
    }
    g_sum[index] = sum;
    g_max[index] = largest;
    if (changed != 0)
    {
        InterlockedAdd(g_changed, changed);
    }
    if (count != 0)
    {
        InterlockedAdd(g_count, count);
        InterlockedMin(g_min_x, box_lo.x);
        InterlockedMin(g_min_y, box_lo.y);
        InterlockedMax(g_max_x, box_hi.x);
        InterlockedMax(g_max_y, box_hi.y);
    }
    GroupMemoryBarrierWithGroupSync();

    [unroll]
    for (uint stride = GROUP_THREADS / 2; stride > 0; stride >>= 1)
    {
        if (index < stride)
        {
            g_sum[index] += g_sum[index + stride];
            g_max[index] = max(g_max[index], g_max[index + stride]);
        }
        GroupMemoryBarrierWithGroupSync();
    }

    if (index == 0)
    {
        const uint base = (CellBase + group.y * Grid.x + group.x) * CELL_WORDS;
        Cells[base + 0] = g_count;
        Cells[base + 1] = asuint(g_sum[0]);
        Cells[base + 2] = asuint(g_max[0]);
        Cells[base + 3] = g_min_x;
        Cells[base + 4] = g_min_y;
        Cells[base + 5] = g_max_x;
        Cells[base + 6] = g_max_y;
        Cells[base + 7] = g_changed;
    }
}
