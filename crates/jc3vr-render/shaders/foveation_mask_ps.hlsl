// Foveation mask pixel shader (issue #29): tags a dithered fraction of the peripheral pixels in the
// stencil buffer so the expensive scene passes discard them. Runs full-screen (capture_vs) against the
// main depth-stencil surface with a stencil-write depth-stencil state (StencilOp REPLACE, ref = the mask
// bit); it writes no colour. A fragment that survives (is not discarded) here has the mask bit written to
// its stencil, marking it for the per-pass stencil test to drop; a discarded fragment keeps full
// resolution (its stencil bit stays clear).
//
// The drop is decided per cell of g_CellPx by g_CellPx pixels. With 2 by 2 cells aligned to the
// rasterizer's pixel quads, a dropped cell is a whole quad with no live lane, so a masked pass runs no
// pixel shader there at all; with single-pixel cells nearly every quad keeps a live pixel, and the
// shader still runs for the whole quad.
//
// A target can hold both eyes side by side (the single-pass collapse, left half = eye 0): a pixel at or
// beyond g_SplitPx belongs to eye 1 and measures its radius and dither from the left edge of eye 1's half,
// so each half is masked exactly as an eye-sized target would be.

cbuffer FoveationParams : register(b0)
{
    float4 g_CenterPx;   // foveal centre per eye, in eye-local pixels: xy eye 0 (or the only eye), zw eye 1
    float  g_InnerPx;    // radius (px): inside = full resolution, no mask bit
    float  g_OuterPx;    // radius (px): the drop ramp reaches its maximum here
    float  g_MaxDrop;    // maximum fraction of peripheral pixels dropped (0..1)
    float  g_DebugMode;  // unused here; shared layout with the fill-in pass
    float  g_SplitPx;    // first column of eye 1's half; beyond the target when it holds one eye
    float  g_CellPx;     // the side of a drop cell in pixels: 1 or 2
    float2 _pad;
};

// Interleaved gradient noise (Jimenez 2014): a stable, screen-space dither in [0, 1).
float ign(float2 p)
{
    return frac(52.9829189 * frac(dot(p, float2(0.06711056, 0.00583715))));
}

// Whether the cell holding the eye-local pixel centre `px` is dropped. Must match foveation_fill_ps.hlsl
// exactly. With single-pixel cells the cell centre is the pixel centre itself.
bool is_dropped(float2 px, float2 centre)
{
    float2 cell = floor(px / g_CellPx);
    float d = distance((cell + 0.5) * g_CellPx, centre);
    if (d <= g_InnerPx)
        return false;
    float ramp = saturate((d - g_InnerPx) / max(g_OuterPx - g_InnerPx, 1.0));
    return ign(cell + 0.5) < ramp * g_MaxDrop;
}

void main(float4 pos : SV_Position)
{
    bool second = pos.x >= g_SplitPx;
    float2 local = pos.xy - float2(second ? g_SplitPx : 0.0, 0.0);
    float2 centre = second ? g_CenterPx.zw : g_CenterPx.xy;
    if (!is_dropped(local, centre))
        discard;                                            // kept full resolution -> no mask bit
    // reaching here: this pixel is dropped -> the output-merger writes the mask bit (StencilOp REPLACE).
}
