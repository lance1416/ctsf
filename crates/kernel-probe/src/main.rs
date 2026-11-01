fn main() {
    let info = kernel::kernel_info();
    let health = match kernel::health().status {
        kernel::HealthStatus::Ready => "ready",
    };
    println!(
        "ctsf-kernel version={} contract={} health={health}",
        info.kernel_version, info.contract_version
    );
}
