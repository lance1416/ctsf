use std::io::{self, Read};
use std::os::fd::AsFd;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use nix::errno::Errno;
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;

use super::*;

struct Pipe<R> {
    reader: Option<R>,
    captured: CapturedStream,
}
impl<R: Read + AsFd> Pipe<R> {
    fn new(reader: Option<R>) -> (Self, Option<io::Error>) {
        let setup = reader
            .as_ref()
            .ok_or_else(|| io::Error::other("missing child pipe"))
            .and_then(|fd| {
                let flags = fcntl(fd, FcntlArg::F_GETFL).map_err(io::Error::from)?;
                fcntl(
                    fd,
                    FcntlArg::F_SETFL(OFlag::from_bits_retain(flags) | OFlag::O_NONBLOCK),
                )
                .map_err(io::Error::from)?;
                Ok(())
            });
        let mut pipe = Self {
            reader,
            captured: CapturedStream {
                bytes: Vec::new(),
                observed_bytes: 0,
                truncated: false,
                utf8_valid: true,
                eof: false,
            },
        };
        let error = setup.err();
        // Never call Read on a descriptor whose nonblocking setup failed.
        if error.is_some() {
            pipe.reader = None;
        }
        (pipe, error)
    }
    fn read_once(&mut self, limit: usize) -> Result<bool, io::Error> {
        if self.captured.eof {
            return Ok(false);
        }
        let Some(reader) = self.reader.as_mut() else {
            return Ok(false);
        };
        let mut bytes = [0_u8; 4096];
        match reader.read(&mut bytes) {
            Ok(0) => {
                self.captured.eof = true;
                Ok(true)
            }
            Ok(length) => {
                self.captured.observed_bytes =
                    self.captured.observed_bytes.saturating_add(length as u64);
                let retained = length.min(limit.saturating_sub(self.captured.bytes.len()));
                self.captured.bytes.extend_from_slice(&bytes[..retained]);
                self.captured.truncated |= retained < length;
                Ok(true)
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(false)
            }
            Err(error) => {
                self.reader = None;
                Err(error)
            }
        }
    }
    fn finish(mut self) -> CapturedStream {
        self.captured.utf8_valid = std::str::from_utf8(&self.captured.bytes).is_ok();
        self.captured
    }
}
#[derive(Default)]
struct Failures {
    io: Option<io::Error>,
    limited: bool,
    timed_out: bool,
}
impl Failures {
    fn read(&mut self, result: io::Result<bool>) -> bool {
        match result {
            Ok(progress) => progress,
            Err(error) => {
                if self.io.is_none() {
                    self.io = Some(error);
                }
                false
            }
        }
    }
}
fn pump<A: Read + AsFd, B: Read + AsFd>(
    stdout: &mut Pipe<A>,
    stderr: &mut Pipe<B>,
    failures: &mut Failures,
    limit: usize,
) -> bool {
    let mut progress = false;
    for _ in 0..4 {
        progress |= failures.read(stdout.read_once(limit));
        progress |= failures.read(stderr.read_once(limit));
    }
    failures.limited |= stdout.captured.truncated || stderr.captured.truncated;
    progress
}
fn pause(until: Instant, budgets: Budgets) {
    std::thread::sleep(
        until
            .saturating_duration_since(Instant::now())
            .min(budgets.tick),
    );
}
fn signal(group: Pid, signal: Signal, label: ProbeSignal) -> SignalAttempt {
    match killpg(group, signal) {
        Ok(()) => SignalAttempt {
            signal: label,
            sent: true,
            os_code: None,
        },
        Err(error) => SignalAttempt {
            signal: label,
            sent: false,
            os_code: Some(error as i32),
        },
    }
}
fn gone(group: Pid) -> bool {
    matches!(killpg(group, None), Err(Errno::ESRCH))
}

pub(super) fn observe(
    executable: &Path,
    step: ProbeStep,
    deadline: Instant,
    budgets: Budgets,
) -> Result<(CommandObservation, Option<EngineProbeError>), EngineProbeError> {
    let started = Instant::now();
    let argument = match step {
        ProbeStep::Version => "--version",
        ProbeStep::Help => "--help",
    };
    let mut child = Command::new(executable)
        .arg(argument)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir("/")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env("TZ", "UTC")
        .process_group(0)
        .spawn()
        .map_err(|error| {
            let code = if error.kind() == io::ErrorKind::PermissionDenied {
                EngineProbeErrorCode::PermissionDenied
            } else {
                EngineProbeErrorCode::SpawnFailed
            };
            failure(code, Some(step), error.to_string(), error.raw_os_error())
        })?;
    let pid = child.id();
    // Unix PIDs must fit pid_t. No broadcast/special identifiers are signalled.
    let group = match i32::try_from(pid) {
        Ok(id) if id > 1 && id != nix::unistd::getpgrp().as_raw() => Pid::from_raw(id),
        _ => {
            let _ = child.kill();
            let status = child.wait().ok();
            let captured = CapturedStream {
                bytes: Vec::new(),
                observed_bytes: 0,
                truncated: false,
                utf8_valid: true,
                eof: false,
            };
            let observation = CommandObservation {
                step,
                argv: vec![argument.into()],
                pid,
                exit_code: status.as_ref().and_then(std::process::ExitStatus::code),
                term_signal: status.as_ref().and_then(ExitStatusExt::signal),
                elapsed_ms: millis(started.elapsed()),
                timed_out: false,
                stdout: captured.clone(),
                stderr: captured,
                cleanup: CleanupEvidence {
                    child_reaped: status.is_some(),
                    group: GroupState::Unconfirmed,
                    signals: Vec::new(),
                },
            };
            return Ok((
                observation,
                Some(failure(
                    EngineProbeErrorCode::CleanupUnconfirmed,
                    Some(step),
                    "child returned an invalid process-group identity",
                    None,
                )),
            ));
        }
    };
    let (mut stdout, out_error) = Pipe::new(child.stdout.take());
    let (mut stderr, err_error) = Pipe::new(child.stderr.take());
    let mut failures = Failures {
        io: out_error.or(err_error),
        ..Failures::default()
    };
    let normal_eof = loop {
        if Instant::now() >= deadline {
            failures.timed_out = true;
            break false;
        }
        let progress = pump(&mut stdout, &mut stderr, &mut failures, budgets.capture);
        if failures.io.is_some() || failures.limited {
            break false;
        }
        if Instant::now() >= deadline {
            failures.timed_out = true;
            break false;
        }
        if stdout.captured.eof && stderr.captured.eof {
            break true;
        }
        if !progress {
            pause(deadline, budgets);
        }
    };
    let cleanup_deadline = Instant::now() + budgets.cleanup;
    if normal_eof {
        let until = (Instant::now() + budgets.eof_grace).min(cleanup_deadline);
        while Instant::now() < until {
            pause(until, budgets);
        }
    }
    // No wait/try_wait occurs until both possible group signals are finished.
    // Even an exited leader remains waitable and pins the identifier here.
    let term = signal(group, Signal::SIGTERM, ProbeSignal::Term);
    let until = (Instant::now() + budgets.term_grace).min(cleanup_deadline);
    while Instant::now() < until {
        pump(&mut stdout, &mut stderr, &mut failures, budgets.capture);
        pause(until, budgets);
    }
    let kill = signal(group, Signal::SIGKILL, ProbeSignal::Kill);
    let status = loop {
        match child.wait() {
            Ok(status) => break Some(status),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                if failures.io.is_none() {
                    failures.io = Some(error);
                }
                break None;
            }
        }
    };
    // Reaping ends signal ownership. Only read-only group observations follow.
    let group_gone = loop {
        pump(&mut stdout, &mut stderr, &mut failures, budgets.capture);
        let absent = gone(group);
        if absent && stdout.captured.eof && stderr.captured.eof {
            break true;
        }
        if Instant::now() >= cleanup_deadline {
            break absent;
        }
        pause(cleanup_deadline, budgets);
    };
    let observation = CommandObservation {
        step,
        argv: vec![argument.into()],
        pid,
        exit_code: status.as_ref().and_then(std::process::ExitStatus::code),
        term_signal: status.as_ref().and_then(ExitStatusExt::signal),
        elapsed_ms: millis(started.elapsed()),
        timed_out: failures.timed_out,
        stdout: stdout.finish(),
        stderr: stderr.finish(),
        cleanup: CleanupEvidence {
            child_reaped: status.is_some(),
            group: if group_gone {
                GroupState::Gone
            } else {
                GroupState::Unconfirmed
            },
            signals: vec![term, kill],
        },
    };
    let incomplete = !observation.cleanup.child_reaped
        || !group_gone
        || !observation.stdout.eof
        || !observation.stderr.eof;
    let error = if incomplete {
        Some(failure(
            EngineProbeErrorCode::CleanupUnconfirmed,
            Some(step),
            failures.io.as_ref().map_or_else(
                || "child, group or output cleanup could not be confirmed".into(),
                |error| format!("cleanup unconfirmed after I/O failure: {error}"),
            ),
            failures.io.as_ref().and_then(io::Error::raw_os_error),
        ))
    } else if failures.limited {
        Some(failure(
            EngineProbeErrorCode::OutputLimitExceeded,
            Some(step),
            "probe output exceeded the per-stream capture limit",
            None,
        ))
    } else if let Some(error) = failures.io {
        Some(failure(
            EngineProbeErrorCode::IoFailed,
            Some(step),
            error.to_string(),
            error.raw_os_error(),
        ))
    } else if failures.timed_out {
        Some(failure(
            EngineProbeErrorCode::DeadlineExceeded,
            Some(step),
            "shared probe deadline expired",
            None,
        ))
    } else if observation.exit_code != Some(0) || observation.term_signal.is_some() {
        Some(failure(
            EngineProbeErrorCode::NonZeroExit,
            Some(step),
            "probe did not exit successfully",
            None,
        ))
    } else {
        None
    };
    Ok((observation, error))
}
