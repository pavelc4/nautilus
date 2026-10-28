pub fn normalize_url(url: &str) -> String {
    let parsed = match url::Url::parse(url) {
        Ok(u) => u,
        Err(_) => return url.to_string(),
    };
    let tracking: [&str; 9] = [
        "utm_source",
        "utm_medium",
        "utm_campaign",
        "utm_term",
        "utm_content",
        "igsh",
        "igshid",
        "fbclid",
        "gclid",
    ];
    let keep: Vec<_> = parsed
        .query_pairs()
        .filter(|(k, _)| !tracking.contains(&&k[..]))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if keep.is_empty() && parsed.query().is_some() {
        let mut clean = parsed;
        clean.set_query(None);
        return clean.as_str().to_string();
    }
    if keep.is_empty() {
        return parsed.as_str().to_string();
    }
    let mut clean = parsed;
    clean.query_pairs_mut().clear();
    for (k, v) in &keep {
        clean.query_pairs_mut().append_pair(k, v);
    }
    clean.as_str().to_string()
}
