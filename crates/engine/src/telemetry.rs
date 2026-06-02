//! GPU/system telemetry sampler. `sample()` shells `nvidia-smi --query-gpu` CSV
//! and parses it into a `Telemetry`; degrades gracefully (empty `gpus`) when the
//! driver is not active. CPU/mem come from sysinfo. Used by the GUI sampler tick.
use crate::event::{GpuSample, Telemetry};

pub fn sample() -> Telemetry {
    let mut t = Telemetry {
        at_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
        ..Default::default()
    };

    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    t.mem_total_mb = Some(sys.total_memory() / 1024 / 1024);
    t.mem_used_mb = Some(sys.used_memory() / 1024 / 1024);
    t.load_avg = Some(sysinfo::System::load_average().one as f32);

    let query = "index,name,utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw";
    if let Ok(out) = std::process::Command::new("nvidia-smi")
        .args([
            &format!("--query-gpu={query}"),
            "--format=csv,noheader,nounits",
        ])
        .output()
    {
        if out.status.success() {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                let f: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
                if f.len() >= 6 {
                    t.gpus.push(GpuSample {
                        index: f[0].parse().unwrap_or(0),
                        name: f[1].to_string(),
                        util_pct: f[2].parse().unwrap_or(0),
                        mem_used_mb: f[3].parse().unwrap_or(0),
                        mem_total_mb: f[4].parse().unwrap_or(0),
                        temp_c: f[5].parse().unwrap_or(0),
                        power_w: f
                            .get(6)
                            .and_then(|s| s.parse::<f32>().ok())
                            .map(|p| p as u32),
                    });
                }
            }
        }
    }
    t
}
