use super::*;

pub(super) fn empty_identity() -> EngineIdentity {
    EngineIdentity {
        kind: EngineKind::Unknown,
        implementation_version: None,
        compatibility_version: None,
        protocol_version: None,
        evidence: Vec::new(),
    }
}
fn usable(command: &CommandObservation) -> bool {
    command.exit_code == Some(0)
        && command.term_signal.is_none()
        && !command.timed_out
        && command.stdout.eof
        && command.stderr.eof
        && !command.stdout.truncated
        && !command.stderr.truncated
        && command.stdout.utf8_valid
        && command.stderr.utf8_valid
        && command.cleanup.child_reaped
        && command.cleanup.group == GroupState::Gone
}
fn lines(command: &CommandObservation) -> impl Iterator<Item = (&str, EvidenceRef)> {
    [
        (ProbeStream::Stdout, &command.stdout),
        (ProbeStream::Stderr, &command.stderr),
    ]
    .into_iter()
    .flat_map(move |(stream, capture)| {
        std::str::from_utf8(&capture.bytes)
            .unwrap_or("")
            .split('\n')
            .enumerate()
            .map(move |(line, text)| {
                (
                    text,
                    EvidenceRef {
                        step: command.step,
                        stream,
                        line: u32::try_from(line + 1).unwrap_or(u32::MAX),
                    },
                )
            })
    })
}
fn version(token: &str) -> Option<String> {
    (token.as_bytes().first().is_some_and(u8::is_ascii_digit)
        && token.contains('.')
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".+_-".contains(&byte)))
    .then(|| token.to_owned())
}
struct Banner {
    kind: EngineKind,
    version: Option<String>,
    compatibility: Option<String>,
    protocol: Option<u32>,
}
fn banner(line: &str) -> Option<Banner> {
    let words: Vec<_> = line.split_ascii_whitespace().collect();
    match words.as_slice() {
        ["rsync", "version", value, "compatible"] => Some(Banner {
            kind: EngineKind::Unknown,
            version: None,
            compatibility: Some(version(value)?),
            protocol: None,
        }),
        ["rsync", "version", value, "protocol", "version", protocol] => Some(Banner {
            kind: EngineKind::Rsync,
            version: Some(version(value)?),
            compatibility: None,
            protocol: Some(protocol.parse().ok()?),
        }),
        ["openrsync:", "protocol", "version", protocol] => Some(Banner {
            kind: EngineKind::Openrsync,
            version: None,
            compatibility: None,
            protocol: Some(protocol.parse().ok()?),
        }),
        ["openrsync", value, "(protocol", "version", protocol] => Some(Banner {
            kind: EngineKind::Openrsync,
            version: Some(version(value)?),
            compatibility: None,
            protocol: Some(protocol.strip_suffix(')')?.parse().ok()?),
        }),
        _ => None,
    }
}
fn merge<T: Eq>(existing: &mut Option<T>, next: Option<T>) -> bool {
    match (existing.as_ref(), next) {
        (Some(old), Some(new)) => old != &new,
        (None, Some(new)) => {
            *existing = Some(new);
            false
        }
        _ => false,
    }
}
pub(super) fn identity(commands: &[CommandObservation]) -> EngineIdentity {
    let mut result = empty_identity();
    let Some(command) = commands
        .iter()
        .find(|command| command.step == ProbeStep::Version && usable(command))
    else {
        return result;
    };
    let mut conflict = false;
    for (line, evidence) in lines(command) {
        let Some(banner) = banner(line) else {
            continue;
        };
        let mut changed = false;
        if banner.kind != EngineKind::Unknown {
            if result.kind == EngineKind::Unknown {
                result.kind = banner.kind;
            } else if result.kind != banner.kind {
                changed = true;
            }
        }
        changed |= merge(&mut result.implementation_version, banner.version);
        changed |= merge(&mut result.compatibility_version, banner.compatibility);
        changed |= merge(&mut result.protocol_version, banner.protocol);
        if result.evidence.len() < 2 {
            result.evidence.push(evidence);
        } else if changed {
            result.evidence.truncate(1);
            result.evidence.push(evidence);
        }
        conflict |= changed;
    }
    if conflict {
        result.kind = EngineKind::Unknown;
        result.implementation_version = None;
        result.compatibility_version = None;
        result.protocol_version = None;
    }
    result
}

// Only leading option tokens form the option column. Description words do not.
fn entry(line: &str) -> (Vec<&str>, String) {
    let mut remaining = line.trim_ascii_start();
    let mut tokens = Vec::new();
    while remaining.starts_with('-') {
        let end = remaining
            .bytes()
            .position(|byte| byte.is_ascii_whitespace() || byte == b',')
            .unwrap_or(remaining.len());
        tokens.push(&remaining[..end]);
        remaining = remaining[end..].trim_start_matches(|character: char| {
            character.is_ascii_whitespace() || character == ','
        });
    }
    (
        tokens,
        remaining
            .split_ascii_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_ascii_lowercase(),
    )
}
pub(super) fn capabilities(
    identity: &EngineIdentity,
    commands: &[CommandObservation],
) -> Vec<OptionCapability> {
    let help = commands
        .iter()
        .find(|command| command.step == ProbeStep::Help && usable(command));
    ["--recursive", "--archive", "--dry-run", "--"]
        .into_iter()
        .map(|token| {
            let mut result = OptionCapability {
                token: token.into(),
                status: CapabilityStatus::Unknown,
                reason: CapabilityReason::MissingEvidence,
                evidence: Vec::new(),
            };
            if identity.kind == EngineKind::Unknown {
                result.reason = CapabilityReason::UnknownImplementation;
                return result;
            }
            let Some(help) = help else {
                result.reason = CapabilityReason::UnusableHelp;
                return result;
            };
            let mut positive = None;
            let mut negative = None;
            let mut ambiguous = None;
            for (line, evidence) in lines(help) {
                let (tokens, description) = entry(line);
                if !tokens.contains(&token) {
                    continue;
                }
                if token == "--" && tokens.as_slice() != ["--"] {
                    continue;
                }
                if ["unsupported", "not supported", "(not supported)"]
                    .contains(&description.as_str())
                {
                    if negative.is_none() {
                        negative = Some(evidence);
                    }
                } else if description.contains("unsupported")
                    || description.contains("not supported")
                {
                    if ambiguous.is_none() {
                        ambiguous = Some(evidence);
                    }
                } else if (token != "--"
                    || [
                        "end of options",
                        "stop option processing",
                        "stop parsing options",
                    ]
                    .contains(&description.as_str()))
                    && positive.is_none()
                {
                    positive = Some(evidence);
                }
            }
            result.evidence.extend(positive.iter().cloned());
            result.evidence.extend(negative.iter().cloned());
            if positive.is_some() && negative.is_some() {
                result.reason = CapabilityReason::ConflictingEvidence;
            } else if ambiguous.is_some() {
                if result.evidence.is_empty() {
                    result.evidence.extend(ambiguous);
                }
            } else if positive.is_some() {
                result.status = CapabilityStatus::Supported;
                result.reason = CapabilityReason::HelpEntry;
            } else if negative.is_some() {
                result.status = CapabilityStatus::Unsupported;
                result.reason = CapabilityReason::ExplicitUnsupported;
            }
            result
        })
        .collect()
}

#[cfg(test)]
mod tests;
