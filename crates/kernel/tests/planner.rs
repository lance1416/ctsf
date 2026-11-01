use std::fs;

use kernel::{
    EngineCheck, LocalPath, PathCheck, PlanningError, PlanningErrorCode, Task, TaskOption,
    build_local_plan, parse_task_json, validate_task,
};
use tempfile::TempDir;

struct Fixture {
    root: TempDir,
    task: Task,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("源 目录 '$HOME;*\" :local");
        let destination = root.path().join("target");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&destination).unwrap();
        let task = Task {
            schema_version: 1,
            kind: "rsync.task.pot".into(),
            task_id: " local-fixture ".into(),
            revision: 1,
            name: "中文 local planning".into(),
            engine_executable: root
                .path()
                .join("not-installed-rsync")
                .to_str()
                .unwrap()
                .into(),
            source: LocalPath {
                raw: source.to_str().unwrap().into(),
            },
            destination: LocalPath {
                raw: destination.to_str().unwrap().into(),
            },
            options: vec![TaskOption::Recursive],
        };
        Self { root, task }
    }

    fn json(&self) -> String {
        serde_json::to_string(&self.task).unwrap()
    }
}

fn assert_error(failure: PlanningError, code: PlanningErrorCode, field: Option<&str>) {
    assert_eq!(failure.code, code);
    if let Some(field) = field {
        assert_eq!(failure.field.as_deref(), Some(field));
    }
    assert!(!failure.message.is_empty());
}

#[test]
fn task_and_plan_round_trip_preserve_values_and_option_objects() {
    let mut fixture = Fixture::new();
    fixture.task.source.raw.push('/');
    fixture.task.destination.raw.push('/');
    fixture.task.revision = u64::MAX;
    for option in [TaskOption::Recursive, TaskOption::Archive] {
        fixture.task.options = vec![option];
        let task = parse_task_json(&fixture.json()).unwrap();
        assert_eq!(task, fixture.task);
        let plan = build_local_plan(task).unwrap();
        let json = serde_json::to_string(&plan).unwrap();
        assert_eq!(
            serde_json::from_str::<kernel::InvocationPlan>(&json).unwrap(),
            plan
        );
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["pathCheck"], "checked_at_planning");
        assert_eq!(value["engineCheck"], "not_performed");
        assert!(value["taskSnapshot"]["options"][0].is_object());
    }
}

#[test]
fn strict_objects_reject_duplicate_keys_including_option_tag() {
    let fixture = Fixture::new();
    let json = fixture.json();
    let source = serde_json::to_string(&fixture.task.source).unwrap();
    let cases = [
        format!("{{\"taskId\":\"duplicate\",{}", &json[1..]),
        json.replace(&source, r#"{"raw":"/one","raw":"/two"}"#),
        json.replace(
            r#"{"kind":"recursive"}"#,
            r#"{"kind":"recursive","kind":"recursive"}"#,
        ),
        json.replace(
            r#"{"kind":"recursive"}"#,
            r#"{"kind":"recursive","kind":"archive"}"#,
        ),
    ];
    for input in cases {
        assert_error(
            parse_task_json(&input).unwrap_err(),
            PlanningErrorCode::InvalidTask,
            None,
        );
    }
}

#[test]
fn strict_objects_reject_unknown_missing_null_and_wrong_types() {
    let fixture = Fixture::new();
    let valid: serde_json::Value = serde_json::from_str(&fixture.json()).unwrap();
    let mut inputs = vec![];
    for field in [
        "schemaVersion",
        "kind",
        "taskId",
        "revision",
        "name",
        "engineExecutable",
        "source",
        "destination",
        "options",
    ] {
        let mut missing = valid.clone();
        missing.as_object_mut().unwrap().remove(field);
        inputs.push(missing);
        let mut null = valid.clone();
        null[field] = serde_json::Value::Null;
        inputs.push(null);
    }
    let mut unknown = valid.clone();
    unknown["extra"] = true.into();
    inputs.push(unknown);
    let mut source_extra = valid.clone();
    source_extra["source"]["extra"] = true.into();
    inputs.push(source_extra);
    for source in [
        serde_json::json!({}),
        serde_json::json!({"raw":null}),
        serde_json::json!({"raw":false}),
    ] {
        let mut value = valid.clone();
        value["source"] = source;
        inputs.push(value);
    }
    for option in [
        serde_json::json!({"kind":"recursive","extra":true}),
        serde_json::json!({"kind":"bandwidth_limit_kib"}),
        serde_json::json!({"kind":null}),
        serde_json::json!({"kind":{"recursive":null}}),
        serde_json::json!({"kind":{"archive":null}}),
        serde_json::json!({"kind":["recursive"]}),
        serde_json::json!({"kind":true}),
        serde_json::json!({"kind":1}),
        serde_json::json!({}),
        serde_json::json!("recursive"),
        serde_json::json!(["recursive"]),
    ] {
        let mut value = valid.clone();
        value["options"] = serde_json::json!([option]);
        inputs.push(value);
    }
    for (field, bad) in [
        ("schemaVersion", serde_json::json!(-1)),
        ("schemaVersion", serde_json::json!(4_294_967_296_u64)),
        ("schemaVersion", serde_json::json!(1.0)),
        ("revision", serde_json::json!(-1)),
        ("revision", serde_json::json!("1")),
        ("name", serde_json::json!(false)),
        ("source", serde_json::json!("/tmp/source")),
        ("source", serde_json::json!(["/tmp/source"])),
        ("options", serde_json::json!({"kind":"archive"})),
    ] {
        let mut value = valid.clone();
        value[field] = bad;
        inputs.push(value);
    }
    for input in inputs {
        let failure = parse_task_json(&input.to_string()).unwrap_err();
        assert_eq!(
            failure.code,
            PlanningErrorCode::InvalidTask,
            "input: {input}; error: {failure:?}"
        );
        assert_error(failure, PlanningErrorCode::InvalidTask, None);
    }
    let positional = serde_json::json!([
        1, "rsync.task.pot", "fixture", 1, "name", "/engine",
        {"raw":"/source"}, {"raw":"/destination"}, [{"kind":"recursive"}]
    ]);
    assert_error(
        parse_task_json(&positional.to_string()).unwrap_err(),
        PlanningErrorCode::InvalidTask,
        None,
    );
}

#[test]
fn syntax_and_size_failures_have_distinct_codes() {
    let fixture = Fixture::new();
    for json in ["{", "", "{broken}", &(fixture.json() + " trailing")] {
        assert_error(
            parse_task_json(json).unwrap_err(),
            PlanningErrorCode::InvalidJson,
            None,
        );
    }
    let mut input = fixture.json();
    input.extend(std::iter::repeat_n(' ', 65_536 - input.len()));
    assert_eq!(parse_task_json(&input).unwrap(), fixture.task);
    input.push(' ');
    assert_error(
        parse_task_json(&input).unwrap_err(),
        PlanningErrorCode::InvalidTask,
        Some("json"),
    );
}

#[test]
fn semantic_task_validation_is_used_by_parse_and_build() {
    let fixture = Fixture::new();
    let mut cases = vec![];
    let mut task = fixture.task.clone();
    task.schema_version = 2;
    cases.push((task, PlanningErrorCode::UnsupportedSchema, "schemaVersion"));
    let mut task = fixture.task.clone();
    task.kind = "other".into();
    cases.push((task, PlanningErrorCode::InvalidTask, "kind"));
    let mut task = fixture.task.clone();
    task.revision = 0;
    cases.push((task, PlanningErrorCode::InvalidTask, "revision"));
    for text in ["".into(), " \t\n".into(), "a\0b".into(), "中".repeat(43)] {
        let mut task = fixture.task.clone();
        task.task_id = text;
        cases.push((task, PlanningErrorCode::InvalidTask, "taskId"));
    }
    for text in ["".into(), "\u{2003}".into(), "a\0b".into(), "中".repeat(86)] {
        let mut task = fixture.task.clone();
        task.name = text;
        cases.push((task, PlanningErrorCode::InvalidTask, "name"));
    }
    for options in [
        vec![],
        vec![TaskOption::Recursive, TaskOption::Recursive],
        vec![TaskOption::Archive, TaskOption::Recursive],
    ] {
        let mut task = fixture.task.clone();
        task.options = options;
        cases.push((task, PlanningErrorCode::InvalidTask, "options"));
    }
    for (task, code, field) in cases {
        assert_error(validate_task(&task).unwrap_err(), code.clone(), Some(field));
        assert_error(
            build_local_plan(task.clone()).unwrap_err(),
            code.clone(),
            Some(field),
        );
        assert_error(
            parse_task_json(&serde_json::to_string(&task).unwrap()).unwrap_err(),
            code,
            Some(field),
        );
    }
    let mut task = fixture.task.clone();
    task.task_id = "x".repeat(128);
    task.name = "x".repeat(256);
    assert!(validate_task(&task).is_ok());
}

#[test]
fn path_expressions_are_validated_without_io_or_normalization() {
    let fixture = Fixture::new();
    for field in ["engineExecutable", "source.raw", "destination.raw"] {
        for raw in [
            "".into(),
            "relative".into(),
            "~/source".into(),
            "host:path".into(),
            "rsync://host/path".into(),
            "/nul\0path".into(),
            format!("/{}", "中".repeat(1366)),
        ] {
            let mut task = fixture.task.clone();
            match field {
                "engineExecutable" => task.engine_executable = raw,
                "source.raw" => task.source.raw = raw,
                _ => task.destination.raw = raw,
            }
            assert_error(
                validate_task(&task).unwrap_err(),
                PlanningErrorCode::InvalidPath,
                Some(field),
            );
            assert_error(
                build_local_plan(task.clone()).unwrap_err(),
                PlanningErrorCode::InvalidPath,
                Some(field),
            );
            assert_error(
                parse_task_json(&serde_json::to_string(&task).unwrap()).unwrap_err(),
                PlanningErrorCode::InvalidPath,
                Some(field),
            );
        }
    }
    let mut task = fixture.task;
    task.source.raw = format!("/{}", "x".repeat(4095));
    task.destination.raw = "/not-created/destination/".into();
    assert!(validate_task(&task).is_ok());
    assert!(parse_task_json(&serde_json::to_string(&task).unwrap()).is_ok());
}

#[test]
fn ordered_argv_preserves_path_tokens_and_trailing_slashes() {
    let fixture = Fixture::new();
    for (option, flag) in [
        (TaskOption::Recursive, "--recursive"),
        (TaskOption::Archive, "--archive"),
    ] {
        for source_slash in [false, true] {
            for destination_slash in [false, true] {
                let mut task = fixture.task.clone();
                task.options = vec![option.clone()];
                if source_slash {
                    task.source.raw.push('/');
                }
                if destination_slash {
                    task.destination.raw.push('/');
                }
                let plan = build_local_plan(task.clone()).unwrap();
                assert_eq!(
                    plan.run_argv,
                    [flag, "--", &task.source.raw, &task.destination.raw]
                );
                assert_eq!(
                    plan.dry_run_argv,
                    [
                        flag,
                        "--dry-run",
                        "--",
                        &task.source.raw,
                        &task.destination.raw
                    ]
                );
                assert_eq!(plan.task_snapshot, task);
                assert_eq!(plan.executable, task.engine_executable);
                assert_eq!(plan.contract_version, "planner.v1");
                assert_eq!(plan.path_check, PathCheck::CheckedAtPlanning);
                assert_eq!(plan.engine_check, EngineCheck::NotPerformed);
            }
        }
    }
}

#[test]
fn engine_is_not_inspected_even_when_it_names_a_directory() {
    let fixture = Fixture::new();
    assert!(!std::path::Path::new(&fixture.task.engine_executable).exists());
    assert!(build_local_plan(fixture.task.clone()).is_ok());
    let mut task = fixture.task;
    task.engine_executable = fixture.root.path().to_str().unwrap().into();
    assert_eq!(
        build_local_plan(task).unwrap().engine_check,
        EngineCheck::NotPerformed
    );
}

#[test]
fn same_directory_and_both_nesting_directions_are_rejected() {
    let fixture = Fixture::new();
    let mut task = fixture.task.clone();
    task.destination = task.source.clone();
    assert_error(
        build_local_plan(task).unwrap_err(),
        PlanningErrorCode::UnsafePathRelation,
        Some("destination.raw"),
    );
    let child = std::path::Path::new(&fixture.task.source.raw).join("child");
    fs::create_dir(&child).unwrap();
    let mut task = fixture.task.clone();
    task.destination.raw = child.to_str().unwrap().into();
    assert_error(
        build_local_plan(task.clone()).unwrap_err(),
        PlanningErrorCode::UnsafePathRelation,
        Some("destination.raw"),
    );
    std::mem::swap(&mut task.source, &mut task.destination);
    assert_error(
        build_local_plan(task).unwrap_err(),
        PlanningErrorCode::UnsafePathRelation,
        Some("destination.raw"),
    );
}

#[test]
fn component_prefixes_and_raw_dot_segments_are_not_confused() {
    let fixture = Fixture::new();
    let source = fixture.root.path().join("src");
    let destination = fixture.root.path().join("src2");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&destination).unwrap();
    let mut task = fixture.task;
    task.source.raw = format!("{}/../src/", source.display());
    task.destination.raw = destination.to_str().unwrap().into();
    let plan = build_local_plan(task.clone()).unwrap();
    assert_eq!(plan.run_argv[2], task.source.raw);
}

#[test]
fn missing_files_not_a_directory_and_root_map_to_invalid_path() {
    let fixture = Fixture::new();
    let file = fixture.root.path().join("file");
    fs::write(&file, b"unchanged").unwrap();
    for (raw, has_os_code) in [
        (
            fixture
                .root
                .path()
                .join("missing")
                .to_str()
                .unwrap()
                .to_owned(),
            true,
        ),
        (file.to_str().unwrap().to_owned(), false),
        (file.join("child").to_str().unwrap().to_owned(), true),
        ("/".into(), false),
    ] {
        for is_source in [true, false] {
            let mut task = fixture.task.clone();
            let field = if is_source {
                task.source.raw = raw.clone();
                "source.raw"
            } else {
                task.destination.raw = raw.clone();
                "destination.raw"
            };
            let failure = build_local_plan(task).unwrap_err();
            assert_eq!(failure.os_code.is_some(), has_os_code);
            assert_error(failure, PlanningErrorCode::InvalidPath, Some(field));
        }
    }
}

#[cfg(unix)]
#[test]
fn symlink_aliases_are_checked_but_valid_raw_alias_is_preserved() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let alias = fixture.root.path().join("alias");
    symlink(&fixture.task.source.raw, &alias).unwrap();
    let mut task = fixture.task.clone();
    task.destination.raw = alias.to_str().unwrap().into();
    assert_error(
        build_local_plan(task).unwrap_err(),
        PlanningErrorCode::UnsafePathRelation,
        Some("destination.raw"),
    );
    let mut task = fixture.task.clone();
    task.source.raw = format!("{}/", alias.display());
    assert_eq!(
        build_local_plan(task.clone()).unwrap().run_argv[2],
        task.source.raw
    );
    let child = std::path::Path::new(&fixture.task.source.raw).join("child");
    fs::create_dir(&child).unwrap();
    let nested_alias = fixture.root.path().join("nested-alias");
    symlink(child, &nested_alias).unwrap();
    task.destination.raw = nested_alias.to_str().unwrap().into();
    assert_error(
        build_local_plan(task).unwrap_err(),
        PlanningErrorCode::UnsafePathRelation,
        Some("destination.raw"),
    );
    let dangling = fixture.root.path().join("dangling");
    symlink(fixture.root.path().join("missing"), &dangling).unwrap();
    let mut task = fixture.task.clone();
    task.source.raw = dangling.to_str().unwrap().into();
    assert_error(
        build_local_plan(task).unwrap_err(),
        PlanningErrorCode::InvalidPath,
        Some("source.raw"),
    );
    let root_alias = fixture.root.path().join("root-alias");
    symlink("/", &root_alias).unwrap();
    let mut task = fixture.task;
    task.source.raw = root_alias.to_str().unwrap().into();
    assert_error(
        build_local_plan(task).unwrap_err(),
        PlanningErrorCode::InvalidPath,
        Some("source.raw"),
    );
}

// This fixture needs a filesystem accepting arbitrary byte names. The local
// macOS filesystem rejects its creation, so exercise it on the Linux target.
#[cfg(target_os = "linux")]
#[test]
fn canonical_target_need_not_be_utf8() {
    use std::ffi::OsString;
    use std::os::unix::{ffi::OsStringExt, fs::symlink};
    let fixture = Fixture::new();
    let target = fixture
        .root
        .path()
        .join(OsString::from_vec(b"non-utf8-\xff".to_vec()));
    fs::create_dir(&target).unwrap();
    let alias = fixture.root.path().join("utf8-alias");
    symlink(&target, &alias).unwrap();
    let mut task = fixture.task;
    task.source.raw = alias.to_str().unwrap().into();
    assert_eq!(
        build_local_plan(task.clone()).unwrap().run_argv[2],
        task.source.raw
    );
}

#[cfg(unix)]
#[test]
fn symlink_loop_reports_path_io_and_os_code() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let cycle = fixture.root.path().join("cycle");
    symlink("cycle", &cycle).unwrap();
    let mut task = fixture.task;
    task.source.raw = cycle.to_str().unwrap().into();
    let failure = build_local_plan(task).unwrap_err();
    assert!(failure.os_code.is_some());
    assert_error(failure, PlanningErrorCode::PathIo, Some("source.raw"));
}

#[test]
fn repeated_plans_are_owned_deterministic_and_recheck_directory_state() {
    let fixture = Fixture::new();
    let source_file = std::path::Path::new(&fixture.task.source.raw).join("content");
    let destination_file = std::path::Path::new(&fixture.task.destination.raw).join("content");
    fs::write(&source_file, b"source unchanged").unwrap();
    fs::write(&destination_file, b"destination unchanged").unwrap();
    let first = build_local_plan(fixture.task.clone()).unwrap();
    assert_eq!(first, build_local_plan(fixture.task.clone()).unwrap());
    let mut edited = fixture.task.clone();
    edited.name = "edited".into();
    edited.options = vec![TaskOption::Archive];
    assert_ne!(first.task_snapshot, edited);
    assert_eq!(first.task_snapshot, fixture.task);
    assert_eq!(fs::read(&source_file).unwrap(), b"source unchanged");
    assert_eq!(
        fs::read(&destination_file).unwrap(),
        b"destination unchanged"
    );
    fs::remove_file(source_file).unwrap();
    fs::remove_dir(&fixture.task.source.raw).unwrap();
    assert_error(
        build_local_plan(fixture.task).unwrap_err(),
        PlanningErrorCode::InvalidPath,
        Some("source.raw"),
    );
}

#[test]
fn errors_serialize_as_typed_camel_case_records() {
    let failure = PlanningError {
        code: PlanningErrorCode::PathAccessDenied,
        field: Some("source.raw".into()),
        message: "denied".into(),
        os_code: Some(13),
    };
    let json = serde_json::to_string(&failure).unwrap();
    assert_eq!(
        serde_json::from_str::<PlanningError>(&json).unwrap(),
        failure
    );
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["code"], "path_access_denied");
    assert_eq!(value["osCode"], 13);
    assert!(failure.to_string().contains("source.raw"));
    let _: &dyn std::error::Error = &failure;
}

#[cfg(unix)]
#[test]
#[ignore = "run explicitly as a non-privileged user; chmod denial is not reliable under root"]
fn inaccessible_parent_reports_permission_denied() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let parent = fixture.root.path().join("restricted");
    let source = parent.join("source");
    fs::create_dir(&parent).unwrap();
    fs::create_dir(&source).unwrap();
    let original = fs::metadata(&parent).unwrap().permissions();
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o000)).unwrap();
    let mut task = fixture.task.clone();
    task.source.raw = source.to_str().unwrap().into();
    let result = build_local_plan(task);
    fs::set_permissions(&parent, original).unwrap();
    let failure = result.expect_err("permission fixture must actually deny access");
    assert!(failure.os_code.is_some());
    assert_error(
        failure,
        PlanningErrorCode::PathAccessDenied,
        Some("source.raw"),
    );
}
