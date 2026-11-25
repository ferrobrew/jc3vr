// A residual add and a layer normalisation: y = LayerNorm(x + residual), PyTorch's `nn.LayerNorm`
// with the biased variance and the default epsilon, over rows of WIDTH values.
//
// One thread group normalises one row, a thread per value; the mean and then the variance about
// it are summed by a tree reduction in groupshared memory.

#define WIDTH 256
#define EPSILON 1e-5

ByteAddressBuffer Input : register(t0);
ByteAddressBuffer Residual : register(t1);
ByteAddressBuffer Parameters : register(t2);
RWByteAddressBuffer Output : register(u0);

cbuffer Params : register(b0)
{
    uint ResidualOffset;  // where the residual's first row starts in Residual
    uint GammaOffset;     // where the WIDTH scales start in Parameters
    uint BetaOffset;      // where the WIDTH shifts start in Parameters
    uint _padding;
};

groupshared float reduction[WIDTH];

float row_sum(uint thread, float value)
{
    reduction[thread] = value;
    GroupMemoryBarrierWithGroupSync();
    [unroll]
    for (uint stride = WIDTH / 2; stride > 0; stride /= 2)
    {
        if (thread < stride)
        {
            reduction[thread] += reduction[thread + stride];
        }
        GroupMemoryBarrierWithGroupSync();
    }
    float sum = reduction[0];
    // Every thread has read the sum before the next reduction overwrites it.
    GroupMemoryBarrierWithGroupSync();
    return sum;
}

[numthreads(WIDTH, 1, 1)]
void main(uint3 group : SV_GroupID, uint thread : SV_GroupIndex)
{
    uint row = group.x;
    float x = asfloat(Input.Load((row * WIDTH + thread) * 4))
              + asfloat(Residual.Load((ResidualOffset + row * WIDTH + thread) * 4));
    float mean = row_sum(thread, x) / WIDTH;
    float centred = x - mean;
    float variance = row_sum(thread, centred * centred) / WIDTH;
    float gamma = asfloat(Parameters.Load((GammaOffset + thread) * 4));
    float beta = asfloat(Parameters.Load((BetaOffset + thread) * 4));
    float y = centred * (1.0 / sqrt(variance + EPSILON)) * gamma + beta;
    Output.Store((row * WIDTH + thread) * 4, asuint(y));
}
