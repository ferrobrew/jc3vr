// Recording pack pixel shader: draws both eyes' display-referred RGBA captures into one frame, each
// scaled into its own rectangle, and converts the frame to planar 4:2:0 YUV (I420) packed into a
// single R8 render target, so one staging readback of that target yields the exact byte layout a
// `yuv4mpegpipe` frame wants.
//
// The target is `width` x `height * 3 / 2` pixels, `width` and `height` both even. Rows
// `[0, height)` are the full-resolution luma plane. The remaining `height / 2` rows hold the two
// quarter-size chroma planes back to back, each `width / 2` x `height / 2` samples laid out
// linearly: U first, then V. A row of the packed region therefore holds two chroma rows of one
// plane, and the plane boundary falls at row `height + height / 4`.
//
// Each eye's rectangle is given in luma pixels, with an even origin and an even size, so a 2x2
// chroma block never straddles an eye's edge. A pixel outside both rectangles is black.
//
// Luma samples the pixel's footprint and chroma the 2x2 block's (the "centred" 4:2:0 siting,
// `C420jpeg` in y4m terms). A footprint is four bilinear taps at its quarter points, so a downscale
// of up to about two to one averages every source texel under the footprint instead of skipping
// some. The conversion is BT.709 with limited (video) range, the convention every consumer assumes
// for HD material.

cbuffer PackParams : register(b0)
{
    // The output luma plane size (`width`, `height`), both even.
    uint2 size;
    uint2 padding;
    // Each eye's rectangle in the frame: `x, y, width, height` in luma pixels.
    float4 eye_rects[2];
};

Texture2D LeftEye : register(t0);
Texture2D RightEye : register(t1);
SamplerState EyeSampler : register(s0);

float luma(float3 rgb)
{
    return 16.0 / 255.0 + (219.0 / 255.0) * dot(rgb, float3(0.2126, 0.7152, 0.0722));
}

float chroma_u(float3 rgb)
{
    return 128.0 / 255.0 + (224.0 / 255.0) * dot(rgb, float3(-0.1146, -0.3854, 0.5));
}

float chroma_v(float3 rgb)
{
    return 128.0 / 255.0 + (224.0 / 255.0) * dot(rgb, float3(0.5, -0.4542, -0.0458));
}

bool inside(float2 p, float4 rect)
{
    return all(p >= rect.xy) && all(p < rect.xy + rect.zw);
}

float3 footprint(Texture2D eye, float2 uv, float2 quarter)
{
    float3 sum = eye.Sample(EyeSampler, uv + float2(-quarter.x, -quarter.y)).rgb;
    sum += eye.Sample(EyeSampler, uv + float2(quarter.x, -quarter.y)).rgb;
    sum += eye.Sample(EyeSampler, uv + float2(-quarter.x, quarter.y)).rgb;
    sum += eye.Sample(EyeSampler, uv + float2(quarter.x, quarter.y)).rgb;
    return sum * 0.25;
}

// The frame's colour around `p` (in luma pixels), averaged over `extent` luma pixels.
float3 frame_colour(float2 p, float extent)
{
    if (inside(p, eye_rects[0]))
    {
        float4 rect = eye_rects[0];
        return footprint(LeftEye, (p - rect.xy) / rect.zw, 0.25 * extent / rect.zw);
    }
    if (inside(p, eye_rects[1]))
    {
        float4 rect = eye_rects[1];
        return footprint(RightEye, (p - rect.xy) / rect.zw, 0.25 * extent / rect.zw);
    }
    return float3(0.0, 0.0, 0.0);
}

float main(float4 position : SV_Position, float2 uv : TEXCOORD0) : SV_Target
{
    uint x = (uint)position.x;
    uint y = (uint)position.y;
    if (y < size.y)
    {
        return luma(frame_colour(float2(x, y) + 0.5, 1.0));
    }
    uint half_width = size.x / 2;
    uint plane_samples = half_width * (size.y / 2);
    uint index = (y - size.y) * size.x + x;
    bool is_v = index >= plane_samples;
    uint sample_index = is_v ? index - plane_samples : index;
    uint cx = sample_index % half_width;
    uint cy = sample_index / half_width;
    float3 rgb = frame_colour(float2(cx, cy) * 2.0 + 1.0, 2.0);
    return is_v ? chroma_v(rgb) : chroma_u(rgb);
}
