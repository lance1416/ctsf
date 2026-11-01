use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;

const TASK_JSON: &str = r#"{
    "schemaVersion": 1,
    "kind": "rsync.task.pot",
    "taskId": "bridge-input",
    "revision": 18446744073709551615,
    "name": "中文 bridge task",
    "engineExecutable": "/not-installed/rsync",
    "source": {"raw": "/source 中文 '$HOME;*:local/"},
    "destination": {"raw": "/destination/"},
    "options": [{"kind": "recursive"}]
}"#;

fn task() -> PlannerTask {
    PlannerTask {
        schema_version: 1,
        kind: "rsync.task.pot".into(),
        task_id: "bridge-input".into(),
        revision: u64::MAX,
        name: "中文 bridge task".into(),
        engine_executable: "/not-installed/rsync".into(),
        source: PlannerLocalPath {
            raw: "/source 中文 '$HOME;*:local/".into(),
        },
        destination: PlannerLocalPath {
            raw: "/destination/".into(),
        },
        options: vec![PlannerTaskOption::Recursive],
    }
}

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    task: PlannerTask,
}

impl Fixture {
    fn new() -> Self {
        let root = loop {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("ctsf-ffi-test-{}-{sequence}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => break path,
                Err(failure) if failure.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(failure) => panic!("create test fixture: {failure}"),
            }
        };
        let source = root.join("源 目录 '$HOME;*:local");
        let destination = root.join("destination");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&destination).unwrap();
        let mut task = task();
        task.engine_executable = root.join("absent-engine").to_str().unwrap().into();
        task.source.raw = format!("{}/", source.display());
        task.destination.raw = format!("{}/", destination.display());
        Self { root, task }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn assert_error(
    error: PlannerBridgeError,
    expected: PlannerErrorCode,
    expected_field: Option<&str>,
) {
    let PlannerBridgeError::Planning {
        code,
        field,
        message,
        ..
    } = error;
    assert_eq!(code, expected);
    if let Some(expected_field) = expected_field {
        assert_eq!(field.as_deref(), Some(expected_field));
    }
    assert!(!message.is_empty());
}

#[test]
fn setup_queries_remain_compatible() {
    let info = crate::kernel_info();
    assert_eq!(info.contract_version, "setup.v1");
    assert_eq!(info.kernel_version, kernel::kernel_info().kernel_version);
    assert_eq!(crate::health().status, crate::HealthStatus::Ready);
}

#[test]
fn parse_returns_typed_task_with_lossless_unsigned_revision_and_paths() {
    assert_eq!(parse_task_json(TASK_JSON.into()).unwrap(), task());
    let archive = TASK_JSON.replace("recursive", "archive");
    let mut expected = task();
    expected.options = vec![PlannerTaskOption::Archive];
    assert_eq!(parse_task_json(archive).unwrap(), expected);
}

#[test]
fn task_conversion_round_trip_does_not_normalize_or_validate_fields() {
    let mut original = task();
    original.schema_version = u32::MAX;
    original.kind = " untouched kind ".into();
    original.task_id = " original identity ".into();
    original.name = " name\nwith\0control characters ".into();
    original.engine_executable = "/literal $ENGINE;* ".into();
    original.source.raw = "/raw/../source//".into();
    original.destination.raw = "/another/路径:literal/".into();
    original.options = vec![
        PlannerTaskOption::Archive,
        PlannerTaskOption::Recursive,
        PlannerTaskOption::Archive,
    ];
    let core: kernel::Task = original.clone().into();
    assert_eq!(core.schema_version, u32::MAX);
    assert_eq!(core.revision, u64::MAX);
    assert_eq!(core.source.raw, original.source.raw);
    assert_eq!(
        core.options,
        [
            kernel::TaskOption::Archive,
            kernel::TaskOption::Recursive,
            kernel::TaskOption::Archive
        ]
    );
    assert_eq!(PlannerTask::from(core), original);
}

#[test]
fn plan_conversion_preserves_all_core_fields_without_rebuilding_argv() {
    let expected_task = task();
    let run_argv = vec![
        "--archive".into(),
        "--".into(),
        "/raw source/".into(),
        "/target".into(),
    ];
    let dry_run_argv = vec![
        "--archive".into(),
        "--dry-run".into(),
        "--".into(),
        "/raw source/".into(),
        "/target".into(),
    ];
    let core = kernel::InvocationPlan {
        contract_version: "planner.v1".into(),
        task_snapshot: expected_task.clone().into(),
        executable: "/literal program".into(),
        run_argv: run_argv.clone(),
        dry_run_argv: dry_run_argv.clone(),
        path_check: kernel::PathCheck::CheckedAtPlanning,
        engine_check: kernel::EngineCheck::NotPerformed,
    };
    let plan = PlannerInvocationPlan::from(core);
    assert_eq!(
        plan,
        PlannerInvocationPlan {
            contract_version: "planner.v1".into(),
            task_snapshot: expected_task,
            executable: "/literal program".into(),
            run_argv,
            dry_run_argv,
            path_check: PlannerPathCheck::CheckedAtPlanning,
            engine_check: PlannerEngineCheck::NotPerformed,
        }
    );
}

#[test]
fn build_calls_real_core_and_returns_both_ordered_argument_groups() {
    let fixture = Fixture::new();
    fs::write(
        PathBuf::from(&fixture.task.source.raw).join("content"),
        b"source unchanged",
    )
    .unwrap();
    fs::write(
        PathBuf::from(&fixture.task.destination.raw).join("content"),
        b"target unchanged",
    )
    .unwrap();
    for (option, flag) in [
        (PlannerTaskOption::Recursive, "--recursive"),
        (PlannerTaskOption::Archive, "--archive"),
    ] {
        let mut input = fixture.task.clone();
        input.options = vec![option];
        let core = kernel::build_local_plan(input.clone().into()).unwrap();
        let plan = build_local_plan(input.clone()).unwrap();
        assert_eq!(plan.contract_version, core.contract_version);
        assert_eq!(plan.task_snapshot, input);
        assert_eq!(plan.executable, input.engine_executable);
        assert_eq!(
            plan.run_argv,
            [flag, "--", &input.source.raw, &input.destination.raw]
        );
        assert_eq!(plan.run_argv, core.run_argv);
        assert_eq!(
            plan.dry_run_argv,
            [
                flag,
                "--dry-run",
                "--",
                &input.source.raw,
                &input.destination.raw
            ]
        );
        assert_eq!(plan.dry_run_argv, core.dry_run_argv);
        assert_eq!(plan.path_check, PlannerPathCheck::CheckedAtPlanning);
        assert_eq!(plan.engine_check, PlannerEngineCheck::NotPerformed);
        assert_eq!(plan.task_snapshot.revision, u64::MAX);
        assert_eq!(build_local_plan(input).unwrap(), plan);
    }
    assert_eq!(
        fs::read(PathBuf::from(&fixture.task.source.raw).join("content")).unwrap(),
        b"source unchanged"
    );
    assert_eq!(
        fs::read(PathBuf::from(&fixture.task.destination.raw).join("content")).unwrap(),
        b"target unchanged"
    );
    assert!(!fixture.root.join("absent-engine").exists());
}

#[test]
fn typed_build_cannot_bypass_core_validation() {
    let fixture = Fixture::new();
    let mut input = fixture.task.clone();
    input.schema_version = 2;
    assert_error(
        build_local_plan(input).unwrap_err(),
        PlannerErrorCode::UnsupportedSchema,
        Some("schemaVersion"),
    );
    let mut input = fixture.task.clone();
    input.name.clear();
    assert_error(
        build_local_plan(input).unwrap_err(),
        PlannerErrorCode::InvalidTask,
        Some("name"),
    );
    let mut input = fixture.task.clone();
    input.options.clear();
    assert_error(
        build_local_plan(input).unwrap_err(),
        PlannerErrorCode::InvalidTask,
        Some("options"),
    );
    let mut input = fixture.task.clone();
    input.engine_executable = "relative-rsync".into();
    assert_error(
        build_local_plan(input).unwrap_err(),
        PlannerErrorCode::InvalidPath,
        Some("engineExecutable"),
    );
}

#[test]
fn parse_keeps_core_strict_json_and_semantic_error_categories() {
    let cases = [
        ("{".into(), PlannerErrorCode::InvalidJson),
        (
            TASK_JSON.replace("\"schemaVersion\": 1", "\"schemaVersion\": 2"),
            PlannerErrorCode::UnsupportedSchema,
        ),
        (
            TASK_JSON.replace(
                "\"taskId\": \"bridge-input\"",
                "\"taskId\": \"bridge-input\", \"taskId\": \"duplicate\"",
            ),
            PlannerErrorCode::InvalidTask,
        ),
        (
            TASK_JSON.replace(
                "\"taskId\": \"bridge-input\"",
                "\"taskId\": \"bridge-input\", \"unknown\": true",
            ),
            PlannerErrorCode::InvalidTask,
        ),
        (
            TASK_JSON.replace(
                "{\"kind\": \"recursive\"}",
                "{\"kind\": \"recursive\", \"kind\": \"archive\"}",
            ),
            PlannerErrorCode::InvalidTask,
        ),
        (
            TASK_JSON.replace(
                "{\"kind\": \"recursive\"}",
                "{\"kind\": {\"recursive\": null}}",
            ),
            PlannerErrorCode::InvalidTask,
        ),
        (
            TASK_JSON.replace("/not-installed/rsync", "relative-rsync"),
            PlannerErrorCode::InvalidPath,
        ),
    ];
    for (json, expected) in cases {
        assert_error(parse_task_json(json).unwrap_err(), expected, None);
    }
}

#[test]
fn directory_errors_are_forwarded_from_real_core_checks() {
    let fixture = Fixture::new();
    let mut input = fixture.task.clone();
    input.destination = input.source.clone();
    let original = kernel::build_local_plan(input.clone().into()).unwrap_err();
    let actual = build_local_plan(input).unwrap_err();
    let PlannerBridgeError::Planning {
        code,
        field,
        message,
        os_code,
    } = actual;
    assert_eq!(code, PlannerErrorCode::UnsafePathRelation);
    assert_eq!(field.as_deref(), Some("destination.raw"));
    assert_eq!(field, original.field);
    assert_eq!(message, original.message);
    assert_eq!(os_code, original.os_code);
    fs::remove_dir(&fixture.task.source.raw).unwrap();
    let actual = build_local_plan(fixture.task.clone()).unwrap_err();
    let PlannerBridgeError::Planning {
        code,
        field,
        os_code,
        ..
    } = actual;
    assert_eq!(code, PlannerErrorCode::InvalidPath);
    assert_eq!(field.as_deref(), Some("source.raw"));
    assert!(os_code.is_some());
}

#[test]
fn every_error_code_preserves_optional_fields_and_signed_os_code() {
    let codes = [
        (
            kernel::PlanningErrorCode::InvalidJson,
            PlannerErrorCode::InvalidJson,
        ),
        (
            kernel::PlanningErrorCode::UnsupportedSchema,
            PlannerErrorCode::UnsupportedSchema,
        ),
        (
            kernel::PlanningErrorCode::InvalidTask,
            PlannerErrorCode::InvalidTask,
        ),
        (
            kernel::PlanningErrorCode::InvalidPath,
            PlannerErrorCode::InvalidPath,
        ),
        (
            kernel::PlanningErrorCode::UnsafePathRelation,
            PlannerErrorCode::UnsafePathRelation,
        ),
        (
            kernel::PlanningErrorCode::PathAccessDenied,
            PlannerErrorCode::PathAccessDenied,
        ),
        (kernel::PlanningErrorCode::PathIo, PlannerErrorCode::PathIo),
    ];
    for (core_code, expected_code) in codes {
        for field in [None, Some("source.raw"), Some(""), Some("未知.field")] {
            for os_code in [None, Some(0), Some(13), Some(i32::MIN), Some(i32::MAX)] {
                let original = kernel::PlanningError {
                    code: core_code.clone(),
                    field: field.map(String::from),
                    message: "original detail 中文\n\0".into(),
                    os_code,
                };
                let converted = PlannerBridgeError::from(original.clone());
                assert_eq!(
                    converted,
                    PlannerBridgeError::Planning {
                        code: expected_code.clone(),
                        field: field.map(String::from),
                        message: original.message.clone(),
                        os_code,
                    }
                );
                assert_eq!(converted.to_string(), original.to_string());
                let standard_error: &dyn std::error::Error = &converted;
                assert!(standard_error.source().is_none());
            }
        }
    }
}

#[test]
fn returned_snapshot_is_independent_of_later_input_edits() {
    let fixture = Fixture::new();
    let mut input = fixture.task.clone();
    let plan = build_local_plan(input.clone()).unwrap();
    input.name = "edited name".into();
    input.source.raw.push_str("changed");
    input.options = vec![PlannerTaskOption::Archive];
    assert_ne!(plan.task_snapshot, input);
    assert_eq!(plan.task_snapshot, fixture.task);
}
