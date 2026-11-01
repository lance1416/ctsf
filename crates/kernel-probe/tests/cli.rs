use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use kernel::{InvocationPlan, PlanningError, PlanningErrorCode};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    task_file: PathBuf,
    source: PathBuf,
    destination: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        // Exclusive creation avoids sharing another test's or user's directory.
        let root = loop {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("ctsf-probe-test-{}-{sequence}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => break path,
                Err(failure) if failure.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(failure) => panic!("create test directory: {failure}"),
            }
        };
        let source = root.join("源 目录 ' $HOME;* :local");
        let destination = root.join("destination");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&destination).unwrap();
        let task_file = root.join("task 中文.json");
        let fixture = Self {
            root,
            task_file,
            source,
            destination,
        };
        fs::write(&fixture.task_file, fixture.json().to_string()).unwrap();
        fixture
    }

    fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "schemaVersion": 1,
            "kind": "rsync.task.pot",
            "taskId": "probe-fixture",
            "revision": 1,
            "name": "Local planning only",
            "engineExecutable": self.root.join("not-installed-engine"),
            "source": {"raw": format!("{}/", self.source.display())},
            "destination": {"raw": format!("{}/", self.destination.display())},
            "options": [{"kind": "archive"}]
        })
    }

    fn plan(&self) -> Output {
        probe().arg("plan").arg(&self.task_file).output().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn probe() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kernel-probe"))
}

fn assert_failure(output: Output, code: PlanningErrorCode, field: Option<&str>) {
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = std::str::from_utf8(&output.stderr).unwrap();
    assert!(stderr.ends_with('\n'));
    assert_eq!(stderr.lines().count(), 1);
    let failure: PlanningError = serde_json::from_str(stderr).unwrap();
    assert_eq!(failure.code, code);
    if let Some(field) = field {
        assert_eq!(failure.field.as_deref(), Some(field));
    }
}

#[test]
fn no_arguments_keeps_setup_health_output() {
    let output = probe().output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!(
            "ctsf-kernel version={} contract=setup.v1 health=ready\n",
            kernel::kernel_info().kernel_version
        )
    );
}

#[test]
fn plan_emits_one_json_record_and_preserves_files_and_argv() {
    let fixture = Fixture::new();
    fs::write(fixture.source.join("content"), b"source").unwrap();
    fs::write(fixture.destination.join("content"), b"target").unwrap();
    let task_bytes = fs::read(&fixture.task_file).unwrap();
    let output = fixture.plan();
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    assert!(stdout.ends_with('\n'));
    assert_eq!(stdout.lines().count(), 1);
    let plan: InvocationPlan = serde_json::from_str(stdout).unwrap();
    let source = format!("{}/", fixture.source.display());
    let destination = format!("{}/", fixture.destination.display());
    assert_eq!(plan.run_argv, ["--archive", "--", &source, &destination]);
    assert_eq!(
        plan.dry_run_argv,
        ["--archive", "--dry-run", "--", &source, &destination]
    );
    assert_eq!(plan.contract_version, "planner.v1");
    assert_eq!(fs::read(&fixture.task_file).unwrap(), task_bytes);
    assert_eq!(fs::read(fixture.source.join("content")).unwrap(), b"source");
    assert_eq!(
        fs::read(fixture.destination.join("content")).unwrap(),
        b"target"
    );
    assert!(!fixture.root.join("not-installed-engine").exists());
}

#[test]
fn invalid_command_lines_report_structured_usage_errors() {
    for arguments in [
        vec!["plan"],
        vec!["plan", "file", "extra"],
        vec!["run", "file"],
        vec!["--help"],
    ] {
        assert_failure(
            probe().args(arguments).output().unwrap(),
            PlanningErrorCode::InvalidTask,
            Some("arguments"),
        );
    }
}

#[test]
fn task_file_errors_and_non_utf8_are_distinguished() {
    let fixture = Fixture::new();
    for path in [
        fixture.root.join("missing"),
        fixture.task_file.join("child"),
        fixture.root.clone(),
    ] {
        assert_failure(
            probe().arg("plan").arg(path).output().unwrap(),
            PlanningErrorCode::InvalidPath,
            Some("taskFile"),
        );
    }
    fs::write(&fixture.task_file, [0xff]).unwrap();
    assert_failure(
        fixture.plan(),
        PlanningErrorCode::InvalidJson,
        Some("taskFile"),
    );
}

#[test]
fn bounded_input_accepts_64_kib_and_rejects_one_more_byte() {
    let fixture = Fixture::new();
    let mut input = fixture.json().to_string();
    input.extend(std::iter::repeat_n(' ', 65_536 - input.len()));
    fs::write(&fixture.task_file, &input).unwrap();
    assert_eq!(fixture.plan().status.code(), Some(0));
    input.push(' ');
    fs::write(&fixture.task_file, &input).unwrap();
    assert_failure(fixture.plan(), PlanningErrorCode::InvalidTask, Some("json"));
}

#[test]
fn parse_validation_and_directory_failures_are_never_success_plans() {
    let fixture = Fixture::new();
    fs::write(&fixture.task_file, "{").unwrap();
    assert_failure(fixture.plan(), PlanningErrorCode::InvalidJson, None);
    let mut json = fixture.json();
    json["schemaVersion"] = 2.into();
    fs::write(&fixture.task_file, json.to_string()).unwrap();
    assert_failure(
        fixture.plan(),
        PlanningErrorCode::UnsupportedSchema,
        Some("schemaVersion"),
    );
    let mut json = fixture.json();
    json["destination"] = json["source"].clone();
    fs::write(&fixture.task_file, json.to_string()).unwrap();
    assert_failure(
        fixture.plan(),
        PlanningErrorCode::UnsafePathRelation,
        Some("destination.raw"),
    );
    fs::write(&fixture.task_file, fixture.json().to_string()).unwrap();
    fs::remove_dir(&fixture.source).unwrap();
    assert_failure(
        fixture.plan(),
        PlanningErrorCode::InvalidPath,
        Some("source.raw"),
    );
}

#[test]
fn repeated_option_kind_is_rejected_through_the_cli() {
    let fixture = Fixture::new();
    for option in [
        r#"{"kind":"archive","kind":"archive"}"#,
        r#"{"kind":{"recursive":null}}"#,
    ] {
        let input = fixture
            .json()
            .to_string()
            .replace(r#"{"kind":"archive"}"#, option);
        fs::write(&fixture.task_file, input).unwrap();
        assert_failure(fixture.plan(), PlanningErrorCode::InvalidTask, None);
    }
}

// Byte-name fixtures are exercised on Linux; the macOS fixture cannot be created.
#[cfg(target_os = "linux")]
#[test]
fn task_file_path_itself_can_be_non_utf8() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let fixture = Fixture::new();
    let path = fixture.root.join(OsStr::from_bytes(b"task-\xff.json"));
    fs::copy(&fixture.task_file, &path).unwrap();
    assert_eq!(
        probe()
            .arg("plan")
            .arg(&path)
            .output()
            .unwrap()
            .status
            .code(),
        Some(0)
    );
}
