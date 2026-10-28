use std::time::Duration;

pub fn format_duration(dur: Duration) -> String {
    let ms = dur.as_millis();
    if ms < 1000 {
        format!("{ms} ms")
    } else if dur.as_secs() < 60 {
        let secs = dur.as_secs_f64();
        if (secs.fract() * 100.0).round() == 0.0 {
            format!("{:.0}s", secs)
        } else {
            format!("{secs:.2}s")
        }
    } else {
        let mins = dur.as_secs() / 60;
        let rem_secs = dur.as_secs() % 60;
        format!("{mins}m {rem_secs}s")
    }
}

pub fn format_bytes(bytes: u64) -> String {
    let mb = (bytes as f64) / (1024.0 * 1024.0);
    if mb >= 1024.0 {
        format!("{:.2} GB", mb / 1024.0)
    } else {
        format!("{:.2} MB", mb)
    }
}

pub fn format_resolution(dims: (i32, i32)) -> String {
    format!("{}x{}", dims.0, dims.1)
}
