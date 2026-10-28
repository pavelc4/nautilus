/// Calculates dynamic download concurrency based on available CPU hardware threads.
/// Since network downloading is I/O-bound, 2 concurrent workers per CPU thread
/// provides optimal throughput without overloading memory.
///
/// Can be overridden via the `DOWNLOAD_CONCURRENCY` environment variable.
/// Safe bounds: [2, 16] (guards against OOM on small VPS, prevents 429 rate limits on large servers).
pub fn dynamic_download_concurrency() -> usize {
    if let Ok(val) = std::env::var("DOWNLOAD_CONCURRENCY") {
        if let Ok(n) = val.parse::<usize>() {
            if n > 0 {
                return n;
            }
        }
    }

    let cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or_else(|_| num_cpus::get());

    (cpus * 2).clamp(2, 16)
}
