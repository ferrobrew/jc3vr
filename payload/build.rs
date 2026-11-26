//! Embeds a compile-time build stamp so the running payload can prove which build it is. An
//! uninject can leave the module resident (a hung shutdown thread keeps the DLL mapped), and a
//! failed re-inject then silently continues on the stale code — the stamp is announced at startup
//! (log, in-headset banner, and the debug window) so a stale payload is visible at a glance.
//!
//! Also stages every layer crate's tuning files beside the built DLL, as `tuning/<crate>/<file>`,
//! since the payload refuses to start without them (`docs/mod/tuning.md`). The build's output
//! directory is the mod's package: what sits beside the DLL there is what a player copies.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    // Re-stamp whenever any payload source changes, so the stamp tracks the code it was built
    // from rather than the last time this script happened to run. The layer crates the payload is
    // built from count as its source.
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=../crates/jc3vr-core/src");
    println!("cargo:rerun-if-changed=../crates/jc3vr-input/src");
    println!("cargo:rerun-if-changed=../crates/jc3vr-view/src");
    println!("cargo:rerun-if-changed=../crates/jc3vr-view/shaders");
    println!("cargo:rerun-if-changed=../crates/jc3vr-render/src");
    println!("cargo:rerun-if-changed=../crates/jc3vr-render/shaders");
    println!("cargo:rerun-if-changed=../crates/jc3vr-capture/src");
    println!("cargo:rerun-if-changed=../crates/jc3vr-capture/shaders");
    println!("cargo:rerun-if-changed=../crates/jc3vr-body/src");
    println!("cargo:rerun-if-changed=../crates/jc3vr-weapons/src");
    println!("cargo:rerun-if-changed=../crates/jc3vr-physical/src");
    println!("cargo:rerun-if-changed=../crates/jc3vr-vehicles/src");
    println!("cargo:rerun-if-changed=../crates/jc3vr-grapple/src");
    println!("cargo:rerun-if-changed=../crates/jc3vr-gameplay/src");
    println!("cargo:rerun-if-changed=../crates/jc3vr-hud/src");
    println!("cargo:rerun-if-changed=../crates/jc3vr-hud/shaders");
    println!("cargo:rerun-if-changed=../crates/jc3vr-hud/assets");
    println!("cargo:rerun-if-changed=../crates/jc3vr-tooling/src");
    println!("cargo:rerun-if-changed=../crates/jc3vr-tooling/shaders");
    println!("cargo:rerun-if-changed=../crates/jc3vr-ui/src");
    let stamp = jiff::Zoned::now().strftime("%Y-%m-%d %H:%M:%S").to_string();
    println!("cargo:rustc-env=jc3vr_build_stamp={stamp}");
    stage_tuning();
}

/// Mirrors each `crates/<crate>/tuning/*.toml` into `<profile dir>/tuning/<crate>/`, replacing what
/// an earlier build staged so that a removed file does not linger.
fn stage_tuning() {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    // `OUT_DIR` is `<profile dir>/build/<package>-<hash>/out`; the DLL is built into the profile
    // directory.
    let profile_dir = out_dir
        .ancestors()
        .nth(3)
        .expect("OUT_DIR sits three levels under the profile directory");
    let staged = profile_dir.join("tuning");
    if staged.exists() {
        fs::remove_dir_all(&staged)
            .unwrap_or_else(|e| panic!("tuning: could not clear {}: {e}", staged.display()));
    }

    let crates = Path::new("../crates");
    let mut entries: Vec<_> = fs::read_dir(crates)
        .unwrap_or_else(|e| panic!("tuning: could not list {}: {e}", crates.display()))
        .map(|entry| entry.expect("tuning: a crate directory entry").path())
        .collect();
    entries.sort();
    for crate_dir in entries {
        let source = crate_dir.join("tuning");
        if !source.is_dir() {
            continue;
        }
        println!("cargo:rerun-if-changed={}", source.display());
        let name = crate_dir.file_name().expect("a crate directory has a name");
        let target = staged.join(name);
        fs::create_dir_all(&target)
            .unwrap_or_else(|e| panic!("tuning: could not create {}: {e}", target.display()));
        for file in fs::read_dir(&source)
            .unwrap_or_else(|e| panic!("tuning: could not list {}: {e}", source.display()))
        {
            let file = file.expect("tuning: a tuning directory entry").path();
            if file
                .extension()
                .is_some_and(|extension| extension == "toml")
            {
                let to = target.join(file.file_name().expect("a file has a name"));
                fs::copy(&file, &to).unwrap_or_else(|e| {
                    panic!(
                        "tuning: could not copy {} to {}: {e}",
                        file.display(),
                        to.display()
                    )
                });
            }
        }
    }
}
