use std::fmt::Write as _;

use windows::{
    Win32::Graphics::{
        Direct3D11::{D3D11_BUFFER_DESC, ID3D11Buffer, ID3D11DeviceContext},
        Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_UNKNOWN},
    },
    core::Interface,
};

use crate::screenshot::harness::water_gpu_probe::ProbeContext;

const VERTEX_SLOTS: usize = 4;

pub(super) unsafe fn capture(ctx: &ID3D11DeviceContext, context: ProbeContext) {
    // SAFETY: per this function's contract, the getters only read bound state.
    let input_layout = unsafe { ctx.IAGetInputLayout() }.ok();
    let mut buffers: [Option<ID3D11Buffer>; VERTEX_SLOTS] = std::array::from_fn(|_| None);
    let mut strides = [0u32; VERTEX_SLOTS];
    let mut offsets = [0u32; VERTEX_SLOTS];
    // SAFETY: the arrays are sized for the slots read and are out parameters only.
    unsafe {
        ctx.IAGetVertexBuffers(
            0,
            VERTEX_SLOTS as u32,
            Some(buffers.as_mut_ptr()),
            Some(strides.as_mut_ptr()),
            Some(offsets.as_mut_ptr()),
        );
    }
    let vertex_buffers: [VertexBuffer; VERTEX_SLOTS] = std::array::from_fn(|slot| {
        // SAFETY: the buffer is live for this call, and `GetDesc` only writes the out parameter.
        unsafe { VertexBuffer::read(buffers[slot].as_ref(), strides[slot], offsets[slot]) }
    });
    let mut index_buffer = None;
    let mut format = DXGI_FORMAT_UNKNOWN;
    let mut offset = 0u32;
    // SAFETY: the out parameters are sized for the one index buffer read.
    unsafe {
        ctx.IAGetIndexBuffer(
            Some(&mut index_buffer),
            Some(&mut format),
            Some(&mut offset),
        );
    }
    // SAFETY: the buffer is live for this call, and `GetDesc` only writes the out parameter.
    let index_buffer = unsafe { IndexBuffer::read(index_buffer.as_ref(), format, offset) };
    let mut text = String::new();
    for (slot, buffer) in vertex_buffers.iter().enumerate() {
        let _ = write!(text, " {slot}[{buffer}]");
    }
    tracing::info!(
        target: "water_probe",
        "water probe: ia state pass={} eye={} invocation={} predicted_display_time_ns={} \
         generation={} input_layout={:#x} vertex_slots={}{text} index_buffer[{index_buffer}]",
        context.pass,
        context.eye,
        context.invocation,
        context.frame.display_time(),
        context.frame.visual_generation,
        input_layout.as_ref().map_or(0, |layout| layout.as_raw() as usize),
        VERTEX_SLOTS,
    );
}

#[derive(Clone, Copy, Default)]
struct VertexBuffer {
    pointer: usize,
    byte_width: u32,
    stride: u32,
    offset: u32,
}

impl VertexBuffer {
    /// # Safety
    ///
    /// `buffer` must be null or a live buffer.
    unsafe fn read(buffer: Option<&ID3D11Buffer>, stride: u32, offset: u32) -> Self {
        let Some(buffer) = buffer else {
            return Self {
                stride,
                offset,
                ..Self::default()
            };
        };
        let mut desc = D3D11_BUFFER_DESC::default();
        // SAFETY: per this function's contract, `buffer` is live and `GetDesc` only writes the out
        // parameter.
        unsafe { buffer.GetDesc(&mut desc) };
        Self {
            pointer: buffer.as_raw() as usize,
            byte_width: desc.ByteWidth,
            stride,
            offset,
        }
    }
}

impl std::fmt::Display for VertexBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "pointer={:#x} byte_width={} stride={} offset={}",
            self.pointer, self.byte_width, self.stride, self.offset
        )
    }
}

#[derive(Clone, Copy, Default)]
struct IndexBuffer {
    pointer: usize,
    byte_width: u32,
    format: i32,
    offset: u32,
}

impl IndexBuffer {
    /// # Safety
    ///
    /// `buffer` must be null or a live buffer.
    unsafe fn read(buffer: Option<&ID3D11Buffer>, format: DXGI_FORMAT, offset: u32) -> Self {
        let Some(buffer) = buffer else {
            return Self {
                format: format.0,
                offset,
                ..Self::default()
            };
        };
        let mut desc = D3D11_BUFFER_DESC::default();
        // SAFETY: per this function's contract, `buffer` is live and `GetDesc` only writes the out
        // parameter.
        unsafe { buffer.GetDesc(&mut desc) };
        Self {
            pointer: buffer.as_raw() as usize,
            byte_width: desc.ByteWidth,
            format: format.0,
            offset,
        }
    }
}

impl std::fmt::Display for IndexBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "pointer={:#x} byte_width={} format={} offset={}",
            self.pointer, self.byte_width, self.format, self.offset
        )
    }
}
