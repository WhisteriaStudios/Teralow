use std::fs; // For file system operations
use std::process::Command; // For executing system commands
use std::thread; // For multithreading

mod capture;
mod encoder; // Import the encoder module // Import the capture module

#[tokio::main]
async fn main() {
    // Program information

    let name = "Teralow"; // Name of the program
    let version = env!("CARGO_PKG_VERSION"); // App version
    let divider = "==============================="; // Divider for formatting
    println!("{}", divider);
    println!("  {} v{}", name, version);
    println!("{}", divider);

    // Get the System Information
    println!("\n[Hardware Scan]\n");

    // OS
    let os = get_os_name();
    println!("OS: {}", os);
    println!("Architecture: {}", std::env::consts::ARCH);

    // CPU

    let cpu = get_cpu_info();
    println!("CPU: {}", cpu);

    let threads = thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(0);
    println!("Threads: {}", threads);

    // Memory

    let ram = get_total_ram();
    println!("RAM: {:.1} GiB", ram);

    // GPU

    let gpu = get_gpu_name();
    println!("GPU: {}", gpu);

    // Finish Hardware Scan

    print!("\n{}\n", divider);
    print!("\nHardware Scan Complete!");

    // Scan for available video encoders

    if let Err(error) = encoder::scan_encoders() {
        eprintln!("Error scanning encoders: {}", error);
    }

    print!("\n{}\n", divider);
    print!("\nEncoder Scan Complete!");

    // Start the screen capture process

    println!("\nStarting Screen Capture...");

    if let Err(error) = capture::start_capture().await {
        eprintln!("Screen capture failed: {error}");
    }
}

// Function to get the operating system name
fn get_os_name() -> String {
    let contents = fs::read_to_string("/etc/os-release").unwrap_or_default();

    for line in contents.lines() {
        if let Some(name) = line.strip_prefix("PRETTY_NAME=") {
            return name.trim_matches('"').to_string();
        }
    }

    "Unknown".to_string()
}

// Read the CPU information from /proc/cpuinfo
fn get_cpu_info() -> String {
    let contents = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();

    for line in contents.lines() {
        if let Some((key, value)) = line.split_once(':') {
            if key.trim() == "model name" {
                return value.trim().to_string();
            }
        }
    }

    "Unknown".to_string()
}

// Function to get the total RAM in GiB
fn get_total_ram() -> f64 {
    let contents = fs::read_to_string("/proc/meminfo").unwrap_or_default();

    for line in contents.lines() {
        if line.starts_with("MemTotal:") {
            if let Some(value) = line.split_whitespace().nth(1) {
                if let Ok(kib) = value.parse::<f64>() {
                    return kib / 1024.0 / 1024.0;
                }
            }
        }
    }

    0.0
}

// Detect NVIDIA GPU using the `nvidia-smi`
fn get_gpu_name() -> String {
    let output = Command::new("nvidia-smi")
        .args(["--query-gpu=name", "--format=csv,noheader"])
        .output();

    match output {
        Ok(result) if result.status.success() => {
            let name = String::from_utf8_lossy(&result.stdout);
            let name = name.lines().next().unwrap_or("").trim();

            if name.is_empty() {
                "No GPU Reported".to_string()
            } else {
                name.to_string()
            }
        }

        _ => "NVIDIA GPU unavailable or not detected".to_string(),
    }
}
