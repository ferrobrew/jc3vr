//! Embeds a compile-time build stamp so the running payload can prove which build it is. An
//! uninject can leave the module resident (a hung shutdown thread keeps the DLL mapped), and a
//! failed re-inject then silently continues on the stale code — the stamp is announced at startup
//! (log, in-headset banner, and the debug window) so a stale payload is visible at a glance.

fn main() {
    // Re-stamp whenever any payload source changes, so the stamp tracks the code it was built
    // from rather than the last time this script happened to run. The layer crates the payload is
    // built from count as its source.
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=../crates/jc3vr-core/src");
    println!("cargo:rerun-if-changed=../crates/jc3vr-render/src");
    println!("cargo:rerun-if-changed=../crates/jc3vr-render/shaders");
    println!("cargo:rerun-if-changed=../crates/jc3vr-gameplay/src");
    println!("cargo:rerun-if-changed=../crates/jc3vr-hud/src");
    println!("cargo:rerun-if-changed=../crates/jc3vr-hud/shaders");
    println!("cargo:rerun-if-changed=../crates/jc3vr-hud/assets");
    println!("cargo:rerun-if-changed=../crates/jc3vr-tooling/src");
    println!("cargo:rerun-if-changed=../crates/jc3vr-tooling/shaders");
    println!("cargo:rerun-if-changed=../crates/jc3vr-ui/src");
    let stamp = jiff::Zoned::now().strftime("%Y-%m-%d %H:%M:%S").to_string();
    println!("cargo:rustc-env=jc3vr_build_stamp={stamp}");
}
