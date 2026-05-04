use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Args)]
pub struct BenchArgs {
    #[command(subcommand)]
    command: BenchCommand,
}

#[derive(Subcommand)]
enum BenchCommand {
    /// CPU benchmark (single + multi core)
    Cpu,
    /// Memory bandwidth benchmark
    Memory,
    /// Disk I/O benchmark
    Disk {
        /// Block device to test
        #[arg(long, default_value = "/tmp")]
        device: String,
    },
    /// Network bandwidth and latency benchmark
    Network {
        /// Target host
        #[arg(long, default_value = "1.1.1.1")]
        target: String,
    },
    /// Run all benchmarks
    All,
    /// Compare current benchmark results vs a saved baseline
    Compare {
        /// Path to baseline results file
        #[arg(long)]
        baseline: Option<String>,
    },
}

impl BenchArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            BenchCommand::Cpu => bench_cpu(),
            BenchCommand::Memory => bench_memory(),
            BenchCommand::Disk { device } => bench_disk(&device),
            BenchCommand::Network { target } => bench_network(&target),
            BenchCommand::All => {
                bench_cpu()?;
                println!();
                bench_memory()?;
                println!();
                bench_disk("/tmp")?;
                println!();
                bench_network("1.1.1.1")?;
                Ok(())
            }
            BenchCommand::Compare { baseline } => bench_compare(baseline.as_deref()),
        }
    }
}

fn bench_cpu() -> Result<()> {
    println!("{}", "CPU Benchmark".bold().underline());
    println!();

    let cpus = num_cpus();
    const ITERATIONS: u64 = 50_000_000;

    // Single-threaded baseline.
    print!("  Single-threaded ({ITERATIONS} ops)... ");
    let start = Instant::now();
    let single_result = busy_loop(ITERATIONS);
    let single = start.elapsed();
    // Prevent optimization
    if single_result == 0 {
        print!(" ");
    }
    let single_ms = single.as_millis();
    let single_ops = ops_per_sec(ITERATIONS, single);
    println!(
        "{} ms — {:.2} Mops/s",
        single_ms.to_string().bold(),
        single_ops / 1_000_000.0
    );

    // Multi-threaded benchmark — saturate every available core/thread
    // by spawning one worker per logical CPU, each running the same
    // busy_loop. We sum their results (atomic wrapping_add) to keep
    // the optimizer honest.
    print!("  Multi-threaded ({cpus} threads)... ");
    let total = Arc::new(AtomicU64::new(0));
    let start = Instant::now();
    let mut handles = Vec::with_capacity(cpus);
    for _ in 0..cpus {
        let total = Arc::clone(&total);
        handles.push(thread::spawn(move || {
            let r = busy_loop(ITERATIONS);
            total.fetch_add(r, Ordering::Relaxed);
        }));
    }
    for h in handles {
        let _ = h.join();
    }
    let multi = start.elapsed();
    let multi_ms = multi.as_millis();
    let multi_ops = ops_per_sec(ITERATIONS * cpus as u64, multi);
    if total.load(Ordering::Relaxed) == 0 {
        print!(" ");
    }
    println!(
        "{} ms — {:.2} Mops/s",
        multi_ms.to_string().bold(),
        multi_ops / 1_000_000.0
    );

    let speedup = if multi.as_secs_f64() > 0.0 {
        single.as_secs_f64() * cpus as f64 / multi.as_secs_f64()
    } else {
        0.0
    };
    let efficiency = if cpus > 0 {
        speedup / cpus as f64 * 100.0
    } else {
        0.0
    };
    println!(
        "  Parallel speedup: {:.2}× across {cpus} threads ({:.1}% efficiency)",
        speedup, efficiency,
    );
    println!(
        "  Score: {} ms single / {} ms multi (lower is better)",
        single_ms.to_string().bold(),
        multi_ms.to_string().bold()
    );
    Ok(())
}

fn busy_loop(iterations: u64) -> u64 {
    let mut result = 0u64;
    for i in 0..iterations {
        result = result.wrapping_add(i.wrapping_mul(i));
    }
    result
}

fn ops_per_sec(ops: u64, elapsed: Duration) -> f64 {
    let secs = elapsed.as_secs_f64();
    if secs <= 0.0 {
        0.0
    } else {
        ops as f64 / secs
    }
}

fn bench_memory() -> Result<()> {
    println!("{}", "Memory Benchmark".bold().underline());
    println!();

    print!("  Sequential write (64 MB)... ");
    let start = Instant::now();
    let size = 64 * 1024 * 1024;
    let data: Vec<u8> = vec![0xAA; size];
    let elapsed = start.elapsed();
    let throughput = size as f64 / elapsed.as_secs_f64() / 1024.0 / 1024.0 / 1024.0;
    // Prevent optimization
    if data[size / 2] == 0 {
        print!(" ");
    }
    println!("{:.2} GB/s", throughput);

    print!("  Sequential read (64 MB)... ");
    let start = Instant::now();
    let mut sum: u64 = 0;
    for &byte in data.iter() {
        sum = sum.wrapping_add(byte as u64);
    }
    let elapsed = start.elapsed();
    let throughput = size as f64 / elapsed.as_secs_f64() / 1024.0 / 1024.0 / 1024.0;
    if sum == 0 {
        print!(" ");
    }
    println!("{:.2} GB/s", throughput);

    Ok(())
}

fn bench_disk(path: &str) -> Result<()> {
    println!("{}", "Disk I/O Benchmark".bold().underline());
    println!("  Target: {}", path.bold());
    println!();

    let test_file = format!("{path}/monolith-bench-test");

    // Write test
    print!("  Sequential write (256 MB)... ");
    let output = Command::new("dd")
        .args([
            "if=/dev/zero",
            &format!("of={test_file}"),
            "bs=1M",
            "count=256",
            "conv=fdatasync",
        ])
        .output()
        .context("failed to run write benchmark")?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let speed = stderr.lines().last().unwrap_or("unknown");
    println!("{}", speed.green());

    // Read test
    print!("  Sequential read (256 MB)... ");
    let _ = Command::new("bash")
        .args(["-c", "echo 3 > /proc/sys/vm/drop_caches"])
        .status();

    let output = Command::new("dd")
        .args([&format!("if={test_file}"), "of=/dev/null", "bs=1M"])
        .output()
        .context("failed to run read benchmark")?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let speed = stderr.lines().last().unwrap_or("unknown");
    println!("{}", speed.green());

    // Random I/O test with fio if available
    let fio = Command::new("fio")
        .args([
            "--name=randread",
            "--ioengine=libaio",
            "--rw=randread",
            "--bs=4k",
            "--numjobs=4",
            "--size=64M",
            &format!("--filename={test_file}"),
            "--runtime=5",
            "--time_based",
            "--output-format=terse",
            "--group_reporting",
        ])
        .output();

    if let Ok(o) = fio {
        if o.status.success() {
            let stdout = String::from_utf8_lossy(&o.stdout);
            println!(
                "  Random 4K read: {}",
                stdout.lines().next().unwrap_or("done").green()
            );
        }
    }

    let _ = std::fs::remove_file(&test_file);
    Ok(())
}

fn bench_network(target: &str) -> Result<()> {
    println!("{}", "Network Benchmark".bold().underline());
    println!("  Target: {}", target.bold());
    println!();

    // Latency test
    print!("  Latency (ping)... ");
    let output = Command::new("ping")
        .args(["-c", "5", "-q", target])
        .output()
        .context("failed to ping")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let rtt = stdout
        .lines()
        .find(|l| l.contains("rtt"))
        .unwrap_or("no data");
    println!("{}", rtt.green());

    // Bandwidth test with curl
    print!("  Download speed... ");
    let output = Command::new("curl")
        .args([
            "-o",
            "/dev/null",
            "-w",
            "%{speed_download}",
            "-s",
            &format!("http://{target}"),
        ])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let speed = String::from_utf8_lossy(&o.stdout);
            let bytes_per_sec: f64 = speed.trim().parse().unwrap_or(0.0);
            let mbps = bytes_per_sec * 8.0 / 1_000_000.0;
            println!("{:.2} Mbps", mbps);
        }
        _ => println!("{}", "unable to measure".dimmed()),
    }

    Ok(())
}

fn bench_compare(baseline: Option<&str>) -> Result<()> {
    match baseline {
        Some(path) => {
            let content =
                std::fs::read_to_string(path).with_context(|| format!("failed to read {path}"))?;
            println!("{}", "Baseline Results:".bold().underline());
            println!("{content}");
            println!();
            println!("{}", "Run 'mnctl bench all' and compare manually.".dimmed());
        }
        None => {
            println!(
                "{}",
                "Usage: mnctl bench compare --baseline <path-to-results>".dimmed()
            );
        }
    }
    Ok(())
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}
