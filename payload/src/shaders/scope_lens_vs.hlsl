// Scope lens vertex shader: a square around the eyepiece, in the lens's plane, facing back along the
// sight line. The pixel shader cuts the disc out of it. Positions are relative to the sighting eye,
// projected through its translation-free view-projection.

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

struct VSOut
{
    float4 position : SV_Position;
    float2 q : TEXCOORD0;
};

VSOut main(uint vid : SV_VertexID)
{
    float2 q = float2((vid & 1) ? 1.0 : -1.0, (vid & 2) ? 1.0 : -1.0);
    float3 p = EyepieceFromEye + (Right * q.x + Up * q.y) * Radius;
    VSOut o;
    o.position = mul(float4(p, 1.0), ViewProjection);
    o.q = q;
    return o;
}
