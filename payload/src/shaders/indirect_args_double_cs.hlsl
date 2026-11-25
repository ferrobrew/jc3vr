// A copy of a buffer of indexed indirect draw arguments with every draw's instance count doubled.
//
// The source holds five-dword `DrawIndexedInstancedIndirect` records back to back (index count,
// instance count, start index, base vertex, start instance). One thread per dword copies it, doubling
// the second dword of each record, so a draw issued against the copy at the same offset runs each of
// its instances twice: instances 2k and 2k + 1 for the game's instance k.

Buffer<uint> Source : register(t0);
RWBuffer<uint> Doubled : register(u0);

#define DWORDS_PER_DRAW 5
#define INSTANCE_COUNT_DWORD 1

[numthreads(64, 1, 1)]
void main(uint3 id : SV_DispatchThreadID)
{
    uint count;
    Source.GetDimensions(count);
    if (id.x >= count)
    {
        return;
    }
    const uint value = Source[id.x];
    Doubled[id.x] = id.x % DWORDS_PER_DRAW == INSTANCE_COUNT_DWORD ? value * 2 : value;
}
