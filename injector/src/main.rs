//! The injector: loads the payload into a running Just Cause 3 and then, unless told not to, stays
//! running as the capture host that encodes the payload's recordings ([`capture`]).
//!
//! Usage: `jc3vr_injector [--eject | --reload] [--source-tuning] [--no-capture-host]
//! [--capture-port PORT]`.

mod capture;

use std::{
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    ptr::NonNull,
    time::{Duration, Instant},
};

use anyhow::Context;
use re_utilities_injector as injector;
use windows::{
    Win32::{
        Foundation::HANDLE,
        System::{
            Diagnostics::{
                Debug::ReadProcessMemory,
                ToolHelp::{
                    CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW,
                    TH32CS_SNAPMODULE,
                },
            },
            Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
        },
    },
    core::Owned,
};

/// The export the injector calls to ask a loaded payload to shut itself down.
const EJECT_EXPORT: &str = "eject";

/// The export the injector calls to start a freshly injected payload.
const RUN_EXPORT: &str = "run";

/// The export that starts the payload reading its tuning from the source tree it was built from
/// rather than from beside the DLL (`docs/mod/tuning.md`).
const RUN_SOURCE_TUNING_EXPORT: &str = "run_source_tuning";

/// How long the injector waits for a requested ejection to unmap the payload.
const UNLOAD_TIMEOUT: Duration = Duration::from_secs(30);

/// How often the injector re-checks the module list while waiting for the payload to unload.
const UNLOAD_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// How long the injector keeps watching the target after the payload's image leaves it.
///
/// The teardown runs on a game thread, and the module leaves the list before the process settles,
/// so a crash caused by that teardown lands shortly after the image is gone. This window only
/// makes the exit visible while the injector is still watching; it says nothing about the
/// process's later lifetime.
const UNLOAD_SETTLE: Duration = Duration::from_millis(250);

/// `STILL_ACTIVE`, the exit code `GetExitCodeProcess` reports for a running process.
const STILL_ACTIVE: u32 = 259;

/// How long a remote export call may take before the injector gives up on it.
const REMOTE_EXPORT_TIMEOUT: Duration = Duration::from_secs(10);

/// The bytes of a PE DOS header that carry the offset of the NT headers.
const DOS_HEADER_LEN: usize = 0x40;

/// Enough of a PE NT header to reach `SizeOfImage`.
const NT_HEADER_LEN: usize = 0x60;

/// How many times a stale module snapshot is retaken before the lookup gives up.
const MODULE_SNAPSHOT_ATTEMPTS: u32 = 3;

/// `ERROR_NO_MORE_FILES`, the end of a `Module32NextW` walk.
const ERROR_NO_MORE_FILES: u32 = 18;

/// `ERROR_BAD_LENGTH`, reported when the module list changed while it was snapshotted.
const ERROR_BAD_LENGTH: u32 = 24;

/// `ERROR_SHARING_VIOLATION`, reported when a file another handle holds is replaced.
const ERROR_SHARING_VIOLATION: i32 = 32;

/// How long the injector retries a sharing violation on the payload's loaded copy.
const SHARING_RETRY_TIMEOUT: Duration = Duration::from_secs(2);

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let options = Options::parse(std::env::args().skip(1))?;

    let payload_name = "jc3vr_payload.dll".to_string();
    let file_name = "JustCause3.exe".to_string();

    let payload_path = std::env::current_exe()?
        .parent()
        .context("injector: could not find the injector's directory")?
        .join(payload_name);
    let loaded_path = loaded_payload_path(&payload_path)
        .context("injector: the payload's path has no file stem and extension")?;

    let mut processes = injector::get_processes_by_name(&file_name)?;
    if processes.is_empty() {
        return Err(anyhow::anyhow!("injector: no {file_name} process found"));
    }
    let (pid, handle) = processes.remove(0);

    let loaded_module = remote_module_base(pid, &loaded_path)?;
    let plan = plan(options.action, loaded_module.is_some());

    if plan == Plan::AlreadyLoaded {
        return Err(anyhow::anyhow!(
            "injector: the payload is already loaded in process {pid}; pass --reload to eject and \
             inject it again, or --eject to unload it"
        ));
    }
    if plan == Plan::NothingLoaded {
        tracing::info!("injector: the payload is not loaded in process {pid}; nothing to eject");
        return Ok(());
    }
    if matches!(plan, Plan::EjectThenStop | Plan::EjectThenInject) {
        if let Some(module) = &loaded_module {
            eject_and_wait(*handle, module, pid, &loaded_path)?;
        }
        if plan == Plan::EjectThenStop {
            tracing::info!("injector: ejected the payload from process {pid}");
            return Ok(());
        }
    }

    tracing::info!("injector: injecting into process {pid}");
    let payload_path = inject_retrying_sharing(*handle, &payload_path)?;

    tracing::info!("injector: running the payload");
    let payload_base = injector::get_remote_module_base(pid, &payload_path)
        .context("injector: could not find the payload's base")?
        .context("injector: the payload's base is null")?;
    let run_export = if options.source_tuning {
        RUN_SOURCE_TUNING_EXPORT
    } else {
        RUN_EXPORT
    };
    injector::call_remote_export(
        *handle,
        payload_base,
        run_export,
        Some(REMOTE_EXPORT_TIMEOUT),
    )
    .context("injector: could not run the payload")?;
    drop(handle);
    // A payload that cannot start (a tuning file missing or invalid) unloads itself before the
    // call returns.
    if remote_module_base(pid, &loaded_path)?.is_none() {
        anyhow::bail!(
            "injector: the payload stopped its startup and unloaded itself; the session log says \
             why"
        );
    }

    if options.capture_host {
        capture::serve(options.capture_port, pid)?;
    }
    Ok(())
}

/// Injects the payload, retrying while the copy to its loaded name meets a sharing violation.
///
/// Right after an ejection, Wine can keep the previous image's file open for a moment after the
/// module has left the module list, and the copy that replaces it fails until it lets go.
fn inject_retrying_sharing(handle: HANDLE, payload_path: &Path) -> anyhow::Result<PathBuf> {
    let started = Instant::now();
    loop {
        match injector::inject(handle, payload_path) {
            Ok(path) => return Ok(path),
            Err(injector::Error::Io { source, .. })
                if source.raw_os_error() == Some(ERROR_SHARING_VIOLATION)
                    && started.elapsed() < SHARING_RETRY_TIMEOUT =>
            {
                tracing::info!("injector: the loaded payload's file is still in use; retrying");
                std::thread::sleep(UNLOAD_POLL_INTERVAL);
            }
            Err(error) => return Err(error).context("injector: could not inject"),
        }
    }
}

/// Asks the loaded payload to shut down and waits for its image to leave the process.
///
/// The request is only a signal: the game thread performs the teardown, and the module stays mapped
/// until that thread finishes. The injector never forces the unload itself, because a pinned module
/// has live threads the process would fault on.
fn eject_and_wait(
    handle: HANDLE,
    module: &RemoteModule,
    pid: u32,
    loaded_path: &Path,
) -> anyhow::Result<()> {
    verify_mapped_image(handle, module, pid)?;
    let observer = open_process_observer(pid)?;

    tracing::info!("injector: requesting ejection from process {pid}");
    if let Err(error) = injector::call_remote_export(
        handle,
        module.base,
        EJECT_EXPORT,
        Some(REMOTE_EXPORT_TIMEOUT),
    ) {
        // The export is resolved from the mapped image's own file, so an absent export means the
        // mapped build predates it: the game's own eject path is the only remaining way out.
        if matches!(
            &error,
            injector::Error::Windows(injector::WindowsError::ExportNotFound { .. })
        ) {
            return Err(anyhow::anyhow!(
                "injector: the payload loaded in process {pid} has no {EJECT_EXPORT} export; \
                 press F5 in game to eject it, then run the injector again"
            ));
        }
        return Err(anyhow::anyhow!(
            "injector: could not request the payload's ejection: {error}"
        ));
    }

    tracing::info!("injector: waiting for the payload to unload");
    wait_until_unloaded(
        || {
            if let ProcessStatus::Exited(code) = process_status(*observer)? {
                return Err(process_exited(
                    pid,
                    code,
                    "while waiting for the payload to unload",
                ));
            }
            remote_module_base(pid, loaded_path).map(|module| module.is_some())
        },
        UNLOAD_TIMEOUT,
        UNLOAD_POLL_INTERVAL,
    )?;

    std::thread::sleep(UNLOAD_SETTLE);
    match process_status(*observer)? {
        ProcessStatus::Alive => {
            tracing::info!("injector: process {pid} is still alive after the payload unloaded");
            Ok(())
        }
        ProcessStatus::Exited(code) => Err(process_exited(pid, code, "after the payload unloaded")),
    }
}

/// Whether the target process is still running, and its exit code once it is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessStatus {
    /// The process is still running.
    Alive,
    /// The process has exited with this code.
    Exited(u32),
}

fn classify_process_status(exit_code: u32) -> ProcessStatus {
    if exit_code == STILL_ACTIVE {
        ProcessStatus::Alive
    } else {
        ProcessStatus::Exited(exit_code)
    }
}

/// Opens a handle that survives the target's exit, so its exit code stays readable.
fn open_process_observer(pid: u32) -> anyhow::Result<Owned<HANDLE>> {
    // SAFETY: the handle is owned and closed on drop; the access right is the least that
    // `GetExitCodeProcess` accepts.
    unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
        .with_context(|| format!("injector: could not open process {pid} to watch its exit status"))
        .map(|handle| unsafe { Owned::new(handle) })
}

fn process_status(handle: HANDLE) -> anyhow::Result<ProcessStatus> {
    let mut exit_code = 0u32;
    // SAFETY: `handle` is a live process handle with query access, and `exit_code` is a valid
    // out-parameter.
    unsafe { GetExitCodeProcess(handle, &mut exit_code) }
        .context("injector: could not read the target's exit code")?;
    Ok(classify_process_status(exit_code))
}

fn process_exited(pid: u32, code: u32, when: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "injector: process {pid} exited with code 0x{code:08X} {when}; the payload's absence from \
         the module list is not evidence of a clean teardown"
    )
}

/// A module mapped into the target process, with the path the loader recorded for it.
struct RemoteModule {
    base: NonNull<u8>,
    path: PathBuf,
}

/// Resolves the module mapped under `loaded_path`'s name, or `None` if nothing is mapped.
///
/// The module list is authoritative even when the file the image was loaded from is gone, which a
/// path lookup cannot prove: it canonicalizes the path, so a deleted file reads as "not loaded".
fn remote_module_base(pid: u32, loaded_path: &Path) -> anyhow::Result<Option<RemoteModule>> {
    let loaded_name = loaded_path
        .file_name()
        .context("injector: the payload's loaded path has no file name")?
        .to_string_lossy();

    for _ in 0..MODULE_SNAPSHOT_ATTEMPTS {
        match find_remote_module(pid, &loaded_name)? {
            Lookup::Found(module) => return Ok(Some(module)),
            Lookup::Absent => return Ok(None),
            Lookup::Stale => {}
        }
    }
    Err(anyhow::anyhow!(
        "injector: the module list of process {pid} kept changing while it was read"
    ))
}

/// What one pass over the module list found.
enum Lookup {
    /// The payload is mapped here.
    Found(RemoteModule),
    /// The walk reached the end of the list without a match.
    Absent,
    /// The snapshot went stale mid-read, so it must be retaken.
    Stale,
}

/// One pass over the module list.
///
/// Only an exhausted walk counts as absence: any other enumeration failure is propagated, because
/// a list that could not be read says nothing about what is mapped.
fn find_remote_module(pid: u32, loaded_name: &str) -> anyhow::Result<Lookup> {
    // SAFETY: the snapshot handle is owned and closed on drop; each entry is a plain copy filled in
    // by the API before it is read.
    unsafe {
        let snapshot = match CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, pid) {
            Ok(snapshot) => Owned::new(snapshot),
            Err(error) => return module_list_failure(pid, error),
        };
        let mut entry = MODULEENTRY32W {
            dwSize: std::mem::size_of::<MODULEENTRY32W>() as u32,
            ..Default::default()
        };
        if let Err(error) = Module32FirstW(*snapshot, &mut entry) {
            return module_list_failure(pid, error);
        }
        loop {
            if let Some(module) = matching_module(&entry, pid, loaded_name)? {
                return Ok(Lookup::Found(module));
            }
            if let Err(error) = Module32NextW(*snapshot, &mut entry) {
                return match classify_module_list_error(error.code().0) {
                    ModuleListError::End => Ok(Lookup::Absent),
                    ModuleListError::Stale => Ok(Lookup::Stale),
                    ModuleListError::Other => Err(anyhow::Error::new(error).context(format!(
                        "injector: could not enumerate the modules of process {pid}"
                    ))),
                };
            }
        }
    }
}

/// Maps a snapshot failure to a retry or an error, never to an empty list.
fn module_list_failure(pid: u32, error: windows::core::Error) -> anyhow::Result<Lookup> {
    match classify_module_list_error(error.code().0) {
        ModuleListError::Stale => Ok(Lookup::Stale),
        ModuleListError::End | ModuleListError::Other => Err(anyhow::Error::new(error).context(
            format!("injector: could not read the modules of process {pid}"),
        )),
    }
}

fn matching_module(
    entry: &MODULEENTRY32W,
    pid: u32,
    loaded_name: &str,
) -> anyhow::Result<Option<RemoteModule>> {
    let len = entry
        .szModule
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(entry.szModule.len());
    if !String::from_utf16_lossy(&entry.szModule[..len]).eq_ignore_ascii_case(loaded_name) {
        return Ok(None);
    }
    let Some(base) = NonNull::new(entry.modBaseAddr) else {
        anyhow::bail!(
            "injector: the module list of process {pid} has {loaded_name} without a base address"
        );
    };
    let path_len = entry
        .szExePath
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(entry.szExePath.len());
    let path = PathBuf::from(String::from_utf16_lossy(&entry.szExePath[..path_len]));
    Ok(Some(RemoteModule { base, path }))
}

/// How a failure from the module enumeration is treated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModuleListError {
    /// The walk reached the end of the list.
    End,
    /// The snapshot changed while it was read and must be retaken.
    Stale,
    /// Any other failure.
    Other,
}

/// Classifies a `windows` error code from the module enumeration.
fn classify_module_list_error(code: i32) -> ModuleListError {
    let code = code as u32;
    // Win32 errors surface as `HRESULT_FROM_WIN32`: `0x8007_0000 | code`.
    if code & 0xFFFF_0000 != 0x8007_0000 {
        return ModuleListError::Other;
    }
    match code & 0xFFFF {
        ERROR_NO_MORE_FILES => ModuleListError::End,
        ERROR_BAD_LENGTH => ModuleListError::Stale,
        _ => ModuleListError::Other,
    }
}

/// Fails unless the image mapped at `module` is the build on disk at the path the loader recorded.
///
/// Remote exports are resolved from that file, so a file replaced under a live image would send the
/// call to an address that means something else in the mapped build.
fn verify_mapped_image(handle: HANDLE, module: &RemoteModule, pid: u32) -> anyhow::Result<()> {
    let local = local_pe_identity(&module.path)?;
    let remote = remote_pe_identity(handle, module.base)?;
    if local.is_some() && local == remote {
        return Ok(());
    }
    Err(anyhow::anyhow!(
        "injector: the payload mapped in process {pid} is not the build at {}; press F5 in game \
         to eject it, then run the injector again (the injector does not call into a mismatched \
         image)",
        module.path.display()
    ))
}

/// The fields that identify a PE build, compared between the mapped image and its file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PeIdentity {
    timestamp: u32,
    size_of_image: u32,
}

fn local_pe_identity(path: &Path) -> anyhow::Result<Option<PeIdentity>> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("injector: could not open {}", path.display()))?;
    let mut dos = [0u8; DOS_HEADER_LEN];
    if file.read_exact(&mut dos).is_err() {
        return Ok(None);
    }
    let Some(nt_offset) = nt_offset(&dos) else {
        return Ok(None);
    };
    if file.seek(SeekFrom::Start(nt_offset as u64)).is_err() {
        return Ok(None);
    }
    let mut nt = [0u8; NT_HEADER_LEN];
    if file.read_exact(&mut nt).is_err() {
        return Ok(None);
    }
    Ok(pe_identity(&nt))
}

fn remote_pe_identity(handle: HANDLE, base: NonNull<u8>) -> anyhow::Result<Option<PeIdentity>> {
    let base = base.as_ptr();
    let mut dos = [0u8; DOS_HEADER_LEN];
    read_remote(handle, base, &mut dos)?;
    let Some(nt_offset) = nt_offset(&dos) else {
        return Ok(None);
    };
    let mut nt = [0u8; NT_HEADER_LEN];
    read_remote(handle, base.wrapping_add(nt_offset), &mut nt)?;
    Ok(pe_identity(&nt))
}

fn read_remote(handle: HANDLE, address: *const u8, buffer: &mut [u8]) -> anyhow::Result<()> {
    // SAFETY: the buffer is writable for its whole length, and the API reports an unreadable source
    // range as an error instead of writing past the buffer.
    unsafe {
        ReadProcessMemory(
            handle,
            address.cast(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            None,
        )
        .with_context(|| format!("injector: could not read process memory at {address:p}"))?;
    }
    Ok(())
}

fn nt_offset(dos: &[u8]) -> Option<usize> {
    Some(u32::from_le_bytes(dos.get(0x3C..0x40)?.try_into().ok()?) as usize)
}

fn pe_identity(nt: &[u8]) -> Option<PeIdentity> {
    if nt.get(..4)? != b"PE\0\0" {
        return None;
    }
    Some(PeIdentity {
        timestamp: u32::from_le_bytes(nt.get(8..12)?.try_into().ok()?),
        // `SizeOfImage` sits at the same offset of the optional header in PE32 and PE32+.
        size_of_image: u32::from_le_bytes(nt.get(80..84)?.try_into().ok()?),
    })
}

/// The path the loader maps the payload from: the injector's copy under a `_loaded` name, which is
/// what a reload can replace while the running image keeps its file.
fn loaded_payload_path(payload_path: &Path) -> Option<PathBuf> {
    let stem = payload_path.file_stem()?.to_str()?;
    let extension = payload_path.extension()?.to_str()?;
    Some(payload_path.with_file_name(format!("{stem}_loaded.{extension}")))
}

/// Polls `is_loaded` until the module is absent, or the timeout expires.
fn wait_until_unloaded(
    mut is_loaded: impl FnMut() -> anyhow::Result<bool>,
    timeout: Duration,
    interval: Duration,
) -> anyhow::Result<()> {
    let started = Instant::now();
    loop {
        if !is_loaded()? {
            return Ok(());
        }
        if started.elapsed() >= timeout {
            return Err(anyhow::anyhow!(
                "injector: the payload is still loaded {timeout:?} after the eject request; \
                 restart the game before injecting again (the injector does not force the unload)"
            ));
        }
        std::thread::sleep(interval);
    }
}

/// The command line.
#[derive(Debug, PartialEq, Eq)]
struct Options {
    /// What to do with the payload.
    action: Action,
    /// Start the payload reading its tuning from the source tree it was built from.
    source_tuning: bool,
    /// Stay running as the capture host after the injection.
    capture_host: bool,
    capture_port: u16,
}

impl Options {
    fn parse(mut args: impl Iterator<Item = String>) -> anyhow::Result<Self> {
        let mut options = Self {
            action: Action::Inject,
            source_tuning: false,
            capture_host: true,
            capture_port: capture::DEFAULT_PORT,
        };
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--eject" => options.action = choose_action(options.action, Action::Eject)?,
                "--reload" => options.action = choose_action(options.action, Action::Reload)?,
                "--source-tuning" => options.source_tuning = true,
                "--no-capture-host" => options.capture_host = false,
                "--capture-port" => {
                    let port = args
                        .next()
                        .context("injector: --capture-port needs a port")?;
                    options.capture_port = port
                        .parse()
                        .with_context(|| format!("injector: {port:?} is not a port"))?;
                }
                other => anyhow::bail!(
                    "injector: unknown argument {other:?}; usage: jc3vr_injector \
                     [--eject | --reload] [--source-tuning] [--no-capture-host] \
                     [--capture-port PORT]"
                ),
            }
        }
        Ok(options)
    }
}

/// The action an `--eject` or `--reload` flag asks for, rejecting a conflicting one.
fn choose_action(current: Action, requested: Action) -> anyhow::Result<Action> {
    if current == requested {
        return Ok(current);
    }
    if current != Action::Inject {
        anyhow::bail!("injector: --eject and --reload are mutually exclusive");
    }
    Ok(requested)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// Inject the payload, refusing if it is already loaded.
    Inject,
    /// Eject the payload, if it is loaded.
    Eject,
    /// Eject the payload if it is loaded, then inject it again.
    Reload,
}

/// What the injector does once it knows whether the payload is loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Plan {
    /// Nothing is loaded; inject.
    Inject,
    /// Something is loaded and a plain inject would collide with it.
    AlreadyLoaded,
    /// An eject was asked for and nothing is loaded.
    NothingLoaded,
    /// Eject and stop there.
    EjectThenStop,
    /// Eject, wait for the unload, then inject.
    EjectThenInject,
}

/// Decides the injector's course from the requested action and whether the payload is loaded.
fn plan(action: Action, loaded: bool) -> Plan {
    match (action, loaded) {
        (Action::Inject, false) | (Action::Reload, false) => Plan::Inject,
        (Action::Inject, true) => Plan::AlreadyLoaded,
        (Action::Eject, false) => Plan::NothingLoaded,
        (Action::Eject, true) => Plan::EjectThenStop,
        (Action::Reload, true) => Plan::EjectThenInject,
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn parse(args: &[&str]) -> anyhow::Result<Options> {
        Options::parse(args.iter().map(|a| (*a).to_owned()))
    }

    #[test]
    fn the_capture_host_is_on_by_default_and_can_be_moved_or_turned_off() {
        assert_eq!(
            parse(&[]).unwrap(),
            Options {
                action: Action::Inject,
                source_tuning: false,
                capture_host: true,
                capture_port: 47172
            }
        );
        assert_eq!(
            parse(&["--no-capture-host", "--capture-port", "5000"]).unwrap(),
            Options {
                action: Action::Inject,
                source_tuning: false,
                capture_host: false,
                capture_port: 5000
            }
        );
        assert!(parse(&["--capture-port"]).is_err());
        assert!(parse(&["--capture-port", "big"]).is_err());
        assert!(parse(&["--frobnicate"]).is_err());
    }

    #[test]
    fn the_tuning_is_read_beside_the_payload_unless_the_source_tree_is_asked_for() {
        assert!(!parse(&[]).unwrap().source_tuning);
        assert!(
            parse(&["--reload", "--source-tuning"])
                .unwrap()
                .source_tuning
        );
    }

    #[test]
    fn the_eject_and_reload_flags_select_the_action_and_exclude_each_other() {
        assert_eq!(parse(&["--eject"]).unwrap().action, Action::Eject);
        assert_eq!(parse(&["--reload"]).unwrap().action, Action::Reload);
        assert_eq!(
            parse(&["--eject", "--eject"]).unwrap().action,
            Action::Eject
        );
        assert_eq!(
            parse(&["--reload", "--eject", "--no-capture-host"])
                .unwrap_err()
                .to_string(),
            "injector: --eject and --reload are mutually exclusive"
        );
    }

    #[test]
    fn the_plan_covers_every_action_and_loaded_state() {
        assert_eq!(plan(Action::Inject, false), Plan::Inject);
        assert_eq!(plan(Action::Inject, true), Plan::AlreadyLoaded);
        assert_eq!(plan(Action::Eject, false), Plan::NothingLoaded);
        assert_eq!(plan(Action::Eject, true), Plan::EjectThenStop);
        assert_eq!(plan(Action::Reload, false), Plan::Inject);
        assert_eq!(plan(Action::Reload, true), Plan::EjectThenInject);
    }

    #[test]
    fn the_process_status_tells_a_running_process_from_an_exited_one() {
        assert_eq!(classify_process_status(STILL_ACTIVE), ProcessStatus::Alive);
        assert_eq!(classify_process_status(0), ProcessStatus::Exited(0));
        assert_eq!(
            classify_process_status(0xC000_0005),
            ProcessStatus::Exited(0xC000_0005)
        );
        let message = process_exited(42, 0xC000_0005, "after the payload unloaded").to_string();
        assert!(message.contains("0xC0000005"), "{message}");
    }

    #[test]
    fn the_unload_wait_returns_once_the_module_is_absent() {
        let probes = std::cell::Cell::new(0);
        wait_until_unloaded(
            || {
                probes.set(probes.get() + 1);
                Ok(probes.get() < 3)
            },
            Duration::from_secs(5),
            Duration::ZERO,
        )
        .unwrap();
        assert_eq!(probes.get(), 3);
    }

    #[test]
    fn the_unload_wait_reports_the_timeout_instead_of_forcing_the_unload() {
        let error = wait_until_unloaded(|| Ok(true), Duration::ZERO, Duration::ZERO).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("still loaded"), "{message}");
        assert!(message.contains("restart the game"), "{message}");
    }

    #[test]
    fn the_loaded_copy_is_named_after_the_payload() {
        assert_eq!(
            loaded_payload_path(Path::new("/mods/jc3vr_payload.dll")).unwrap(),
            PathBuf::from("/mods/jc3vr_payload_loaded.dll")
        );
        assert_eq!(loaded_payload_path(Path::new("/mods/payload")), None);
    }

    #[test]
    fn the_pe_identity_reads_the_timestamp_and_image_size() {
        let mut nt = [0u8; NT_HEADER_LEN];
        nt[..4].copy_from_slice(b"PE\0\0");
        nt[8..12].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        nt[80..84].copy_from_slice(&0x00AB_0000u32.to_le_bytes());
        assert_eq!(
            pe_identity(&nt),
            Some(PeIdentity {
                timestamp: 0x1234_5678,
                size_of_image: 0x00AB_0000
            })
        );
        assert_eq!(pe_identity(&nt[..4]), None);
        nt[0] = b'M';
        assert_eq!(pe_identity(&nt), None);
    }

    #[test]
    fn the_nt_header_offset_comes_from_the_dos_header_and_missing_bytes_are_rejected() {
        let mut dos = [0u8; DOS_HEADER_LEN];
        dos[0x3C..0x40].copy_from_slice(&0x100u32.to_le_bytes());
        assert_eq!(nt_offset(&dos), Some(0x100));
        assert_eq!(nt_offset(&dos[..0x3C]), None);
    }

    #[test]
    fn only_an_exhausted_walk_is_absence_and_a_stale_snapshot_is_retried() {
        let hresult = |win32: u32| (0x8007_0000u32 | win32) as i32;
        assert_eq!(
            classify_module_list_error(hresult(ERROR_NO_MORE_FILES)),
            ModuleListError::End
        );
        assert_eq!(
            classify_module_list_error(hresult(ERROR_BAD_LENGTH)),
            ModuleListError::Stale
        );
        assert_eq!(
            classify_module_list_error(hresult(5)),
            ModuleListError::Other
        );
        assert_eq!(
            classify_module_list_error(ERROR_NO_MORE_FILES as i32),
            ModuleListError::Other
        );
    }
}
