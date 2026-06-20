use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::Context;
use grammers_client::message::InputMessage;
use grammers_client::Client;

use crate::app::AppState;

#[derive(Debug)]
pub struct BotStats {
    boot_time: Instant,
    bot_username: String,
    bot_id: i64,
    processed_total: AtomicU64,
    processed_ok: AtomicU64,
    processed_fail: AtomicU64,
}

impl BotStats {
    pub fn new(bot_username: String, bot_id: i64) -> Self {
        Self {
            boot_time: Instant::now(),
            bot_username,
            bot_id,
            processed_total: AtomicU64::new(0),
            processed_ok: AtomicU64::new(0),
            processed_fail: AtomicU64::new(0),
        }
    }

    pub fn uptime(&self) -> Duration {
        self.boot_time.elapsed()
    }

    pub fn record_success(&self) {
        self.processed_total.fetch_add(1, Ordering::Relaxed);
        self.processed_ok.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_failure(&self) {
        self.processed_total.fetch_add(1, Ordering::Relaxed);
        self.processed_fail.fetch_add(1, Ordering::Relaxed);
    }

    pub fn bot_username(&self) -> &str { &self.bot_username }
    fn bot_id(&self) -> i64 { self.bot_id }
    fn processed_total(&self) -> u64 { self.processed_total.load(Ordering::Relaxed) }
    fn processed_ok(&self) -> u64 { self.processed_ok.load(Ordering::Relaxed) }
    fn processed_fail(&self) -> u64 { self.processed_fail.load(Ordering::Relaxed) }
}

async fn sample_cpu_pct(delay: Duration) -> f64 {
    let stat1 = read_proc("/proc/stat").await.unwrap_or_default();
    tokio::time::sleep(delay).await;
    let stat2 = read_proc("/proc/stat").await.unwrap_or_default();
    cpu_pct_from_stat(&stat1, &stat2)
}

fn cpu_pct_from_stat(prev: &str, cur: &str) -> f64 {
    let parse_cpu = |s: &str| -> Option<(u64, u64)> {
        let line = s.lines().next()?;
        let vals: Vec<u64> = line.split_whitespace().skip(1).filter_map(|v| v.parse().ok()).collect();
        if vals.len() < 5 { return None; }
        let total: u64 = vals.iter().sum();
        let idle = vals.get(3).copied().unwrap_or(0);
        Some((total, idle))
    };
    let (t1, i1) = parse_cpu(prev).unwrap_or((1, 0));
    let (t2, i2) = parse_cpu(cur).unwrap_or((1, 0));
    let dtotal = t2.saturating_sub(t1);
    let didle = i2.saturating_sub(i1);
    if dtotal == 0 { return 0.0; }
    (dtotal.saturating_sub(didle)) as f64 / dtotal as f64 * 100.0
}

fn format_memory(mb: f64) -> String {
    if mb < 1.0 { format!("{:.2} MB", mb) }
    else if mb < 1024.0 { format!("{:.2} MB", mb) }
    else { format!("{:.2} GB", mb / 1024.0) }
}

pub async fn cmd_status(state: &Arc<AppState>, client: &Client) -> anyhow::Result<InputMessage> {
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
    let active_jobs = state.max_concurrent_jobs - state.job_semaphore.available_permits();
    let max_jobs = state.max_concurrent_jobs;

    let processed = stats.processed_total();
    let ok = stats.processed_ok();
    let fail = stats.processed_fail();

    let cpu_pct = sample_cpu_pct(Duration::from_millis(150)).await;

    let swap_str = if swap_total_kb > 0 {
        format!("Swap: {} / {} ({:.0}%)", format_memory(swap_used_mb), format_memory(swap_total_mb), swap_pct)
    } else {
        "Swap: 0 B / 512.00 MB (0%)".to_string()
    };

    let text = format!(
        "Ping: {ping_ms:.0}ms\n\
        \n\
        Pipeline:\n\
        \u{251c} Health: {}\n\
        \u{251c} Processed: {processed}\n\
        \u{2502}  \u{251c} OK: {ok}\n\
        \u{2502}  \u{2514} Failed: {fail}\n\
        \u{251c} Active: {active_jobs} / {max_jobs}\n\
        \u{2514} Handler errors: {fail}\n\
        Metrics:\n\
        \u{251c} Downloads: {processed}\n\
        \u{2514} Memory: {:.2} KB\n\n\
        System:\n\
        \u{251c} CPU: {ncpus} cores @ {cpu_pct:.1}%\n\
        \u{251c} RAM: {} / {} ({mem_pct:.0}%)\n\
        \u{251c} {swap_str}\n\
        \u{2514} Load: {load1} {load5} {load15}\n\n\
        Bot:\n\
        \u{251c} ID: {}\n\
        \u{251c} Username: @{}\n\
        \u{251c} Memory: {} MB\n\
        \u{251c} CPU: N/A\n\
        \u{2514} Uptime: {uptime_str}",
        if fail == 0 { "Healthy" } else { "Errors" },
        vm_rss_kb as f64,
        format_memory(mem_used_mb), format_memory(mem_total_mb),
        stats.bot_id(),
        stats.bot_username(),
        format!("{:.2}", bot_mem_mb),
    );

    Ok(InputMessage::new().text(text))
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
