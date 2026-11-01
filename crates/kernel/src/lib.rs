//! Setup queries and local invocation planning, without engine execution.
//! Ready does not imply an available transfer engine.

mod planner;

pub use planner::{
    EngineCheck, InvocationPlan, LocalPath, PathCheck, PlanningError, PlanningErrorCode, Task,
    TaskOption, build_local_plan, parse_task_json, validate_task,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelInfo {
    pub contract_version: String,
    pub kernel_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthStatus {
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthReport {
    pub status: HealthStatus,
}

pub fn kernel_info() -> KernelInfo {
    KernelInfo {
        contract_version: "setup.v1".into(),
        kernel_version: env!("CARGO_PKG_VERSION").into(),
    }
}

pub fn health() -> HealthReport {
    HealthReport {
        status: HealthStatus::Ready,
    }
}
