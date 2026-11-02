use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};
use tempfile::TempDir;

const VALID: &str = r#"case "$1" in
--version) printf 'rsync  version 3.4.1  protocol version 32\n';;
--help) printf '%s\n' '--recursive recurse' '--archive archive mode' '--dry-run show changes' '-- end of options';;
esac
"#;
fn engine(body: &str) -> (TempDir, String) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("engine 中文 '$HOME;*");
    fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    (root, path.to_str().unwrap().into())
}
fn fast() -> Budgets {
    Budgets {
        work: BUDGETS.work,
        cleanup: Duration::from_millis(500),
        eof_grace: Duration::from_millis(5),
        term_grace: Duration::from_millis(30),
        tick: Duration::from_millis(2),
        capture: 16384,
    }
}
fn clean(command: &CommandObservation) {
    assert!(command.cleanup.child_reaped);
    assert_eq!(command.cleanup.group, GroupState::Gone);
    assert!(command.stdout.eof && command.stderr.eof);
    assert_eq!(command.cleanup.signals.len(), 2);
}
#[test]
fn successful_probe_is_bounded_owned_and_has_fixed_command_order() {
    let (_root, path) = engine(VALID);
    let report = inspect_with_budgets(&path, fast()).unwrap();
    assert_eq!(report.contract_version, "engine-probe.v1");
    assert_eq!(report.schema_version, 1);
    assert_eq!(report.requested_executable, path);
    assert_eq!(report.identity.kind, EngineKind::Rsync);
    assert_eq!(
        report.identity.implementation_version.as_deref(),
        Some("3.4.1")
    );
    assert!(
        report
            .capabilities
            .iter()
            .all(|cap| cap.status == CapabilityStatus::Supported)
    );
    assert_eq!(report.commands[0].argv, ["--version"]);
    assert_eq!(report.commands[1].argv, ["--help"]);
    for command in &report.commands {
        clean(command);
        assert!(!command.timed_out);
    }
    let json = serde_json::to_string(&report).unwrap();
    assert_eq!(
        serde_json::from_str::<EngineProbeReport>(&json).unwrap(),
        report
    );
}
#[test]
fn rejects_invalid_missing_directory_and_non_executable_paths() {
    for input in ["", "relative", "~/rsync", "host:path", "/bad\0path"] {
        assert_eq!(
            inspect_engine(input).unwrap_err().code,
            EngineProbeErrorCode::InvalidEnginePath
        );
    }
    let (root, path) = engine(VALID);
    assert_eq!(
        inspect_engine(root.path().to_str().unwrap())
            .unwrap_err()
            .code,
        EngineProbeErrorCode::InvalidEnginePath
    );
    assert_eq!(
        inspect_engine(root.path().join("missing").to_str().unwrap())
            .unwrap_err()
            .code,
        EngineProbeErrorCode::EngineNotFound
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let error = inspect_engine(&path).unwrap_err();
    assert_eq!(error.code, EngineProbeErrorCode::PermissionDenied);
    assert!(error.partial_report.unwrap().commands.is_empty());
}
#[test]
fn symlink_request_is_preserved_but_resolved_path_is_used() {
    let (root, path) = engine(VALID);
    let alias = root.path().join("alias");
    symlink(&path, &alias).unwrap();
    let report = inspect_with_budgets(alias.to_str().unwrap(), fast()).unwrap();
    assert_eq!(report.requested_executable, alias.to_str().unwrap());
    assert_eq!(
        report.resolved_executable,
        fs::canonicalize(&path).unwrap().to_str().unwrap()
    );
}
#[test]
fn nonzero_version_still_observes_help_and_returns_error() {
    let body = VALID.replace(
        "protocol version 32\\n';;",
        "protocol version 32\\n'; exit 7;;",
    );
    assert_ne!(body, VALID);
    let (_root, path) = engine(&body);
    let error = inspect_with_budgets(&path, fast()).unwrap_err();
    assert_eq!(error.code, EngineProbeErrorCode::NonZeroExit);
    assert_eq!(error.step, Some(ProbeStep::Version));
    let report = error.partial_report.unwrap();
    assert_eq!(report.commands.len(), 2);
    assert_eq!(report.commands[0].exit_code, Some(7));
    for command in &report.commands {
        clean(command);
    }
    assert_eq!(report.identity.kind, EngineKind::Unknown);
}
#[test]
fn help_error_preserves_verified_version_without_supported_capabilities() {
    let body = VALID.replace("--help) printf", "--help) exit 2; printf");
    let (_root, path) = engine(&body);
    let error = inspect_with_budgets(&path, fast()).unwrap_err();
    assert_eq!(error.code, EngineProbeErrorCode::NonZeroExit);
    let report = error.partial_report.unwrap();
    assert_eq!(report.identity.kind, EngineKind::Rsync);
    assert!(
        report
            .capabilities
            .iter()
            .all(|cap| cap.reason == CapabilityReason::UnusableHelp)
    );
}
#[test]
fn invalid_utf8_and_unrecognized_banners_are_unknown_not_unsupported() {
    for body in ["printf '\\377'\n", "printf 'unrecognized tool\\n'\n"] {
        let (_root, path) = engine(body);
        let report = inspect_with_budgets(&path, fast()).unwrap();
        assert_eq!(report.identity.kind, EngineKind::Unknown);
        assert!(
            report
                .capabilities
                .iter()
                .all(|cap| cap.status == CapabilityStatus::Unknown)
        );
    }
}
#[test]
fn exact_capture_limit_is_accepted_but_one_extra_byte_truncates_and_drains() {
    for length in [16384, 16385] {
        let (_root, path) = engine(&format!("printf '%s' '{}'\n", "x".repeat(length)));
        let result = inspect_with_budgets(&path, fast());
        if length == 16384 {
            let report = result.unwrap();
            assert_eq!(report.commands[0].stdout.bytes.len(), length);
            assert!(!report.commands[0].stdout.truncated);
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.code, EngineProbeErrorCode::OutputLimitExceeded);
            let report = error.partial_report.unwrap();
            assert_eq!(report.commands.len(), 1);
            let command = &report.commands[0];
            assert_eq!(command.stdout.bytes.len(), 16384);
            assert_eq!(command.stdout.observed_bytes, 16385);
            assert!(command.stdout.truncated);
            clean(command);
        }
    }
}
#[test]
fn both_streams_are_drained_and_limits_do_not_deadlock() {
    let body = format!(
        "printf stdout_ready\nprintf stderr_ready >&2\nprintf '%s' '{}' &\nprintf '%s' '{}' >&2\nwait\n",
        "o".repeat(100_000),
        "e".repeat(100_000)
    );
    let (_root, path) = engine(&body);
    let error = inspect_with_budgets(&path, fast()).unwrap_err();
    assert!(matches!(
        error.code,
        EngineProbeErrorCode::OutputLimitExceeded | EngineProbeErrorCode::CleanupUnconfirmed
    ));
    let report = error.partial_report.unwrap();
    assert!(report.commands[0].cleanup.child_reaped);
    assert!(report.commands[0].stdout.bytes.len() <= 16384);
    assert!(report.commands[0].stderr.bytes.len() <= 16384);
    assert!(
        report.commands[0].stdout.observed_bytes > 0
            && report.commands[0].stderr.observed_bytes > 0
    );
}
#[test]
fn timeout_kills_term_ignoring_child_and_reaps_it() {
    let (_root, path) = engine("trap '' TERM\nexec /bin/sleep 30\n");
    let mut budgets = fast();
    budgets.work = Duration::from_millis(400);
    let started = Instant::now();
    let error = inspect_with_budgets(&path, budgets).unwrap_err();
    assert_eq!(error.code, EngineProbeErrorCode::DeadlineExceeded);
    assert!(started.elapsed() < Duration::from_secs(3));
    let report = error.partial_report.unwrap();
    let command = &report.commands[0];
    assert!(command.timed_out);
    assert_eq!(command.term_signal, Some(9));
    clean(command);
}
#[test]
fn eof_cleanup_crossing_work_deadline_does_not_retroactively_timeout_version() {
    let (_root, path) = engine(VALID);
    let mut budgets = fast();
    budgets.work = Duration::from_millis(300);
    budgets.eof_grace = Duration::from_millis(350);
    let error = inspect_with_budgets(&path, budgets).unwrap_err();
    assert_eq!(error.code, EngineProbeErrorCode::DeadlineExceeded);
    assert_eq!(error.step, Some(ProbeStep::Help));
    let report = error.partial_report.unwrap();
    assert_eq!(report.commands.len(), 1);
    assert!(!report.commands[0].timed_out);
    assert_eq!(report.identity.kind, EngineKind::Rsync);
}
#[test]
fn file_identity_change_prevents_help_spawn() {
    let (_root, path) = engine(
        "if [ \"$1\" = --version ]; then\n printf 'rsync  version 3.4.1  protocol version 32\\n'\n printf '\\n# changed\\n' >>\"$0\"\n exit 0\nfi\nprintf unexpected >\"$0.help\"\n",
    );
    let error = inspect_with_budgets(&path, fast()).unwrap_err();
    assert_eq!(error.code, EngineProbeErrorCode::EngineChanged);
    assert_eq!(error.step, Some(ProbeStep::Help));
    assert_eq!(error.partial_report.unwrap().commands.len(), 1);
    assert!(!Path::new(&format!("{path}.help")).exists());
}
#[test]
fn environment_cwd_stdin_and_arguments_are_fixed() {
    let body = "printf '%s\\n' \"argc=$#\" \"arg=$1\" \"cwd=$PWD\" \"HOME=${HOME-unset}\" \"PATH=$PATH\" \"LC_ALL=$LC_ALL\" \"LANG=$LANG\" \"TZ=$TZ\"\nif read -r input; then printf unexpected_input; else printf stdin_eof; fi\n";
    let (_root, path) = engine(body);
    let report = inspect_with_budgets(&path, fast()).unwrap();
    let stdout = std::str::from_utf8(&report.commands[0].stdout.bytes).unwrap();
    for expected in [
        "argc=1",
        "arg=--version",
        "cwd=/",
        "HOME=unset",
        "PATH=/usr/bin:/bin",
        "LC_ALL=C",
        "LANG=C",
        "TZ=UTC",
        "stdin_eof",
    ] {
        assert!(stdout.contains(expected), "{stdout}");
    }
}

#[test]
fn descendants_with_open_or_closed_pipes_do_not_escape_group_cleanup() {
    for close_pipes in [false, true] {
        let background = if close_pipes {
            "/bin/sleep 5 >/dev/null 2>&1 &"
        } else {
            "/bin/sleep 5 &"
        };
        let body = format!("{background}\nprintf '%s' \"$!\" >\"$0.$1.child\"\n{VALID}");
        let (root, path) = engine(&body);
        let mut budgets = fast();
        if !close_pipes {
            budgets.work = Duration::from_millis(400);
        }
        let started = Instant::now();
        let result = inspect_with_budgets(&path, budgets);
        let report = match result {
            Ok(report) => {
                assert!(close_pipes);
                report
            }
            Err(error) => {
                assert!(matches!(
                    error.code,
                    EngineProbeErrorCode::DeadlineExceeded
                        | EngineProbeErrorCode::CleanupUnconfirmed
                ));
                *error.partial_report.unwrap()
            }
        };
        assert!(started.elapsed() < Duration::from_secs(6));
        assert!(
            report
                .commands
                .iter()
                .all(|command| command.cleanup.child_reaped)
        );
        assert_eq!(report.commands[0].timed_out, !close_pipes);
        let mut observed_children = 0;
        for entry in fs::read_dir(root.path()).unwrap() {
            let entry = entry.unwrap();
            if !entry.file_name().to_string_lossy().ends_with(".child") {
                continue;
            }
            let pid = fs::read_to_string(entry.path()).unwrap();
            let output = std::process::Command::new("/bin/ps")
                .args(["-p", &pid, "-o", "stat="])
                .output()
                .unwrap();
            let state = String::from_utf8(output.stdout).unwrap();
            assert!(
                state.trim().is_empty() || state.trim().starts_with('Z'),
                "owned descendant {pid} remains live: {state}"
            );
            observed_children += 1;
        }
        assert!(observed_children > 0);
    }
}
