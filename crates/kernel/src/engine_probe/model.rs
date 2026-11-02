use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineKind {
    Rsync,
    Openrsync,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeStep {
    Version,
    Help,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeStream {
    Stdout,
    Stderr,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityStatus {
    Supported,
    Unsupported,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityReason {
    HelpEntry,
    ExplicitUnsupported,
    MissingEvidence,
    UnusableHelp,
    UnknownImplementation,
    ConflictingEvidence,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceRef {
    pub step: ProbeStep,
    pub stream: ProbeStream,
    pub line: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineIdentity {
    pub kind: EngineKind,
    pub implementation_version: Option<String>,
    pub compatibility_version: Option<String>,
    pub protocol_version: Option<u32>,
    pub evidence: Vec<EvidenceRef>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionCapability {
    pub token: String,
    pub status: CapabilityStatus,
    pub reason: CapabilityReason,
    pub evidence: Vec<EvidenceRef>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturedStream {
    pub bytes: Vec<u8>,
    pub observed_bytes: u64,
    pub truncated: bool,
    pub utf8_valid: bool,
    pub eof: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupState {
    Gone,
    Unconfirmed,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeSignal {
    Term,
    Kill,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalAttempt {
    pub signal: ProbeSignal,
    pub sent: bool,
    pub os_code: Option<i32>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupEvidence {
    pub child_reaped: bool,
    pub group: GroupState,
    pub signals: Vec<SignalAttempt>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandObservation {
    pub step: ProbeStep,
    pub argv: Vec<String>,
    pub pid: u32,
    pub exit_code: Option<i32>,
    pub term_signal: Option<i32>,
    pub elapsed_ms: u64,
    pub timed_out: bool,
    pub stdout: CapturedStream,
    pub stderr: CapturedStream,
    pub cleanup: CleanupEvidence,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineProbeReport {
    pub schema_version: u32,
    pub contract_version: String,
    pub requested_executable: String,
    pub resolved_executable: String,
    pub identity: EngineIdentity,
    pub capabilities: Vec<OptionCapability>,
    pub commands: Vec<CommandObservation>,
    pub elapsed_ms: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineProbeErrorCode {
    InvalidEnginePath,
    EngineNotFound,
    PermissionDenied,
    EngineChanged,
    SpawnFailed,
    DeadlineExceeded,
    OutputLimitExceeded,
    IoFailed,
    NonZeroExit,
    CleanupUnconfirmed,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineProbeError {
    pub code: EngineProbeErrorCode,
    pub step: Option<ProbeStep>,
    pub message: String,
    pub os_code: Option<i32>,
    pub partial_report: Option<Box<EngineProbeReport>>,
}
impl std::fmt::Display for EngineProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for EngineProbeError {}
