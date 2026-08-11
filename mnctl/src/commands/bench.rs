use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Serialize;
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

#[derive(Debug, Clone, Serialize)]
struct BenchResult {
    cpu: Option<CpuBenchResult>,
    memory: Option<MemoryBenchResult>,
    disk: Option<DiskBenchResult>,
    network: Option<NetworkBenchResult>,
}

#[derive(Debug, Clone, Serialize)]
struct CpuBenchResult {
    single_thread_ms: u128,
    single_thread_mops: f64,
    multi_thread_ms: u128,
    multi_thread_mops: f64,
    speedup: f64,
    efficiency_pct: f64,
    threads: usize,
}

#[derive(Debug, Clone, Serialize)]
struct MemoryBenchResult {
    write_gbps: f64,
    read_gbps: f64,
}

#[derive(Debug, Clone, Serialize)]
struct DiskBenchResult {
    write_speed: String,
    read_speed: String,
}

#[derive(Debug, Clone, Serialize)]
struct NetworkBenchResult {
    latency: String,
    download_mbps: f64,
}

#[derive(Subcommand)]
enum BenchCommand {
    /// CPU benchmark (single + multi core)
    Cpu {
        #[arg(long)]
        json: bool,
    },
    /// Memory bandwidth benchmark
    Memory {
        #[arg(long)]
        json: bool,
    },
    /// Disk I/O benchmark
    Disk {
        /// Block device to test
        #[arg(long, default_value = "/tmp")]
        device: String,
        #[arg(long)]
        json: bool,
    },
    /// Network bandwidth and latency benchmark
    Network {
        /// Target host
        #[arg(long, default_value = "1.1.1.1")]
        target: String,
        #[arg(long)]
        json: bool,
    },
    /// Run all benchmarks
    All {
        #[arg(long)]
        json: bool,
    },
    /// Run an iperf3 matrix between cluster nodes
    NetworkP2p,
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
            BenchCommand::Cpu { json } => bench_cpu(json),
            BenchCommand::Memory { json } => bench_memory(json),
            BenchCommand::Disk { device, json } => bench_disk(&device, json),
            BenchCommand::Network { target, json } => bench_network(&target, json),
            BenchCommand::All { json } => {
                if json {
                    let result = BenchResult {
                        cpu: Some(to_cpu_result()?),
                        memory: Some(to_memory_result()?),
                        disk: Some(to_disk_result("/tmp")?),
                        network: Some(to_network_result("cloudflare.com")?),
                    };
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    bench_cpu(false)?;
                    println!();
                    bench_memory(false)?;
                    println!();
                    bench_disk("/tmp", false)?;
                    println!();
                    bench_network("cloudflare.com", false)?;
                }
                Ok(())
            }
            BenchCommand::NetworkP2p => bench_network_p2p(),
            BenchCommand::Compare { baseline } => bench_compare(baseline.as_deref()),
        }
    }
}

fn bench_cpu(json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&to_cpu_result()?)?);
        return Ok(());
    }
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

fn to_cpu_result() -> Result<CpuBenchResult> {
    let cpus = num_cpus();
    const ITERATIONS: u64 = 50_000_000;

    let start = Instant::now();
    let single_result = busy_loop(ITERATIONS);
    let single = start.elapsed();
    if single_result == 0 {
        print!("");
    }
    let single_ms = single.as_millis();
    let single_ops = ops_per_sec(ITERATIONS, single);

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
        print!("");
    }

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

    Ok(CpuBenchResult {
        single_thread_ms: single_ms,
        single_thread_mops: single_ops / 1_000_000.0,
        multi_thread_ms: multi_ms,
        multi_thread_mops: multi_ops / 1_000_000.0,
        speedup,
        efficiency_pct: efficiency,
        threads: cpus,
    })
}

fn to_memory_result() -> Result<MemoryBenchResult> {
    let size = 64 * 1024 * 1024;
    let start = Instant::now();
    let data: Vec<u8> = vec![0xAA; size];
    let elapsed = start.elapsed();
    let write_gbps = size as f64 / elapsed.as_secs_f64() / 1024.0 / 1024.0 / 1024.0;
    if data[size / 2] == 0 {
        print!("");
    }

    let start = Instant::now();
    let mut sum: u64 = 0;
    for &byte in data.iter() {
        sum = sum.wrapping_add(byte as u64);
    }
    let elapsed = start.elapsed();
    let read_gbps = size as f64 / elapsed.as_secs_f64() / 1024.0 / 1024.0 / 1024.0;
    if sum == 0 {
        print!("");
    }

    Ok(MemoryBenchResult {
        write_gbps,
        read_gbps,
    })
}

fn to_disk_result(path: &str) -> Result<DiskBenchResult> {
    let test_file = format!("{path}/monolith-bench-test");
    let write_out = Command::new("dd")
        .args([
            "if=/dev/zero",
            &format!("of={test_file}"),
            "bs=1M",
            "count=256",
            "conv=fdatasync",
        ])
        .output()?;
    let write_speed = String::from_utf8_lossy(&write_out.stderr)
        .lines()
        .last()
        .unwrap_or("unknown")
        .to_string();
    let _ = Command::new("bash")
        .args(["-c", "echo 3 > /proc/sys/vm/drop_caches"])
        .status();
    let read_out = Command::new("dd")
        .args([&format!("if={test_file}"), "of=/dev/null", "bs=1M"])
        .output()?;
    let read_speed = String::from_utf8_lossy(&read_out.stderr)
        .lines()
        .last()
        .unwrap_or("unknown")
        .to_string();
    let _ = std::fs::remove_file(&test_file);
    Ok(DiskBenchResult {
        write_speed,
        read_speed,
    })
}

fn to_network_result(target: &str) -> Result<NetworkBenchResult> {
    let ping_out = Command::new("ping")
        .args(["-c", "5", "-q", target])
        .output()?;
    let stdout = String::from_utf8_lossy(&ping_out.stdout);
    let latency = stdout
        .lines()
        .find(|l| l.contains("rtt"))
        .unwrap_or("no data")
        .to_string();
    let test_url = if target == "1.1.1.1" || target == "cloudflare.com" {
        "https://speed.cloudflare.com/__down?bytes=10000000".to_string()
    } else {
        format!("https://{target}")
    };
    let curl_out = Command::new("curl")
        .args([
            "-o",
            "/dev/null",
            "-w",
            "%{speed_download}",
            "-sL",
            "--max-time",
            "30",
            &test_url,
        ])
        .output();
    let download_mbps = match curl_out {
        Ok(o) if o.status.success() => {
            let speed = String::from_utf8_lossy(&o.stdout);
            let bytes_per_sec: f64 = speed.trim().parse().unwrap_or(0.0);
            bytes_per_sec * 8.0 / 1_000_000.0
        }
        _ => 0.0,
    };
    Ok(NetworkBenchResult {
        latency,
        download_mbps,
    })
}

fn bench_memory(json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&to_memory_result()?)?);
        return Ok(());
    }
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

fn bench_disk(path: &str, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&to_disk_result(path)?)?);
        return Ok(());
    }
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

fn bench_network(target: &str, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&to_network_result(target)?)?
        );
        return Ok(());
    }
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

    // Bandwidth test with curl — download a 10 MB test payload over HTTPS
    print!("  Download speed... ");
    let test_url = if target == "1.1.1.1" || target == "cloudflare.com" {
        "https://speed.cloudflare.com/__down?bytes=10000000".to_string()
    } else {
        format!("https://{target}")
    };
    let output = Command::new("curl")
        .args([
            "-o",
            "/dev/null",
            "-w",
            "%{speed_download}",
            "-sL",
            "--max-time",
            "30",
            &test_url,
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

fn bench_network_p2p() -> Result<()> {
    // Find cluster members from /etc/monolith/cluster.toml
    let cluster_cfg = std::fs::read_to_string("/etc/monolith/cluster.toml").unwrap_or_default();

    let nodes: Vec<String> = cluster_cfg
        .lines()
        .filter(|l| l.contains("address ="))
        .filter_map(|l| l.split('"').nth(1))
        .map(|s| s.to_string())
        .collect();

    if nodes.is_empty() {
        println!(
            "{} No cluster nodes found in /etc/monolith/cluster.toml",
            "●".yellow()
        );
        println!("  Running local loopback benchmark...");

        // Start iperf3 server in background
        let _server = std::process::Command::new("iperf3")
            .args(["-s", "-D", "-p", "5201"])
            .spawn()
            .context("iperf3 not found. Install: mnpkg install iperf3")?;

        std::thread::sleep(std::time::Duration::from_secs(1));

        // Run client
        let output = std::process::Command::new("iperf3")
            .args(["-c", "127.0.0.1", "-p", "5201", "-t", "5", "-f", "m"])
            .output()?;

        let _ = std::process::Command::new("pkill")
            .args(["-f", "iperf3 -s -D -p 5201"])
            .status();

        let result = String::from_utf8_lossy(&output.stdout).to_string();
        println!("{} Iperf3 loopback result:", "→".blue());
        for line in result
            .lines()
            .filter(|l| l.contains("bits/sec") || l.contains("sender") || l.contains("receiver"))
        {
            println!("  {line}");
        }
        return Ok(());
    }

    println!(
        "{} Network Matrix Benchmark — {} nodes",
        "≡".blue(),
        nodes.len()
    );
    println!();

    for i in 0..nodes.len() {
        for j in (i + 1)..nodes.len() {
            let node_a = &nodes[i];
            let node_b = &nodes[j];
            println!("  {} → {}", node_a.bold(), node_b.bold());

            // Run iperf3 from this node to the remote node
            match std::process::Command::new("iperf3")
                .args(["-c", node_b, "-p", "5201", "-t", "3", "-f", "m"])
                .output()
            {
                Ok(output) => {
                    let result = String::from_utf8_lossy(&output.stdout);
                    for line in result.lines().filter(|l| l.contains("bits/sec")) {
                        println!("    {line}");
                    }
                }
                Err(e) => {
                    println!("    {} failed: {e}", "✗".red());
                }
            }
            println!();
        }
    }

    Ok(())
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}
