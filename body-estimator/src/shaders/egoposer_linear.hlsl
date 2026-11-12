// A fully connected layer, y = x W^T + b, optionally followed by a ReLU.
//
// One thread group computes one output for up to ROWS_PER_GROUP rows. Each row's threads split the
// inputs between them four values at a time, interleaved so that neighbouring threads read
// neighbouring values: the weight row and the input rows are read coalesced, and the group's rows
// share the weight row's loads through the cache. The lanes' partial sums meet in groupshared
// memory. Every thread keeps one sum, so nothing spills out of registers.
//
// The weights are stored as 32-bit floats, or as 16-bit floats when compiled with WEIGHTS_F16=1,
// and accumulate in 32 bits either way. The biases, the input, and the output are 32-bit floats.
// Every offset is in elements, and every weight matrix starts on a multiple of four elements.

#ifndef WEIGHTS_F16
#define WEIGHTS_F16 0
#endif

#define LANES 16
#define ROWS_PER_GROUP 8

ByteAddressBuffer Weights : register(t0);
ByteAddressBuffer Biases : register(t1);
ByteAddressBuffer Input : register(t2);
RWByteAddressBuffer Output : register(u0);

cbuffer Params : register(b0)
{
    uint Rows;          // rows of the input: ROWS_PER_GROUP to each group along y
    uint Inputs;        // values in an input row, a multiple of four
    uint Outputs;       // values in an output row: one group each along x
    uint Relu;          // nonzero to clamp the outputs at zero
    uint InputOffset;   // where the input's first row starts in Input
    uint OutputOffset;  // where the output's first row starts in Output
    uint WeightOffset;  // where the Outputs x Inputs weight matrix starts in Weights
    uint BiasOffset;    // where the Outputs biases start in Biases
};

groupshared float partial[ROWS_PER_GROUP * LANES];

float4 load_weights(uint element)
{
#if WEIGHTS_F16
    uint2 packed = Weights.Load2(element * 2);
    return float4(f16tof32(packed.x), f16tof32(packed.x >> 16), f16tof32(packed.y),
                  f16tof32(packed.y >> 16));
#else
    return asfloat(Weights.Load4(element * 4));
#endif
}

[numthreads(LANES, ROWS_PER_GROUP, 1)]
void main(uint3 group : SV_GroupID, uint3 thread : SV_GroupThreadID)
{
    uint output = group.x;
    uint lane = thread.x;
    uint row = group.y * ROWS_PER_GROUP + thread.y;
    // The rows past the last compute nothing, but still meet the barrier.
    uint quads = row < Rows ? Inputs / 4 : 0;

    float sum = 0.0;
    for (uint q = lane; q < quads; q += LANES)
    {
        float4 w = load_weights(WeightOffset + output * Inputs + q * 4);
        float4 x = asfloat(Input.Load4((InputOffset + row * Inputs + q * 4) * 4));
        sum += dot(w, x);
    }
    partial[thread.y * LANES + lane] = sum;
    GroupMemoryBarrierWithGroupSync();

    if (lane == 0 && row < Rows)
    {
        float y = asfloat(Biases.Load((BiasOffset + output) * 4));
        [unroll]
        for (uint l = 0; l < LANES; ++l)
        {
            y += partial[thread.y * LANES + l];
        }
        if (Relu != 0)
        {
            y = max(y, 0.0);
        }
        Output.Store((OutputOffset + row * Outputs + output) * 4, asuint(y));
    }
}
