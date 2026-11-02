mod model;
mod owner;
mod parse;
pub use model::*;

use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
struct Budgets {
    work: Duration,
    cleanup: Duration,
    eof_grace: Duration,
    term_grace: Duration,
    tick: Duration,
    capture: usize,
}
const BUDGETS: Budgets = Budgets {
    work: Duration::from_millis(3000),
    cleanup: Duration::from_millis(1000),
    eof_grace: Duration::from_millis(50),
    term_grace: Duration::from_millis(200),
    tick: Duration::from_millis(5),
    capture: 16384,
};

fn bounded_message(mut message: String) -> String {
    if message.len() > 2048 {
        let mut end = 2048;
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
    }
    message
}
fn failure(
    code: EngineProbeErrorCode,
    step: Option<ProbeStep>,
    message: impl Into<String>,
    os_code: Option<i32>,
) -> EngineProbeError {
    EngineProbeError {
        code,
        step,
        message: bounded_message(message.into()),
        os_code,
        partial_report: None,
    }
}
fn path_failure(error: io::Error, step: Option<ProbeStep>) -> EngineProbeError {
    let code = match error.kind() {
        io::ErrorKind::NotFound => EngineProbeErrorCode::EngineNotFound,
        io::ErrorKind::NotADirectory => EngineProbeErrorCode::InvalidEnginePath,
        io::ErrorKind::PermissionDenied => EngineProbeErrorCode::PermissionDenied,
        _ => EngineProbeErrorCode::IoFailed,
    };
    failure(code, step, error.to_string(), error.raw_os_error())
}
#[derive(PartialEq, Eq)]
struct Identity {
    dev: u64,
    ino: u64,
    size: u64,
    modified: i64,
    modified_ns: i64,
}
impl From<fs::Metadata> for Identity {
    fn from(meta: fs::Metadata) -> Self {
        Self {
            dev: meta.dev(),
            ino: meta.ino(),
            size: meta.len(),
            modified: meta.mtime(),
            modified_ns: meta.mtime_nsec(),
        }
    }
}
fn resolve(requested: &str) -> Result<(PathBuf, Identity), EngineProbeError> {
    if requested.is_empty()
        || requested.len() > 4096
        || requested.contains('\0')
        || !Path::new(requested).is_absolute()
    {
        return Err(failure(
            EngineProbeErrorCode::InvalidEnginePath,
            None,
            "expected an absolute UTF-8 engine path within 1..4096 bytes and without NUL",
            None,
        ));
    }
    let resolved = fs::canonicalize(requested).map_err(|error| path_failure(error, None))?;
    if resolved.to_str().is_none() {
        return Err(failure(
            EngineProbeErrorCode::InvalidEnginePath,
            None,
            "resolved engine path is not UTF-8",
            None,
        ));
    }
    let metadata = fs::metadata(&resolved).map_err(|error| path_failure(error, None))?;
    if !metadata.is_file() {
        return Err(failure(
            EngineProbeErrorCode::InvalidEnginePath,
            None,
            "engine must be a regular file",
            None,
        ));
    }
    Ok((resolved, metadata.into()))
}
fn recheck(
    requested: &str,
    resolved: &Path,
    identity: &Identity,
    step: ProbeStep,
) -> Result<(), EngineProbeError> {
    let (current, current_identity) = resolve(requested).map_err(|mut error| {
        error.step = Some(step);
        if matches!(
            error.code,
            EngineProbeErrorCode::EngineNotFound | EngineProbeErrorCode::InvalidEnginePath
        ) {
            error.code = EngineProbeErrorCode::EngineChanged;
        }
        error
    })?;
    if current != resolved || &current_identity != identity {
        return Err(failure(
            EngineProbeErrorCode::EngineChanged,
            Some(step),
            "engine identity changed since preflight",
            None,
        ));
    }
    Ok(())
}
fn finish(report: &mut EngineProbeReport, started: Instant) {
    report.elapsed_ms = millis(started.elapsed());
    report.identity = parse::identity(&report.commands);
    report.capabilities = parse::capabilities(&report.identity, &report.commands);
}
fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Inspect only --version and --help. This does not authorize or perform transfers.
pub fn inspect_engine(executable: &str) -> Result<EngineProbeReport, EngineProbeError> {
    inspect_with_budgets(executable, BUDGETS)
}
fn inspect_with_budgets(
    executable: &str,
    budgets: Budgets,
) -> Result<EngineProbeReport, EngineProbeError> {
    let started = Instant::now();
    let (resolved, identity) = resolve(executable)?;
    // resolve has checked representability; avoid an unchecked conversion here.
    let resolved_string = resolved
        .to_str()
        .ok_or_else(|| {
            failure(
                EngineProbeErrorCode::InvalidEnginePath,
                None,
                "resolved path is not UTF-8",
                None,
            )
        })?
        .to_owned();
    let mut report = EngineProbeReport {
        schema_version: 1,
        contract_version: "engine-probe.v1".into(),
        requested_executable: executable.into(),
        resolved_executable: resolved_string,
        identity: parse::empty_identity(),
        capabilities: Vec::new(),
        commands: Vec::new(),
        elapsed_ms: 0,
    };
    let deadline = Instant::now() + budgets.work;
    let mut nonzero = None;
    for step in [ProbeStep::Version, ProbeStep::Help] {
        let preparation = recheck(executable, &resolved, &identity, step);
        let next = preparation.and_then(|()| {
            if Instant::now() >= deadline {
                Err(failure(
                    EngineProbeErrorCode::DeadlineExceeded,
                    Some(step),
                    "shared probe deadline expired before spawn",
                    None,
                ))
            } else {
                owner::observe(&resolved, step, deadline, budgets)
            }
        });
        let (observation, error) = match next {
            Ok(result) => result,
            Err(mut error) => {
                finish(&mut report, started);
                error.partial_report = Some(Box::new(report));
                return Err(error);
            }
        };
        report.commands.push(observation);
        if let Some(mut error) = error {
            if error.code == EngineProbeErrorCode::NonZeroExit && step == ProbeStep::Version {
                nonzero = Some(error);
            } else {
                finish(&mut report, started);
                error.partial_report = Some(Box::new(report));
                return Err(error);
            }
        }
    }
    finish(&mut report, started);
    if let Some(mut error) = nonzero {
        error.partial_report = Some(Box::new(report));
        Err(error)
    } else {
        Ok(report)
    }
}

#[cfg(test)]
mod tests;
