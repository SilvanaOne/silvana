use std::sync::OnceLock;
use tracing::{info, warn};

/// Hardware information for the coordinator/prover
#[derive(Debug, Clone)]
pub struct HardwareInfo {
    pub cpu_cores: u8,
    pub prover_architecture: String,
    pub prover_memory: u64, // in KB to match sys_info
}

/// Global hardware info initialized once
static HARDWARE_INFO: OnceLock<HardwareInfo> = OnceLock::new();

/// Get hardware information, initializing it once on first call
pub fn get_hardware_info() -> &'static HardwareInfo {
    HARDWARE_INFO.get_or_init(|| {
        info!("Initializing hardware information");
        detect_hardware_info()
    })
}

/// Detect hardware information using standard library functions
fn detect_hardware_info() -> HardwareInfo {
    // Get CPU cores
    let cpu_cores = num_cpus::get().min(255) as u8; // Clamp to u8 max

    // Get system architecture
    let prover_architecture = get_system_architecture();

    // Get total memory in KB
    let prover_memory = get_total_memory();

    let hardware_info = HardwareInfo {
        cpu_cores,
        prover_architecture: prover_architecture.clone(),
        prover_memory,
    };

    info!(
        "Detected hardware: CPU cores: {}, Architecture: {}, Memory: {} KB",
        hardware_info.cpu_cores, hardware_info.prover_architecture, hardware_info.prover_memory
    );

    hardware_info
}

/// Get system architecture string
fn get_system_architecture() -> String {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;

    // Create a more descriptive architecture string
    match (os, arch) {
        ("linux", "x86_64") => "x86_64-linux".to_string(),
        ("linux", "aarch64") => "aarch64-linux".to_string(),
        ("macos", "x86_64") => "x86_64-darwin".to_string(),
        ("macos", "aarch64") => "aarch64-darwin".to_string(),
        ("windows", "x86_64") => "x86_64-windows".to_string(),
        (os, arch) => format!("{}-{}", arch, os),
    }
}

/// Get total system memory in KB
fn get_total_memory() -> u64 {
    match sys_info::mem_info() {
        Ok(mem_info) => {
            info!(
                "Memory info: total={} KB, available={} KB, free={} KB",
                mem_info.total, mem_info.avail, mem_info.free
            );
            mem_info.total
        }
        Err(e) => {
            warn!("Failed to get memory info: {}, using default 8GB", e);
            8 * 1024 * 1024 // Default to 8GB in KB
        }
    }
}

/// Get current available memory in GB
pub fn get_available_memory_gb() -> u64 {
    match sys_info::mem_info() {
        Ok(mem_info) => {
            // Convert from KB to GB
            mem_info.avail / (1024 * 1024)
        }
        Err(e) => {
            warn!("Failed to get memory info: {}, using default 4GB available", e);
            4 // Default to 4GB available
        }
    }
}

/// Get total system memory in GB
pub fn get_total_memory_gb() -> u64 {
    match sys_info::mem_info() {
        Ok(mem_info) => {
            // Convert from KB to GB
            mem_info.total / (1024 * 1024)
        }
        Err(e) => {
            warn!("Failed to get memory info: {}, using default 8GB total", e);
            8 // Default to 8GB total
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hardware_detection() {
        let hardware_info = get_hardware_info();

        // Basic sanity checks
        assert!(
            hardware_info.cpu_cores > 0,
            "Should have at least 1 CPU core"
        );
        assert!(
            !hardware_info.prover_architecture.is_empty(),
            "Architecture should not be empty"
        );
        assert!(
            hardware_info.prover_memory > 0,
            "Memory should be greater than 0"
        );

        println!("Hardware info: {:?}", hardware_info);
    }

    #[test]
    fn test_hardware_info_consistency() {
        // Test that multiple calls return the same info (OnceLock behavior)
        let info1 = get_hardware_info();
        let info2 = get_hardware_info();

        assert_eq!(info1.cpu_cores, info2.cpu_cores);
        assert_eq!(info1.prover_architecture, info2.prover_architecture);
        assert_eq!(info1.prover_memory, info2.prover_memory);
    }

    #[test]
    fn test_architecture_string() {
        let arch = get_system_architecture();
        assert!(!arch.is_empty());

        // Should contain either known architectures or be in format arch-os
        let known_archs = [
            "x86_64-linux",
            "aarch64-linux",
            "x86_64-darwin",
            "aarch64-darwin",
            "x86_64-windows",
        ];
        let is_known = known_archs.iter().any(|&known| arch == known);
        let has_dash = arch.contains('-');

        assert!(
            is_known || has_dash,
            "Architecture should be known format or contain dash: {}",
            arch
        );
    }
}
