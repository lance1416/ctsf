use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PlannerTask {
    pub schema_version: u32,
    pub kind: String,
    pub task_id: String,
    pub revision: u64,
    pub name: String,
    pub engine_executable: String,
    pub source: PlannerLocalPath,
    pub destination: PlannerLocalPath,
    pub options: Vec<PlannerTaskOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PlannerLocalPath {
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum PlannerTaskOption {
    Recursive,
    Archive,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PlannerInvocationPlan {
    pub contract_version: String,
    pub task_snapshot: PlannerTask,
    pub executable: String,
    pub run_argv: Vec<String>,
    pub dry_run_argv: Vec<String>,
    pub path_check: PlannerPathCheck,
    pub engine_check: PlannerEngineCheck,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum PlannerPathCheck {
    CheckedAtPlanning,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum PlannerEngineCheck {
    NotPerformed,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum PlannerErrorCode {
    InvalidJson,
    UnsupportedSchema,
    InvalidTask,
    InvalidPath,
    UnsafePathRelation,
    PathAccessDenied,
    PathIo,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum PlannerBridgeError {
    Planning {
        code: PlannerErrorCode,
        field: Option<String>,
        message: String,
        os_code: Option<i32>,
    },
}

impl fmt::Display for PlannerBridgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Planning { field, message, .. } => match field {
                Some(field) => write!(formatter, "{field}: {message}"),
                None => formatter.write_str(message),
            },
        }
    }
}

impl std::error::Error for PlannerBridgeError {}

impl From<kernel::LocalPath> for PlannerLocalPath {
    fn from(path: kernel::LocalPath) -> Self {
        Self { raw: path.raw }
    }
}

impl From<PlannerLocalPath> for kernel::LocalPath {
    fn from(path: PlannerLocalPath) -> Self {
        Self { raw: path.raw }
    }
}

impl From<kernel::TaskOption> for PlannerTaskOption {
    fn from(option: kernel::TaskOption) -> Self {
        match option {
            kernel::TaskOption::Recursive => Self::Recursive,
            kernel::TaskOption::Archive => Self::Archive,
        }
    }
}

impl From<PlannerTaskOption> for kernel::TaskOption {
    fn from(option: PlannerTaskOption) -> Self {
        match option {
            PlannerTaskOption::Recursive => Self::Recursive,
            PlannerTaskOption::Archive => Self::Archive,
        }
    }
}

impl From<kernel::Task> for PlannerTask {
    fn from(task: kernel::Task) -> Self {
        Self {
            schema_version: task.schema_version,
            kind: task.kind,
            task_id: task.task_id,
            revision: task.revision,
            name: task.name,
            engine_executable: task.engine_executable,
            source: task.source.into(),
            destination: task.destination.into(),
            options: task.options.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<PlannerTask> for kernel::Task {
    fn from(task: PlannerTask) -> Self {
        Self {
            schema_version: task.schema_version,
            kind: task.kind,
            task_id: task.task_id,
            revision: task.revision,
            name: task.name,
            engine_executable: task.engine_executable,
            source: task.source.into(),
            destination: task.destination.into(),
            options: task.options.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<kernel::PathCheck> for PlannerPathCheck {
    fn from(check: kernel::PathCheck) -> Self {
        match check {
            kernel::PathCheck::CheckedAtPlanning => Self::CheckedAtPlanning,
        }
    }
}

impl From<kernel::EngineCheck> for PlannerEngineCheck {
    fn from(check: kernel::EngineCheck) -> Self {
        match check {
            kernel::EngineCheck::NotPerformed => Self::NotPerformed,
        }
    }
}

impl From<kernel::InvocationPlan> for PlannerInvocationPlan {
    fn from(plan: kernel::InvocationPlan) -> Self {
        Self {
            contract_version: plan.contract_version,
            task_snapshot: plan.task_snapshot.into(),
            executable: plan.executable,
            run_argv: plan.run_argv,
            dry_run_argv: plan.dry_run_argv,
            path_check: plan.path_check.into(),
            engine_check: plan.engine_check.into(),
        }
    }
}

impl From<kernel::PlanningErrorCode> for PlannerErrorCode {
    fn from(code: kernel::PlanningErrorCode) -> Self {
        match code {
            kernel::PlanningErrorCode::InvalidJson => Self::InvalidJson,
            kernel::PlanningErrorCode::UnsupportedSchema => Self::UnsupportedSchema,
            kernel::PlanningErrorCode::InvalidTask => Self::InvalidTask,
            kernel::PlanningErrorCode::InvalidPath => Self::InvalidPath,
            kernel::PlanningErrorCode::UnsafePathRelation => Self::UnsafePathRelation,
            kernel::PlanningErrorCode::PathAccessDenied => Self::PathAccessDenied,
            kernel::PlanningErrorCode::PathIo => Self::PathIo,
        }
    }
}

impl From<kernel::PlanningError> for PlannerBridgeError {
    fn from(error: kernel::PlanningError) -> Self {
        Self::Planning {
            code: error.code.into(),
            field: error.field,
            message: error.message,
            os_code: error.os_code,
        }
    }
}

/// Parse the original task JSON through the core's strict parser.
#[uniffi::export]
pub fn parse_task_json(json: String) -> Result<PlannerTask, PlannerBridgeError> {
    kernel::parse_task_json(&json)
        .map(Into::into)
        .map_err(Into::into)
}

/// Plan with the core's validation and read-only directory checks; never execute.
#[uniffi::export]
pub fn build_local_plan(task: PlannerTask) -> Result<PlannerInvocationPlan, PlannerBridgeError> {
    kernel::build_local_plan(task.into())
        .map(Into::into)
        .map_err(Into::into)
}

#[cfg(test)]
mod tests;
