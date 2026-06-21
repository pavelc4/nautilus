use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use grammers_client::Client;
use grammers_client::message::InputMessage;
use serde::Deserialize;

use crate::app::AppState;

#[derive(Deserialize)]
#[allow(dead_code)]
struct AstraHealthResponse {
    success: bool,
    data: Option<AstraHealthData>,
}

#[derive(Deserialize)]
struct AstraHealthData {
    status: String,
    version: String,
    goroutines: usize,
    uptime: String,
    requests: AstraHealthRequests,
    cookies: AstraHealthCookies,
    disk: AstraHealthDisk,
    memory: AstraHealthMemory,
}

#[derive(Deserialize)]
struct AstraHealthRequests {
    total: u64,
    success: u64,
    failed: u64,
}

#[derive(Deserialize)]
struct AstraHealthCookies {
    instagram: bool,
    facebook: bool,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct AstraHealthDisk {
    total: u64,
    used: u64,
    free: u64,
}

#[derive(Deserialize)]
struct AstraHealthMemory {
    #[serde(rename = "heapAlloc")]
    heap_alloc: u64,
    #[serde(rename = "heapInuse")]
    heap_inuse: u64,
    #[serde(rename = "heapObjects")]
    heap_objects: u64,
    #[serde(rename = "stackInuse")]
    stack_inuse: u64,
    #[serde(rename = "gcCycles")]
    gc_cycles: u32,
    #[serde(rename = "gcPause")]
    gc_pause: u64,
}

fn clean_go_duration(dur: &str) -> String {
    let mut result = String::new();
    let mut chars = dur.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '.' {
            // Skip all digits following the dot
            while let Some(&next_c) = chars.peek() {
                if next_c.is_ascii_digit() {
                    chars.next();
                } else {
                    break;
                }
            }
        } else {
            result.push(c);
        }
    }

    let mut formatted = String::new();
    let mut prev_is_unit = false;
    for c in result.chars() {
        if c.is_ascii_alphabetic() && !prev_is_unit {
            prev_is_unit = true;
        } else if c.is_ascii_digit() && prev_is_unit {
            formatted.push(' ');
            prev_is_unit = false;
        }
        formatted.push(c);
    }
    formatted
}

fn parse_cpu_ticks(stat: &str) -> Option<(u64, u64)> {
    let line = stat.lines().next()?;
    let vals: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|v| v.parse().ok())
        .collect();
    if vals.len() < 5 {
        return None;
    }
    let total: u64 = vals.iter().sum();
    let idle = vals.get(3).copied().unwrap_or(0);
    Some((total, idle))
}

fn parse_self_stat_ticks(stat: &str) -> Option<u64> {
    let right_paren = stat.rfind(')')?;
    let rest = &stat[right_paren + 1..];
    let parts: Vec<&str> = rest.split_whitespace().collect();
    let utime: u64 = parts.get(11)?.parse().ok()?;
    let stime: u64 = parts.get(12)?.parse().ok()?;
    Some(utime + stime)
}

async fn sample_cpu_stats(delay: Duration) -> (f64, f64) {
    let sys_stat1 = read_proc("/proc/stat").await.unwrap_or_default();
    let self_stat1 = read_proc("/proc/self/stat").await.unwrap_or_default();

    tokio::time::sleep(delay).await;

    let sys_stat2 = read_proc("/proc/stat").await.unwrap_or_default();
    let self_stat2 = read_proc("/proc/self/stat").await.unwrap_or_default();

    let (sys_tot1, sys_idl1) = parse_cpu_ticks(&sys_stat1).unwrap_or((1, 0));
    let (sys_tot2, sys_idl2) = parse_cpu_ticks(&sys_stat2).unwrap_or((2, 0));

    let self_ticks1 = parse_self_stat_ticks(&self_stat1).unwrap_or(0);
    let self_ticks2 = parse_self_stat_ticks(&self_stat2).unwrap_or(0);

    let d_sys_tot = sys_tot2.saturating_sub(sys_tot1);
    let d_sys_idl = sys_idl2.saturating_sub(sys_idl1);
    let d_self = self_ticks2.saturating_sub(self_ticks1);

    let sys_cpu = if d_sys_tot == 0 {
        0.0
    } else {
        (d_sys_tot.saturating_sub(d_sys_idl)) as f64 / d_sys_tot as f64 * 100.0
    };

    let num_cpus = num_cpus::get() as f64;
    let bot_cpu = if d_sys_tot == 0 {
        0.0
    } else {
        (d_self as f64 / d_sys_tot as f64) * 100.0 * num_cpus
    };

    (sys_cpu, bot_cpu)
}

fn format_memory(mb: f64) -> String {
    if mb < 1.0 {
        format!("{:.2} MB", mb)
    } else if mb < 1024.0 {
        format!("{:.2} MB", mb)
    } else {
        format!("{:.2} GB", mb / 1024.0)
    }
}

async fn query_astra_health(api_url: &str) -> String {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let resp = match client.get(format!("{}/health", api_url)).send().await {
        Ok(r) => r,
        Err(e) => return format!("\u{2514} Offline (Connection error: {})", e),
    };

    if !resp.status().is_success() {
        return format!("\u{2514} Offline (HTTP {})", resp.status());
    }

    let payload: AstraHealthResponse = match resp.json().await {
        Ok(p) => p,
        Err(e) => return format!("\u{2514} Error parsing stats: {}", e),
    };

    let Some(data) = payload.data else {
        return "\u{2514} Offline (No data returned)".to_string();
    };

    let heap_alloc_mb = data.memory.heap_alloc as f64 / (1024.0 * 1024.0);
    let heap_inuse_mb = data.memory.heap_inuse as f64 / (1024.0 * 1024.0);
    let stack_inuse_mb = data.memory.stack_inuse as f64 / (1024.0 * 1024.0);
    let gc_pause_ms = data.memory.gc_pause as f64 / 1_000_000.0;

    let uptime_clean = clean_go_duration(&data.uptime);
    let disk_total_mb = data.disk.total as f64 / (1024.0 * 1024.0);
    let disk_used_mb = data.disk.used as f64 / (1024.0 * 1024.0);
    let disk_pct = if data.disk.total > 0 {
        data.disk.used as f64 / data.disk.total as f64 * 100.0
    } else {
        0.0
    };

    let ig_cookie = if data.cookies.instagram {
        "Loaded"
    } else {
        "Missing"
    };
    let fb_cookie = if data.cookies.facebook {
        "Loaded"
    } else {
        "Missing"
    };

    format!(
        "\u{251c} Status: {} (Go {})\n\
        \u{251c} Uptime: {}\n\
        \u{251c} Requests: {}\n\
        \u{2502}  \u{251c} OK: {}\n\
        \u{2502}  \u{2514} Fail: {}\n\
        \u{251c} Cookies:\n\
        \u{2502}  \u{251c} Instagram: {}\n\
        \u{2502}  \u{2514} Facebook: {}\n\
        \u{251c} Goroutines: {}\n\
        \u{251c} Memory:\n\
        \u{2502}  \u{251c} Heap Alloc: {}\n\
        \u{2502}  \u{251c} Heap Inuse: {}\n\
        \u{2502}  \u{251c} Heap Objects: {}\n\
        \u{2502}  \u{251c} Stack Inuse: {}\n\
        \u{2502}  \u{2514} GC Cycles: {} (Pause: {:.2} ms)\n\
        \u{2514} Disk: {} / {} ({:.0}%)",
        data.status.to_uppercase(),
        data.version,
        uptime_clean,
        data.requests.total,
        data.requests.success,
        data.requests.failed,
        ig_cookie,
        fb_cookie,
        data.goroutines,
        format_memory(heap_alloc_mb),
        format_memory(heap_inuse_mb),
        data.memory.heap_objects,
        format_memory(stack_inuse_mb),
        data.memory.gc_cycles,
        gc_pause_ms,
        format_memory(disk_used_mb),
        format_memory(disk_total_mb),
        disk_pct
    )
}

pub async fn cmd_stats(state: &Arc<AppState>, client: &Client) -> anyhow::Result<InputMessage> {
    let stats = &state.bot_stats;

    let ping_start = Instant::now();
    let _ = client.resolve_username("telegram").await?;
    let ping_ms = ping_start.elapsed().as_micros() as f64 / 1000.0;
    let uptime = stats.uptime();
    let secs = uptime.as_secs();
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;

    let uptime_str = if days > 0 {
        format!("{} days {} hours {} mins", days, hours, mins)
    } else if hours > 0 {
        format!("{} hours {} mins", hours, mins)
    } else {
        format!("0 hours {} mins", mins)
    };

    let proc_self = read_proc("/proc/self/status").await.unwrap_or_default();
    let vm_rss_kb = parse_proc_val(&proc_self, "VmRSS:").unwrap_or(0);
    let bot_mem_mb = vm_rss_kb as f64 / 1024.0;

    let meminfo = read_proc("/proc/meminfo").await.unwrap_or_default();
    let mem_total_kb = parse_proc_val(&meminfo, "MemTotal:").unwrap_or(1);
    let mem_avail_kb = parse_proc_val(&meminfo, "MemAvailable:").unwrap_or(0);
    let mem_used_kb = mem_total_kb.saturating_sub(mem_avail_kb);
    let swap_total_kb = parse_proc_val(&meminfo, "SwapTotal:").unwrap_or(0);
    let swap_free_kb = parse_proc_val(&meminfo, "SwapFree:").unwrap_or(0);
    let swap_used_kb = swap_total_kb.saturating_sub(swap_free_kb);

    let loadavg = read_proc("/proc/loadavg").await.unwrap_or_default();
    let load_parts: Vec<&str> = loadavg.split_whitespace().collect();
    let load1 = load_parts.first().unwrap_or(&"?");
    let load5 = load_parts.get(1).unwrap_or(&"?");
    let load15 = load_parts.get(2).unwrap_or(&"?");

    let mem_used_mb = mem_used_kb as f64 / 1024.0;
    let mem_total_mb = mem_total_kb as f64 / 1024.0;
    let mem_pct = mem_used_kb as f64 / mem_total_kb as f64 * 100.0;
    let swap_used_mb = swap_used_kb as f64 / 1024.0;
    let swap_total_mb = swap_total_kb as f64 / 1024.0;
    let swap_pct = if swap_total_kb > 0 {
        swap_used_kb as f64 / swap_total_kb as f64 * 100.0
    } else {
        0.0
    };

    let ncpus = num_cpus::get();
    let active_jobs = state.bot_stats.active_jobs();

    let processed = stats.processed_total();
    let ok = stats.processed_ok();
    let fail = stats.processed_fail();
    let cache_hits = stats.cache_hits();

    let (cpu_pct, bot_cpu) = sample_cpu_stats(Duration::from_millis(150)).await;

    let swap_str = if swap_total_kb > 0 {
        format!(
            "Swap: {} / {} ({:.0}%)",
            format_memory(swap_used_mb),
            format_memory(swap_total_mb),
            swap_pct
        )
    } else {
        "Swap: 0 B / 512.00 MB (0%)".to_string()
    };

    let success_rate = if processed > 0 {
        (ok as f64 / processed as f64) * 100.0
    } else {
        100.0
    };
    let success_filled = ((success_rate / 10.0).floor() as usize).clamp(0, 10);
    let success_bar = format!("{}{}", "#".repeat(success_filled), "-".repeat(10 - success_filled));

    let cache_ratio = if processed > 0 {
        (cache_hits as f64 / processed as f64) * 100.0
    } else {
        0.0
    };
    let cache_filled = ((cache_ratio / 10.0).floor() as usize).clamp(0, 10);
    let cache_bar = format!("{}{}", "#".repeat(cache_filled), "-".repeat(10 - cache_filled));

    let astra_url = state
        .config
        .astra_api_url
        .as_deref()
        .unwrap_or("http://localhost:3000");
    let astra_status = query_astra_health(astra_url).await;

    let text = format!(
        "⚡️ <b>Nautilus Status Dashboard</b> (Ping: {ping_ms:.0}ms)\n\n\
         <b>Pipeline:</b>\n\
         ├ <b>Health:</b> {}\n\
         ├ <b>Processed:</b> {processed}\n\
         │  ├ <b>OK:</b> {ok}\n\
         │  └ <b>Failed:</b> {fail}\n\
         ├ <b>Success Rate:</b> [<code>{}</code>] {:.1}%\n\
         ├ <b>Active Jobs:</b> {active_jobs}\n\
         └ <b>Handler Errors:</b> {fail}\n\n\
         <b>Metrics:</b>\n\
         ├ <b>Total Downloads:</b> {processed}\n\
         ├ <b>Cache Hits:</b> {cache_hits}\n\
         └ <b>Cache Ratio:</b> [<code>{}</code>] {:.1}%\n\n\
         <b>System:</b>\n\
         ├ <b>CPU:</b> {ncpus} cores @ {cpu_pct:.1}%\n\
         ├ <b>RAM:</b> {} / {} ({mem_pct:.0}%)\n\
         ├ <b>{}</b>\n\
         └ <b>Load:</b> {load1} {load5} {load15}\n\n\
         <b>Bot:</b>\n\
         ├ <b>Username:</b> @{}\n\
         ├ <b>Memory:</b> {} MB (RSS: {:.2} KB)\n\
         ├ <b>CPU:</b> {bot_cpu:.1}%\n\
         └ <b>Uptime:</b> {}\n\n\
         <b>Astra Backend:</b>\n\
         {}",
        if fail == 0 { "Healthy" } else { "Errors" },
        success_bar,
        success_rate,
        cache_bar,
        cache_ratio,
        format_memory(mem_used_mb),
        format_memory(mem_total_mb),
        swap_str,
        stats.bot_username(),
        format!("{:.2}", bot_mem_mb),
        vm_rss_kb as f64,
        uptime_str,
        astra_status,
    );

    Ok(InputMessage::new().html(text))
}

async fn read_proc(path: &str) -> anyhow::Result<String> {
    tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("read {path}"))
}

fn parse_proc_val(content: &str, key: &str) -> Option<u64> {
    for line in content.lines() {
        if let Some(val) = line.strip_prefix(key) {
            let num_str = val.trim().split_whitespace().next()?;
            return num_str.parse().ok();
        }
    }
    None
}
