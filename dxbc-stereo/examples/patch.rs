//! Rewrites a shader for single-pass stereo and writes the result to stdout.
//!
//! A dev tool for producing rewritten shaders to inspect (disassemble, diff, feed a validator). The
//! default is the vertex shader's `cb0` remap; `--reproject` takes the reprojection,
//! `--reproject-instanced` the instanced reprojection, which halves the shader's own
//! `SV_InstanceID`, and `--velocity` the velocity pixel shader's per-eye motion vector:
//!
//! ```sh
//! cargo run -p dxbc-stereo --example patch --target x86_64-unknown-linux-gnu -- in.dxbc > out.dxbc
//! cargo run -p dxbc-stereo --example patch --target x86_64-unknown-linux-gnu -- \
//!     --reproject-instanced in.dxbc > out.dxbc
//! ```

use std::io::Write;

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let rewrite = match args.first().map(String::as_str) {
        Some("--reproject") => {
            args.remove(0);
            dxbc_stereo::reproject_vertex_shader
        }
        Some("--reproject-instanced") => {
            args.remove(0);
            dxbc_stereo::reproject_instanced_vertex_shader
        }
        Some("--velocity") => {
            args.remove(0);
            dxbc_stereo::localize_velocity
        }
        _ => dxbc_stereo::patch_vertex_shader,
    };
    let path = args
        .first()
        .expect("usage: patch [--reproject | --reproject-instanced | --velocity] <in.dxbc>");
    let blob = std::fs::read(path).expect("read input");
    match rewrite(&blob) {
        Ok(patched) => std::io::stdout().write_all(&patched).expect("write stdout"),
        Err(e) => {
            eprintln!("patch: {e}");
            std::process::exit(1);
        }
    }
}
