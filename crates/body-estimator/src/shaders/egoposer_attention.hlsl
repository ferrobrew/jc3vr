// Multi-head scaled dot-product self-attention over TOKENS tokens with no mask, before the output
// projection: for each query and head, softmax(q k^T / sqrt(HEAD_WIDTH)) v.
//
// The input holds each token's fused projection, its query, key, and value of WIDTH values each,
// side by side with the heads' columns adjacent within each. One thread group handles one head of
// one query: a thread per key scores it, the softmax's weights go through groupshared memory, and
// a thread per column of the head sums the weighted values. Only the last Queries tokens are
// queried, the output's rows being theirs in order.

#define TOKENS 40
#define WIDTH 256
#define HEADS 8
#define HEAD_WIDTH (WIDTH / HEADS)
#define THREADS 64

ByteAddressBuffer Projections : register(t0);
RWByteAddressBuffer Output : register(u0);

cbuffer Params : register(b0)
{
    uint Queries;  // how many of the last tokens are queried: one group each along y
    float Scale;   // 1 / sqrt(HEAD_WIDTH)
    uint2 _padding;
};

groupshared float query[HEAD_WIDTH];
groupshared float scores[TOKENS];
groupshared float weights[TOKENS];

// Where a token's query (part 0), key (1), or value (2) starts for a head, in bytes.
uint projection(uint token, uint part, uint head)
{
    return (token * 3 * WIDTH + part * WIDTH + head * HEAD_WIDTH) * 4;
}

[numthreads(THREADS, 1, 1)]
void main(uint3 group : SV_GroupID, uint thread : SV_GroupIndex)
{
    uint head = group.x;
    uint token = TOKENS - Queries + group.y;
    if (thread < HEAD_WIDTH)
    {
        query[thread] = asfloat(Projections.Load(projection(token, 0, head) + thread * 4));
    }
    GroupMemoryBarrierWithGroupSync();

    if (thread < TOKENS)
    {
        float score = 0.0;
        [unroll]
        for (uint d = 0; d < HEAD_WIDTH; d += 4)
        {
            float4 k = asfloat(Projections.Load4(projection(thread, 1, head) + d * 4));
            score += dot(float4(query[d], query[d + 1], query[d + 2], query[d + 3]), k);
        }
        scores[thread] = score * Scale;
    }
    GroupMemoryBarrierWithGroupSync();

    float highest = scores[0];
    for (uint i = 1; i < TOKENS; ++i)
    {
        highest = max(highest, scores[i]);
    }
    if (thread < TOKENS)
    {
        weights[thread] = exp(scores[thread] - highest);
    }
    GroupMemoryBarrierWithGroupSync();

    float total = 0.0;
    for (uint j = 0; j < TOKENS; ++j)
    {
        total += weights[j];
    }
    if (thread < HEAD_WIDTH)
    {
        float context = 0.0;
        for (uint k = 0; k < TOKENS; ++k)
        {
            float value = asfloat(Projections.Load(projection(k, 2, head) + thread * 4));
            context += weights[k] / total * value;
        }
        Output.Store((group.y * WIDTH + head * HEAD_WIDTH + thread) * 4, asuint(context));
    }
}
