//! The detours that make a replay of
//! [`RenderBlockTypeGeneralMkIII::Create`](jc3gi::graphics_engine::render_block::RenderBlockTypeGeneralMkIII::Create)
//! on a scratch copy of the type create nothing but its instanced vertex declarations, with their
//! step rates doubled (see [`crate::stereo::single_pass::hw_instanced`]).
//!
//! Every detour forwards unchanged unless the calling thread is inside that replay, so outside it
//! they cost one thread-local read. Inside it, the callees of `Create` that would acquire a
//! resource -- the wind noise texture, the shader holders, the sampler states, the identity palette,
//! and the per-vertex declarations -- return without doing so, which leaves `Create` with no effect
//! beyond the scratch copy and the instanced declarations the replay asks for.

use std::ffi::c_void;

use detours_macro::detour;
use jc3gi::graphics_engine::{
    graphics_engine::{
        HDevice_t, HVertexDeclaration_t, HVertexProgram_t, SInstancedVertexElement, SVertexElement,
    },
    render_block::RenderBlockTypeGeneralMkIII,
    render_engine::{ResourceContext, TextureCache},
};
use re_utilities::hook_library::HookLibrary;

use crate::stereo::single_pass::hw_instanced::{replay_instanced, replaying};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&CREATE_VERTEX_DECLARATION_INSTANCED_BINDER)
        .with_static_binder(&CREATE_VERTEX_DECLARATION_BINDER)
        .with_static_binder(&CREATE_CONSTANT_BUFFER_BINDER)
        .with_static_binder(&TEXTURE_CACHE_GET_TEXTURE_BINDER)
        .with_static_binder(&GENERAL_MK_III_LOAD_SHADERS_BINDER)
        .with_static_binder(&GENERAL_MK_III_CREATE_SAMPLER_STATES_BINDER)
}

/// Inside the replay, the instanced declaration is created with its step rates doubled and recorded
/// as a twin.
#[detour(address = jc3gi::graphics_engine::graphics_engine::CreateVertexDeclarationInstanced_ADDRESS)]
fn create_vertex_declaration_instanced(
    device: *mut HDevice_t,
    parameters: *const SVertexElement,
    parameters_count: i32,
    inst_parameters: *const SInstancedVertexElement,
    inst_parameters_count: i32,
    program: *mut HVertexProgram_t,
) -> *mut HVertexDeclaration_t {
    let original = CREATE_VERTEX_DECLARATION_INSTANCED.get().unwrap();
    if !replaying() {
        return original.call(
            device,
            parameters,
            parameters_count,
            inst_parameters,
            inst_parameters_count,
            program,
        );
    }
    // The engine dereferences the program for its bytecode; a holder with no program creates nothing.
    if program.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: `Create` passes the element array it assembled on its stack, of this length.
    unsafe {
        replay_instanced(inst_parameters, inst_parameters_count, |doubled| {
            original.call(
                device,
                parameters,
                parameters_count,
                doubled,
                inst_parameters_count,
                program,
            )
        })
    }
}

/// Inside the replay, the per-vertex declarations are not created.
#[detour(address = jc3gi::graphics_engine::graphics_engine::CreateVertexDeclaration_ADDRESS)]
fn create_vertex_declaration(
    device: *mut HDevice_t,
    parameters: *const SVertexElement,
    parameters_count: i32,
    program: *mut HVertexProgram_t,
) -> *mut HVertexDeclaration_t {
    if replaying() {
        return std::ptr::null_mut();
    }
    CREATE_VERTEX_DECLARATION
        .get()
        .unwrap()
        .call(device, parameters, parameters_count, program)
}

/// Inside the replay, the identity palette is not created.
#[detour(address = jc3gi::graphics_engine::graphics_engine::CreateConstantBuffer_ADDRESS)]
fn create_constant_buffer(
    device: *mut HDevice_t,
    register_count: i32,
    usage: i32,
    data: *const c_void,
    tag: bool,
) -> *mut c_void {
    if replaying() {
        return std::ptr::null_mut();
    }
    CREATE_CONSTANT_BUFFER
        .get()
        .unwrap()
        .call(device, register_count, usage, data, tag)
}

/// Inside the replay, the wind noise texture is not bound: the scratch copy's holder is the live
/// object's, and binding through it would release the live object's reference.
#[detour(address = jc3gi::graphics_engine::render_engine::TextureCacheGetTexture_ADDRESS)]
fn texture_cache_get_texture(
    cache: *mut TextureCache,
    holder: *mut c_void,
    path: *const u8,
    texture_type: i32,
) -> i32 {
    if replaying() {
        return 0;
    }
    TEXTURE_CACHE_GET_TEXTURE
        .get()
        .unwrap()
        .call(cache, holder, path, texture_type)
}

/// Inside the replay, the shader holders are not acquired: the scratch copy already holds the live
/// object's, which is what the declaration code reads the programs from.
#[detour(address = jc3gi::graphics_engine::render_block::RenderBlockTypeGeneralMkIII::LoadShaders_ADDRESS)]
fn general_mk_iii_load_shaders(
    this: *mut RenderBlockTypeGeneralMkIII,
    resource_context: *const ResourceContext,
) {
    if replaying() {
        return;
    }
    GENERAL_MK_III_LOAD_SHADERS
        .get()
        .unwrap()
        .call(this, resource_context);
}

/// Inside the replay, the sampler states are not created.
#[detour(
    address = jc3gi::graphics_engine::render_block::RenderBlockTypeGeneralMkIII::CreateSamplerStates_ADDRESS
)]
fn general_mk_iii_create_sampler_states(
    this: *mut RenderBlockTypeGeneralMkIII,
    device: *mut HDevice_t,
) {
    if replaying() {
        return;
    }
    GENERAL_MK_III_CREATE_SAMPLER_STATES
        .get()
        .unwrap()
        .call(this, device);
}
