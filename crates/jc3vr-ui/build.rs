//! Embeds the newest session's checklist (`docs/sessions/<stamp>/next-session.md`, the stamps
//! sorting by time) for the debug UI's session checklist, so the list in the headset is the one the
//! session's notes hold.

use std::{
    fs,
    path::{Path, PathBuf},
};

fn main() {
    embed_checklist();
}

/// Copy the newest session's checklist into the build's output directory, with its session's
/// stamp, or an empty one when there is none.
fn embed_checklist() {
    let sessions = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/sessions");
    println!("cargo:rerun-if-changed={}", sessions.display());
    let newest = newest_checklist(&sessions);
    let (session, text) = match &newest {
        Some((session, path)) => {
            println!("cargo:rerun-if-changed={}", path.display());
            (
                session.clone(),
                fs::read_to_string(path).expect("the session checklist is readable"),
            )
        }
        None => (String::new(), String::new()),
    };
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    fs::write(out.join("next-session.md"), text).expect("the build's output is writable");
    println!("cargo:rustc-env=jc3vr_checklist_session={session}");
}

/// The newest session directory holding a `next-session.md`, by its stamp, and that file.
fn newest_checklist(sessions: &Path) -> Option<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = fs::read_dir(sessions)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path().join("next-session.md");
            let name = entry.file_name().into_string().ok()?;
            path.is_file().then_some((name, path))
        })
        .collect();
    found.sort();
    found.pop()
}
