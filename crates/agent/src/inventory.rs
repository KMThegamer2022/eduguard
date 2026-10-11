//! Inventory assembly (C2): hardware, OS, disks, installed software.

use crate::platform;
use protocol::{AppInfo, DiskInfo, Inventory};
use sysinfo::{Disks, System};

pub fn collect(sys: &mut System) -> Inventory {
    sys.refresh_cpu_all();
    sys.refresh_memory();
    let disks = Disks::new_with_refreshed_list();
    let disk_infos: Vec<DiskInfo> = disks
        .list()
        .iter()
        .filter(|d| d.total_space() > 0)
        .map(|d| DiskInfo {
            name: d.name().to_string_lossy().to_string(),
            total_mb: d.total_space() / 1_048_576,
            available_mb: d.available_space() / 1_048_576,
        })
        .collect();

    let apps: Vec<AppInfo> = platform::installed_apps();
    let cpu = sys.cpus().first().map(|c| c.brand().to_string()).unwrap_or_default();

    Inventory {
        ts: chrono::Utc::now().timestamp(),
        os: System::name().unwrap_or_else(|| platform::os_label().into()),
        os_version: System::os_version().unwrap_or_default(),
        hostname: platform::hostname(),
        arch: std::env::consts::ARCH.to_string(),
        cpu,
        cpu_cores: sys.cpus().len() as u32,
        mem_total_mb: sys.total_memory() / 1_048_576,
        disks: disk_infos,
        installed_apps: apps.into_iter().take(500).collect(),
        peripherals: Vec::new(),
        agent_version: protocol::AGENT_VERSION.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_works() {
        let mut sys = System::new();
        let inv = collect(&mut sys);
        assert!(!inv.os.is_empty());
        assert!(inv.mem_total_mb > 0);
        assert!(inv.cpu_cores > 0);
    }
}
