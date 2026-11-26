// The mod's own temporal anti-aliasing resolve: one compute pass over every eye a chain texture
// holds (one eye, or both side by side under the single-pass collapse), in place of FSR at the
// engine's anti-aliasing stage. See docs/mod/rendering/taa.md.
//
// Per pixel:
//   1. The 3x3 neighbourhood of the current (jittered, post-tonemap) colour, clamped to the pixel's
//      eye: its mean and variance in YCoCg for the clip box, its closest and farthest depth, and a
//      reconstruction of the colour at the pixel's unjittered centre. A neighbour at offset d holds
//      the scene at d - Jitter from that centre (the jitter moves the content by Jitter pixels), so
//      it is weighted by exp(-CurrentSharpness * |d - Jitter|^2).
//   2. The motion of the closest-depth pixel of the neighbourhood (velocity dilation), decoded from
//      the engine's velocity buffer exactly as velocity_decode.hlsl decodes it for FSR (the same
//      stereo correction and hybrid camera motion), so a thin foreground edge carries its own motion
//      over the background around it.
//   3. The history at the pixel's previous position through a five-tap Catmull-Rom filter, every
//      tap clamped half a texel inside the eye's window so no tap reaches across the eye seam.
//   4. The history rejected where it cannot belong to the pixel: outside the eye in the previous
//      frame, or (for a pixel whose motion is the camera's) a previous depth outside the range the
//      neighbourhood's nearest and farthest depths reproject to; otherwise clipped toward the
//      variance box (mean +/- gamma sigma, YCoCg) of the current neighbourhood.
//   5. An exponential blend toward the history: HistoryWeight at rest, falling to MotionWeight as
//      the motion approaches MOTION_FULL_PIXELS, and none at all on a reset or a rejection.
//
// The colour is resolved as it is stored, post-tonemap and display-encoded (the chain's LDR slot),
// which keeps the clip box and the blend perceptually even; see docs/mod/rendering/taa.md.
//
// The history texture's alpha holds each pixel's own raw depth, which the next frame's depth test
// reads back.

Texture2D<float4> Color : register(t0);
Texture2D<float4> Velocity : register(t1);
Texture2D<float> Depth : register(t2);
Texture2D<float4> History : register(t3);
SamplerState LinearClamp : register(s0);
RWTexture2D<float4> HistoryOut : register(u0);
RWTexture2D<float4> Output : register(u1);

struct EyeParams
{
    float4x4 ReprojCenter;  // current clip -> previous clip, the camera the engine encoded against
    float4x4 ReprojEye;     // current clip -> previous clip, this eye's own camera, jitter-free
    float2 Jitter;          // this frame's offset in the eye's pixels (x right, y down)
    uint Flags;             // FLAG_* below
    uint Padding;
};

cbuffer Params : register(b0)
{
    uint2 TextureSize;      // the whole chain texture, in pixels
    uint EyeWidth;          // one eye's window width (the texture's width when it holds one eye)
    uint EyeCount;          // 1, or 2 side by side
    float HistoryWeight;    // the history's share at rest
    float MotionWeight;     // the history's share at MOTION_FULL_PIXELS of motion and beyond
    float ClipGamma;        // the variance box's half-size in standard deviations, in motion
    float ClipGammaRest;    // the same at rest
    float CurrentSharpness; // the unjitter kernel's falloff
    float DepthTolerance;   // the relative depth slack of the history's depth test
    float2 Padding0;
    EyeParams Eyes[2];
};

static const uint FLAG_HISTORY = 1u;      // the eye's history is valid (no reset, no cut)
static const uint FLAG_REPROJECTION = 2u; // the reprojection matrices are valid
static const uint FLAG_CORRECTION = 4u;   // apply the stereo correction to drawn pixels
static const uint FLAG_HYBRID = 8u;       // undrawn and saturated pixels take the camera's motion

// The velocity target's clear value, as bytes (velocity_decode.hlsl).
static const uint4 UNDRAWN = uint4(127, 127, 255, 0);
// One code of the encoded motion, in UV.
static const float ENCODING_STEP = 0.25 / 255.0;
// The motion at which the history's share reaches MotionWeight, in pixels per frame.
static const float MOTION_FULL_PIXELS = 16.0;

float3 rgb_to_ycocg(float3 c)
{
    return float3(
        0.25 * c.r + 0.5 * c.g + 0.25 * c.b,
        0.5 * c.r - 0.5 * c.b,
        -0.25 * c.r + 0.5 * c.g - 0.25 * c.b);
}

float3 ycocg_to_rgb(float3 c)
{
    return float3(c.x + c.y - c.z, c.x + c.z, c.x - c.y - c.z);
}

// Clip q toward the centre of the box [lo, hi] until it lies inside it.
float3 clip_to_box(float3 lo, float3 hi, float3 q)
{
    float3 centre = 0.5 * (hi + lo);
    float3 extent = 0.5 * (hi - lo) + 1e-5;
    float3 offset = q - centre;
    float3 units = abs(offset / extent);
    float largest = max(units.x, max(units.y, units.z));
    return largest > 1.0 ? centre + offset / largest : q;
}

// The decoded motion (current UV less previous UV, Y down) of the texel at `source`, whose
// eye-local centre is at `uv`; `camera` is set when the motion is the camera's alone (no object
// motion the engine wrote).
float2 decode_motion(EyeParams eye, int2 source, float2 uv, out bool camera)
{
    float4 raw = Velocity.Load(int3(source, 0));
    uint4 bytes = uint4(round(saturate(raw) * 255.0));
    bool undrawn = all(bytes == UNDRAWN);
    bool saturated = any(bytes.xy == 0u) || any(bytes.xy == 255u);
    float2 motion = undrawn ? float2(0.0, 0.0) : (raw.xy - 0.5) * 0.25;
    camera = undrawn;
    bool correction = (eye.Flags & FLAG_CORRECTION) != 0;
    bool hybrid = (eye.Flags & FLAG_HYBRID) != 0;
    if ((eye.Flags & FLAG_REPROJECTION) != 0 && (correction || hybrid))
    {
        float depth = Depth.Load(int3(source, 0));
        float4 clip = float4(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
        float4 prev_eye = mul(eye.ReprojEye, clip);
        float2 camera_uv = (clip.xy - prev_eye.xy / prev_eye.w) * float2(0.5, -0.5);
        if (hybrid && (undrawn || saturated))
        {
            motion = camera_uv;
            camera = true;
        }
        else if (!undrawn && correction)
        {
            float4 prev_center = mul(eye.ReprojCenter, clip);
            float2 center_uv = (clip.xy - prev_center.xy / prev_center.w) * float2(0.5, -0.5);
            float2 object_uv = motion - center_uv;
            if (all(abs(object_uv) <= ENCODING_STEP))
            {
                object_uv = float2(0.0, 0.0);
                camera = true;
            }
            motion = camera_uv + object_uv;
        }
    }
    return motion;
}

// The view depth in the previous frame, over the near plane's, of the point at NDC `ndc_xy` and raw
// reverse-Z depth `depth` this frame: the previous clip w of the point, which the reprojection gives
// over this frame's w (the near plane over the depth).
float previous_depth_over_near(EyeParams eye, float2 ndc_xy, float depth)
{
    float4 previous = mul(eye.ReprojEye, float4(ndc_xy, depth, 1.0));
    return previous.w / depth;
}

[numthreads(8, 8, 1)]
void main(uint3 id : SV_DispatchThreadID)
{
    if (id.x >= TextureSize.x || id.y >= TextureSize.y)
    {
        return;
    }
    uint eye_index = (EyeCount > 1 && id.x >= EyeWidth) ? 1u : 0u;
    EyeParams eye = Eyes[eye_index];
    int left = int(eye_index * EyeWidth);
    int2 size = int2(EyeWidth, TextureSize.y);
    int2 local = int2(int(id.x) - left, int(id.y));

    float3 m1 = 0.0;
    float3 m2 = 0.0;
    float3 lo = 1e9;
    float3 hi = -1e9;
    float3 filtered = 0.0;
    float filtered_weight = 0.0;
    float nearest = -1.0;
    float farthest = 2.0;
    int2 nearest_local = local;
    float4 centre = 0.0;
    float centre_depth = 0.0;
    [unroll]
    for (int dy = -1; dy <= 1; ++dy)
    {
        [unroll]
        for (int dx = -1; dx <= 1; ++dx)
        {
            int2 q = clamp(local + int2(dx, dy), int2(0, 0), size - 1);
            int2 source = q + int2(left, 0);
            float4 rgba = Color.Load(int3(source, 0));
            float3 c = rgb_to_ycocg(rgba.rgb);
            m1 += c;
            m2 += c * c;
            lo = min(lo, c);
            hi = max(hi, c);
            float2 d = float2(dx, dy) - eye.Jitter;
            float w = exp(-CurrentSharpness * dot(d, d));
            filtered += c * w;
            filtered_weight += w;
            float z = Depth.Load(int3(source, 0));
            if (z > nearest)
            {
                nearest = z;
                nearest_local = q;
            }
            farthest = min(farthest, z);
            if (dx == 0 && dy == 0)
            {
                centre = rgba;
                centre_depth = z;
            }
        }
    }
    float3 current = filtered / filtered_weight;
    float3 mean = m1 / 9.0;
    float3 sigma = sqrt(max(m2 / 9.0 - mean * mean, 0.0));

    float2 eye_size = float2(size);
    float2 uv = (float2(local) + 0.5) / eye_size;
    bool camera_motion;
    float2 motion = decode_motion(
        eye, nearest_local + int2(left, 0), (float2(nearest_local) + 0.5) / eye_size, camera_motion);
    float2 previous_uv = uv - motion;
    float motion_pixels = length(motion * eye_size);
    float motion_factor = saturate(motion_pixels / MOTION_FULL_PIXELS);

    bool history_valid = (eye.Flags & FLAG_HISTORY) != 0
        && all(previous_uv >= 0.0) && all(previous_uv <= 1.0);
    float3 resolved = current;
    if (history_valid)
    {
        // Catmull-Rom through five bilinear taps (the corner taps' weights are negligible).
        float2 position = previous_uv * eye_size;
        float2 texel1 = floor(position - 0.5) + 0.5;
        float2 f = position - texel1;
        float2 w0 = f * (-0.5 + f * (1.0 - 0.5 * f));
        float2 w1 = 1.0 + f * f * (-2.5 + 1.5 * f);
        float2 w2 = f * (0.5 + f * (2.0 - 1.5 * f));
        float2 w3 = f * f * (-0.5 + 0.5 * f);
        float2 w12 = w1 + w2;
        float2 texel12 = texel1 + w2 / w12;
        float2 texel0 = texel1 - 1.0;
        float2 texel3 = texel1 + 2.0;
        float2 texture_size = float2(TextureSize);
        float2 lo_uv = float2(float(left) + 0.5, 0.5) / texture_size;
        float2 hi_uv = float2(float(left + size.x) - 0.5, float(size.y) - 0.5) / texture_size;
        float2 uv0 = clamp((texel0 + float2(left, 0)) / texture_size, lo_uv, hi_uv);
        float2 uv12 = clamp((texel12 + float2(left, 0)) / texture_size, lo_uv, hi_uv);
        float2 uv3 = clamp((texel3 + float2(left, 0)) / texture_size, lo_uv, hi_uv);
        float3 history = 0.0;
        float history_weight_sum = 0.0;
        float weight;
        weight = w12.x * w0.y;
        history += History.SampleLevel(LinearClamp, float2(uv12.x, uv0.y), 0).rgb * weight;
        history_weight_sum += weight;
        weight = w0.x * w12.y;
        history += History.SampleLevel(LinearClamp, float2(uv0.x, uv12.y), 0).rgb * weight;
        history_weight_sum += weight;
        weight = w12.x * w12.y;
        history += History.SampleLevel(LinearClamp, uv12, 0).rgb * weight;
        history_weight_sum += weight;
        weight = w3.x * w12.y;
        history += History.SampleLevel(LinearClamp, float2(uv3.x, uv12.y), 0).rgb * weight;
        history_weight_sum += weight;
        weight = w12.x * w3.y;
        history += History.SampleLevel(LinearClamp, float2(uv12.x, uv3.y), 0).rgb * weight;
        history_weight_sum += weight;
        history = max(history / history_weight_sum, 0.0);

        // The depth test, for camera motion: the previous depth at the nearest texel must lie
        // within what the neighbourhood's nearest and farthest depths reproject to.
        if (camera_motion && (eye.Flags & FLAG_REPROJECTION) != 0)
        {
            int2 previous_texel = clamp(int2(floor(position)), int2(0, 0), size - 1);
            float previous_depth = History.Load(int3(previous_texel + int2(left, 0), 0)).a;
            float2 ndc = float2(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
            // Sky (the infinite far plane) is a depth of zero, and an infinite distance.
            float history_far = previous_depth > 1e-7 ? 1.0 / previous_depth : 1e30;
            float expected_near = nearest > 1e-7
                ? previous_depth_over_near(eye, ndc, nearest) : 1e30;
            float expected_far = farthest > 1e-7
                ? previous_depth_over_near(eye, ndc, farthest) : 1e30;
            if (history_far < expected_near * (1.0 - DepthTolerance)
                || (expected_far < 1e29 && history_far > expected_far * (1.0 + DepthTolerance)))
            {
                history_valid = false;
            }
        }

        if (history_valid)
        {
            float3 history_ycocg = rgb_to_ycocg(history);
            float gamma = lerp(ClipGammaRest, ClipGamma, motion_factor);
            float3 box_lo = max(mean - gamma * sigma, lo);
            float3 box_hi = min(mean + gamma * sigma, hi);
            history_ycocg = clip_to_box(box_lo, box_hi, history_ycocg);
            float history_share = lerp(HistoryWeight, MotionWeight, motion_factor);
            resolved = lerp(current, history_ycocg, history_share);
        }
    }
    float3 rgb = saturate(ycocg_to_rgb(resolved));
    HistoryOut[id.xy] = float4(rgb, centre_depth);
    Output[id.xy] = float4(rgb, centre.a);
}
