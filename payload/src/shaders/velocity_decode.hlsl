// Decode JC3's bias-encoded velocity buffer into an unbiased R16G16F motion-vector buffer for FSR.
//
// JC3's velocity buffer (m_VelocityBufferTexture, ABGR8) stores, per the RE'd velocity-write shader:
//   stored.xy = clamp((curUV - prevUV) * 8, -1, 1) * 0.5 + 0.5
// i.e. screen-space motion in UV units (Y-down), scaled x8, clamped to +/-1, packed into [0,1] with
// 0.5 == zero motion. FSR's motionVectorScale is a pure multiply and cannot subtract the 0.5 bias, so
// we decode here:
//   motion_uv = (stored.xy - 0.5) * 0.25     // = curUV - prevUV, UV space, Y-down
// then apply FSR's convention via the SignScale constant and write UV-space motion; the dispatch's
// motionVectorScale maps UV to pixels. FSR reprojects a pixel's history from its current UV plus its
// motion (Y-down UV), so it wants prevUV - curUV: the sign is negative on both axes.
//
// One code of the encoding is a quarter of a 255th of the screen (three and a half pixels at the
// headset's width), and zero motion falls between two codes, so a still pixel the velocity pass drew
// decodes to half a code of motion. The clear value (UNDRAWN below) is not motion the engine wrote, so
// it decodes to none.
//
// In stereo the engine's velocity is also mis-anchored: the current clip position uses the per-eye
// view-projection, but the previous-frame reprojection uses the single sim-side *center*
// view-projection, so every static pixel gains a spurious parallax vector (depth-dependent, opposite
// sign per eye) and FSR mis-reprojects each eye's temporal history. When CorrectionEnabled is set,
// a drawn pixel is reprojected into the previous frame with both matrices, and its motion becomes this
// eye's camera motion plus the object motion the engine encoded on top of the centre camera's:
//   corrected = cameraUV_eye + (decoded - cameraUV_center)
// which is decoded + (prevUV_center - prevUV_eye). An object motion within one code of zero is the
// encoding's rounding of a still pixel, and is dropped.
//
// The velocity pass draws only a subset of the scene (the vegetation and a few instanced models), and
// every other pixel keeps the target's clear value, which no velocity pixel shader writes short of a
// far pixel whose motion rounds to exactly that code: every velocity shader writes its z from the
// clip w, saturating at the clear's z beyond a hundred metres, and its motion in x and y. When
// HybridEnabled is set, such a pixel, and one whose encoded motion saturated at the encoding's
// +/-0.125 UV clamp, takes the camera's motion computed from its depth instead: the pixel reprojected
// into the previous frame through this eye's own reprojection, at full precision and unclamped.
// Every other pixel keeps the engine's motion, re-anchored as above. At the sky the depth is the
// infinite far plane and the reprojection is the rotation alone.
//
// The pass can read a Size-sized window of larger sources at Offset: one eye's half of the
// single-pass collapse's double-wide target. With CopyDepth set it also copies that window of the
// depth into DepthOutput, so FSR can be handed an eye-sized depth (a depth-stencil resource cannot be
// copied by region).

Texture2D<float4> Velocity : register(t0);
Texture2D<float> Depth : register(t1);
RWTexture2D<float2> Output : register(u0);
RWTexture2D<float> DepthOutput : register(u1);

cbuffer Params : register(b0)
{
    uint2 Size;             // output resolution (pixels)
    float2 SignScale;       // applied to the decoded UV motion (FSR's convention)
    float4x4 ReprojCenter;  // current clip -> previous-frame clip, center camera (what the engine encoded)
    float4x4 ReprojEye;     // current clip -> previous-frame clip, this eye's camera (what FSR wants)
    uint CorrectionEnabled; // 0 = pass the decode through (no stereo disparity, or no history yet)
    uint2 Offset;           // the window's top-left texel in the sources
    uint CopyDepth;         // 1 = also copy the window's depth into DepthOutput
    uint HybridEnabled;     // 1 = undrawn and saturated pixels take the camera motion from depth
    uint3 Padding;
};

// The velocity target's clear value, as bytes: what a pixel the velocity pass never drew holds.
static const uint4 UNDRAWN = uint4(127, 127, 255, 0);

// One code of the encoded motion, in UV.
static const float ENCODING_STEP = 0.25 / 255.0;

[numthreads(8, 8, 1)]
void main(uint3 id : SV_DispatchThreadID)
{
    if (id.x >= Size.x || id.y >= Size.y)
    {
        return;
    }
    int3 source = int3(id.xy + Offset, 0);
    float4 raw = Velocity.Load(source);
    uint4 bytes = uint4(round(saturate(raw) * 255.0));
    bool undrawn = all(bytes == UNDRAWN);
    bool saturated = any(bytes.xy == 0u) || any(bytes.xy == 255u);
    float2 motion_uv = undrawn ? float2(0.0, 0.0) : (raw.xy - 0.5) * 0.25;
    if (CorrectionEnabled != 0 || HybridEnabled != 0)
    {
        // Reconstruct the pixel's clip position from the raster depth (the matrices match the raster
        // view-projection exactly, so the reverse-Z convention passes straight through) and reproject
        // it into the previous frame. At the infinite far plane the translation vanishes, so the
        // camera motion there is the rotation's and the parallax correction tends to zero.
        float depth = Depth.Load(source).x;
        float2 uv = (float2(id.xy) + 0.5) / float2(Size);
        float4 clip = float4(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
        float4 prev_eye = mul(ReprojEye, clip);
        float2 camera_uv = (clip.xy - prev_eye.xy / prev_eye.w) * float2(0.5, -0.5);
        if (HybridEnabled != 0 && (undrawn || saturated))
        {
            motion_uv = camera_uv;
        }
        else if (!undrawn)
        {
            float4 prev_center = mul(ReprojCenter, clip);
            float2 center_uv = (clip.xy - prev_center.xy / prev_center.w) * float2(0.5, -0.5);
            float2 object_uv = motion_uv - center_uv;
            if (all(abs(object_uv) <= ENCODING_STEP))
            {
                object_uv = float2(0.0, 0.0);
            }
            motion_uv = camera_uv + object_uv;
        }
    }
    Output[id.xy] = motion_uv * SignScale;
    if (CopyDepth != 0)
    {
        DepthOutput[id.xy] = Depth.Load(source).x;
    }
}
