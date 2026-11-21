// Foveation fill-in pixel shader (issue #29): reconstructs the peripheral pixels the mask pass dropped.
// Runs full-screen (capture_vs) with the main colour buffer as the render target and a copy of it bound as
// the source; it re-derives, per pixel, whether the mask pass dropped it (identical radial + dither logic),
// passes through the kept pixels unchanged, and reconstructs each dropped pixel from its kept neighbours:
// the average of the kept pixels around a dropped single pixel, or, for a dropped cell, the nearest pixel
// of each kept cell around it, weighted by inverse squared distance. Far in the periphery, where every
// neighbour was dropped, it falls back to the source value. Under the stencil test the fill-in draws
// only the dropped pixels; without it the kept ones pass through.
//
// A target can hold both eyes side by side (the single-pass collapse, left half = eye 0): a pixel at or
// beyond g_SplitPx belongs to eye 1 and works in pixels measured from the left edge of eye 1's half. A
// neighbour across the seam reads as zero, as one past the edge of an eye-sized target does.

Texture2D SceneColor : register(t0);

cbuffer FoveationParams : register(b0)
{
    float4 g_CenterPx;   // foveal centre per eye, in eye-local pixels: xy eye 0 (or the only eye), zw eye 1
    float  g_InnerPx;    // radius (px): inside = full resolution, no mask bit
    float  g_OuterPx;    // radius (px): the drop ramp reaches its maximum here
    float  g_MaxDrop;    // maximum fraction of peripheral pixels dropped (0..1)
    float  g_DebugMode;  // >0.5: tint dropped pixels magenta instead of reconstructing them
    float  g_SplitPx;    // first column of eye 1's half; beyond the target when it holds one eye
    float  g_CellPx;     // the side of a drop cell in pixels: 1 or 2
    float2 _pad;
};

// Interleaved gradient noise (Jimenez 2014): must match foveation_mask_ps.hlsl exactly.
float ign(float2 p)
{
    return frac(52.9829189 * frac(dot(p, float2(0.06711056, 0.00583715))));
}

// Reproduces the mask pass's drop decision for a cell (in cell coordinates of the eye's half).
bool cell_dropped(float2 cell, float2 centre)
{
    float d = distance((cell + 0.5) * g_CellPx, centre);
    if (d <= g_InnerPx)
        return false;
    float ramp = saturate((d - g_InnerPx) / max(g_OuterPx - g_InnerPx, 1.0));
    return ign(cell + 0.5) < ramp * g_MaxDrop;
}

// Reproduces the mask pass's drop decision for the cell holding the eye-local pixel centre `px`.
bool is_dropped(float2 px, float2 centre)
{
    return cell_dropped(floor(px / g_CellPx), centre);
}

// Accumulates the neighbour at `offset` pixels from the pixel `p` with weight `w`; a neighbour across the
// seam reads as zero.
void accumulate(int2 p, int2 offset, bool second, float w, inout float3 sum, inout float weight)
{
    int2 n = p + offset;
    bool same_eye = ((float(n.x) + 0.5) >= g_SplitPx) == second;
    sum += same_eye ? w * SceneColor.Load(int3(n, 0)).rgb : 0.0;
    weight += w;
}

float4 main(float4 pos : SV_Position, float2 uv : TEXCOORD0) : SV_Target
{
    int2 p = int2(pos.xy);
    bool second = pos.x >= g_SplitPx;
    float2 origin = float2(second ? g_SplitPx : 0.0, 0.0);
    float2 centre = second ? g_CenterPx.zw : g_CenterPx.xy;
    float2 local = pos.xy - origin;
    if (!is_dropped(local, centre))
        return SceneColor.Load(int3(p, 0));                 // kept full resolution: pass through

    // Diagnostic: paint the dropped set magenta so the mask is directly visible (no reconstruction).
    if (g_DebugMode > 0.5)
        return float4(1.0, 0.0, 1.0, 1.0);

    float3 sum = 0.0;
    float weight = 0.0;
    if (g_CellPx < 1.5)
    {
        // Single dropped pixels: the equal-weight average of the kept pixels around it.
        [unroll] for (int dy = -1; dy <= 1; dy++)
        {
            [unroll] for (int dx = -1; dx <= 1; dx++)
            {
                if ((dx != 0 || dy != 0) && !is_dropped(local + float2(dx, dy), centre))
                    accumulate(p, int2(dx, dy), second, 1.0, sum, weight);
            }
        }
    }
    else
    {
        // Dropped cells: from each kept cell around this one, its pixel nearest to this pixel, weighted
        // by the inverse squared distance.
        float2 cell = floor(local / g_CellPx);
        [unroll] for (int cy = -1; cy <= 1; cy++)
        {
            [unroll] for (int cx = -1; cx <= 1; cx++)
            {
                float2 neighbour = cell + float2(cx, cy);
                if ((cx != 0 || cy != 0) && !cell_dropped(neighbour, centre))
                {
                    float2 lo = neighbour * g_CellPx + 0.5;
                    float2 nearest = clamp(local, lo, lo + g_CellPx - 1.0);
                    float2 offset = nearest - local;
                    accumulate(p, int2(offset), second, 1.0 / dot(offset, offset), sum, weight);
                }
            }
        }
    }
    if (weight > 0.0)
        return float4(sum / weight, 1.0);
    return SceneColor.Load(int3(p, 0));                     // deep periphery: no kept neighbour to sample
}
