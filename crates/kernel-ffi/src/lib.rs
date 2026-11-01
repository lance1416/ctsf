//! UniFFI adapter for the setup-only, stateless kernel API.

uniffi::setup_scaffolding!();

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct KernelInfo {
    pub contract_version: String,
    pub kernel_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum HealthStatus {
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct HealthReport {
    pub status: HealthStatus,
}

#[uniffi::export]
pub fn kernel_info() -> KernelInfo {
    let info = kernel::kernel_info();
    KernelInfo {
        contract_version: info.contract_version,
        kernel_version: info.kernel_version,
    }
}

#[uniffi::export]
pub fn health() -> HealthReport {
    let status = match kernel::health().status {
        kernel::HealthStatus::Ready => HealthStatus::Ready,
    };
    HealthReport { status }
}
