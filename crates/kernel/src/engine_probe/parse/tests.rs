use super::*;

fn command(step: ProbeStep, stdout: &str, stderr: &str) -> CommandObservation {
    let capture = |text: &str| CapturedStream {
        bytes: text.as_bytes().to_vec(),
        observed_bytes: text.len() as u64,
        truncated: false,
        utf8_valid: true,
        eof: true,
    };
    CommandObservation {
        step,
        argv: vec![],
        pid: 12345,
        exit_code: Some(0),
        term_signal: None,
        elapsed_ms: 0,
        timed_out: false,
        stdout: capture(stdout),
        stderr: capture(stderr),
        cleanup: CleanupEvidence {
            child_reaped: true,
            group: GroupState::Gone,
            signals: vec![],
        },
    }
}
#[test]
fn distinguishes_native_and_compatibility_versions_across_streams() {
    let result = identity(&[command(
        ProbeStep::Version,
        "openrsync: protocol version 29\n",
        "rsync version 2.6.9 compatible\n",
    )]);
    assert_eq!(result.kind, EngineKind::Openrsync);
    assert_eq!(result.implementation_version, None);
    assert_eq!(result.compatibility_version.as_deref(), Some("2.6.9"));
    assert_eq!(result.protocol_version, Some(29));
    assert_eq!(result.evidence.len(), 2);
    assert_eq!(result.evidence[1].stream, ProbeStream::Stderr);
    let result = identity(&[command(
        ProbeStep::Version,
        "",
        "openrsync 0.3.1 (protocol version 29)\n",
    )]);
    assert_eq!(result.implementation_version.as_deref(), Some("0.3.1"));
    assert_eq!(
        identity(&[command(
            ProbeStep::Version,
            "rsync version 2.6.9 compatible",
            ""
        )])
        .kind,
        EngineKind::Unknown
    );
}
#[test]
fn rejects_ambiguous_banners_and_unusable_observations() {
    for other in [
        "rsync  version 3.4.2  protocol version 32",
        "openrsync: protocol version 29",
        "rsync  version 3.4.1  protocol version 33",
    ] {
        let result = identity(&[command(
            ProbeStep::Version,
            "rsync  version 3.4.1  protocol version 32\n",
            other,
        )]);
        assert_eq!(result.kind, EngineKind::Unknown);
        assert_eq!(result.implementation_version, None);
        assert_eq!(result.protocol_version, None);
        assert_eq!(result.evidence.len(), 2);
    }
    for mode in 0..7 {
        let mut observation = command(
            ProbeStep::Version,
            "rsync  version 3.4.1  protocol version 32",
            "",
        );
        match mode {
            0 => observation.exit_code = Some(1),
            1 => observation.timed_out = true,
            2 => observation.stdout.truncated = true,
            3 => observation.stderr.utf8_valid = false,
            4 => observation.stdout.eof = false,
            5 => observation.cleanup.child_reaped = false,
            _ => observation.cleanup.group = GroupState::Unconfirmed,
        }
        assert_eq!(identity(&[observation]).kind, EngineKind::Unknown);
    }
    for line in [
        "rsync version newest protocol version 32",
        "rsync version 3.4.1 protocol version 999999999999",
        "tool says rsync version 3.4.1 protocol version 32",
    ] {
        assert_eq!(
            identity(&[command(ProbeStep::Version, line, "")]).kind,
            EngineKind::Unknown
        );
    }
}
#[test]
fn parses_only_exact_option_columns_and_explicit_negative_evidence() {
    let version = command(
        ProbeStep::Version,
        "rsync  version 3.4.1  protocol version 32",
        "",
    );
    let help = command(
        ProbeStep::Help,
        " -r, --recursive   recurse\n --archive  NOT SUPPORTED\n --dry-run-extra  irrelevant\n explanation --dry-run text\n -- end of options\n",
        "--dry-run  show what would be copied\n",
    );
    let result = capabilities(&identity(std::slice::from_ref(&version)), &[version, help]);
    assert_eq!(
        result
            .iter()
            .map(|cap| cap.token.as_str())
            .collect::<Vec<_>>(),
        ["--recursive", "--archive", "--dry-run", "--"]
    );
    assert_eq!(
        result.iter().map(|cap| &cap.status).collect::<Vec<_>>(),
        [
            &CapabilityStatus::Supported,
            &CapabilityStatus::Unsupported,
            &CapabilityStatus::Supported,
            &CapabilityStatus::Supported
        ]
    );
    assert_eq!(result[1].reason, CapabilityReason::ExplicitUnsupported);
    assert_eq!(
        result[2].evidence[0],
        EvidenceRef {
            step: ProbeStep::Help,
            stream: ProbeStream::Stderr,
            line: 1
        }
    );
}
#[test]
fn missing_conditional_conflicting_and_unknown_capabilities_stay_unknown() {
    let version = command(
        ProbeStep::Version,
        "rsync  version 3.4.1  protocol version 32",
        "",
    );
    let engine = identity(std::slice::from_ref(&version));
    let help = command(
        ProbeStep::Help,
        "--recursive recurse\n--recursive unsupported\n--archive not supported on this platform\n -n short only\n --no-dry-run inverse\n",
        "--archive archive mode",
    );
    let result = capabilities(&engine, &[help]);
    assert!(
        result
            .iter()
            .all(|cap| cap.status == CapabilityStatus::Unknown)
    );
    assert_eq!(result[0].reason, CapabilityReason::ConflictingEvidence);
    assert_eq!(result[0].evidence.len(), 2);
    assert_eq!(result[2].reason, CapabilityReason::MissingEvidence);
    assert!(
        capabilities(&empty_identity(), &[])
            .iter()
            .all(|cap| cap.reason == CapabilityReason::UnknownImplementation)
    );
    assert!(
        capabilities(&engine, &[])
            .iter()
            .all(|cap| cap.reason == CapabilityReason::UnusableHelp)
    );
}

#[test]
fn end_of_options_requires_its_own_option_column() {
    let version = command(
        ProbeStep::Version,
        "rsync  version 3.4.1  protocol version 32",
        "",
    );
    let engine = identity(std::slice::from_ref(&version));
    let combined = command(ProbeStep::Help, "--recursive -- end of options\n", "");
    let result = capabilities(&engine, &[combined]);
    assert_eq!(result[0].status, CapabilityStatus::Supported);
    assert_eq!(result[3].status, CapabilityStatus::Unknown);
    assert_eq!(result[3].reason, CapabilityReason::MissingEvidence);
    assert!(result[3].evidence.is_empty());
    let dedicated = command(ProbeStep::Help, "-- end of options\n", "");
    assert_eq!(
        capabilities(&engine, &[dedicated])[3].status,
        CapabilityStatus::Supported
    );
}
