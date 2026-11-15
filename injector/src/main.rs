//! The injector: loads the payload into a running Just Cause 3 and then, unless told not to, stays
//! running as the capture host that encodes the payload's recordings ([`capture`]).
//!
//! Usage: `jc3vr_injector [--no-capture-host] [--capture-port PORT]`.

mod capture;

use anyhow::Context;
use re_utilities_injector as injector;

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

    let mut processes = injector::get_processes_by_name(&file_name)?;
    if processes.is_empty() {
        return Err(anyhow::anyhow!("injector: no {file_name} process found"));
    }
    let (pid, handle) = processes.remove(0);

    tracing::info!("injector: injecting into process {pid}");
    let payload_path =
        injector::inject(*handle, &payload_path).context("injector: could not inject")?;

    tracing::info!("injector: running the payload");
    let payload_base = injector::get_remote_module_base(pid, &payload_path)
        .context("injector: could not find the payload's base")?
        .context("injector: the payload's base is null")?;
    injector::call_remote_export(
        *handle,
        payload_base,
        "run",
        Some(std::time::Duration::from_secs(10)),
    )
    .context("injector: could not run the payload")?;
    drop(handle);

    if options.capture_host {
        capture::serve(options.capture_port, pid)?;
    }
    Ok(())
}

/// The command line.
#[derive(Debug, PartialEq, Eq)]
struct Options {
    /// Stay running as the capture host after the injection.
    capture_host: bool,
    capture_port: u16,
}

impl Options {
    fn parse(mut args: impl Iterator<Item = String>) -> anyhow::Result<Self> {
        let mut options = Self {
            capture_host: true,
            capture_port: capture::DEFAULT_PORT,
        };
        while let Some(arg) = args.next() {
            match arg.as_str() {
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
                     [--no-capture-host] [--capture-port PORT]"
                ),
            }
        }
        Ok(options)
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
                capture_host: true,
                capture_port: 47172
            }
        );
        assert_eq!(
            parse(&["--no-capture-host", "--capture-port", "5000"]).unwrap(),
            Options {
                capture_host: false,
                capture_port: 5000
            }
        );
        assert!(parse(&["--capture-port"]).is_err());
        assert!(parse(&["--capture-port", "big"]).is_err());
        assert!(parse(&["--frobnicate"]).is_err());
    }
}
