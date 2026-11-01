use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::ExitCode;

use kernel::{InvocationPlan, PlanningError, PlanningErrorCode};

const MAX_JSON_BYTES: usize = 64 * 1024;

fn error(code: PlanningErrorCode, field: &str, message: impl Into<String>) -> PlanningError {
    PlanningError {
        code,
        field: Some(field.into()),
        message: message.into(),
        os_code: None,
    }
}

fn task_file_error(failure: io::Error) -> PlanningError {
    let code = match failure.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory => PlanningErrorCode::InvalidPath,
        io::ErrorKind::PermissionDenied => PlanningErrorCode::PathAccessDenied,
        _ => PlanningErrorCode::PathIo,
    };
    PlanningError {
        code,
        field: Some("taskFile".into()),
        message: failure.to_string(),
        os_code: failure.raw_os_error(),
    }
}

fn plan_file(path: &Path) -> Result<InvocationPlan, PlanningError> {
    let metadata = fs::metadata(path).map_err(task_file_error)?;
    if !metadata.is_file() {
        return Err(error(
            PlanningErrorCode::InvalidPath,
            "taskFile",
            "task file must be a regular file",
        ));
    }
    let file = File::open(path).map_err(task_file_error)?;
    let mut bytes = Vec::new();
    file.take((MAX_JSON_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(task_file_error)?;
    if bytes.len() > MAX_JSON_BYTES {
        return Err(error(
            PlanningErrorCode::InvalidTask,
            "json",
            "task JSON exceeds 64 KiB",
        ));
    }
    let json = std::str::from_utf8(&bytes).map_err(|failure| {
        error(
            PlanningErrorCode::InvalidJson,
            "taskFile",
            failure.to_string(),
        )
    })?;
    kernel::build_local_plan(kernel::parse_task_json(json)?)
}

fn report_error(failure: PlanningError) -> ExitCode {
    match serde_json::to_string(&failure) {
        Ok(json) => {
            let _ = writeln!(io::stderr().lock(), "{json}");
        }
        Err(_) => {
            let _ = writeln!(
                io::stderr().lock(),
                "{{\"code\":\"invalid_task\",\"field\":null,\"message\":\"error serialization failed\",\"osCode\":null}}"
            );
        }
    }
    ExitCode::from(2)
}

fn setup_health() -> ExitCode {
    let info = kernel::kernel_info();
    let health = match kernel::health().status {
        kernel::HealthStatus::Ready => "ready",
    };
    if writeln!(
        io::stdout().lock(),
        "ctsf-kernel version={} contract={} health={health}",
        info.kernel_version,
        info.contract_version
    )
    .is_err()
    {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let mut arguments = std::env::args_os().skip(1);
    let Some(command) = arguments.next() else {
        return setup_health();
    };
    let path = arguments.next();
    if command != "plan" || path.is_none() || arguments.next().is_some() {
        return report_error(error(
            PlanningErrorCode::InvalidTask,
            "arguments",
            "usage: kernel-probe [plan <task-json-path>]",
        ));
    }
    // The usage check above guarantees the path; no untrusted argument is unwrapped.
    let Some(path) = path else {
        return ExitCode::FAILURE;
    };
    let plan = match plan_file(Path::new(&path)) {
        Ok(plan) => plan,
        Err(failure) => return report_error(failure),
    };
    match serde_json::to_string(&plan) {
        Ok(json) => {
            if writeln!(io::stdout().lock(), "{json}").is_err() {
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        Err(failure) => report_error(error(
            PlanningErrorCode::InvalidTask,
            "plan",
            failure.to_string(),
        )),
    }
}
