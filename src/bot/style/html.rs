/// Escapes special HTML characters for Telegram HTML mode.
pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

pub fn bold(s: &str) -> String {
    format!("<b>{}</b>", html_escape(s))
}

pub fn italic(s: &str) -> String {
    format!("<i>{}</i>", html_escape(s))
}

pub fn code(s: &str) -> String {
    format!("<code>{}</code>", html_escape(s))
}

pub fn link(text: &str, url: &str) -> String {
    format!("<a href=\"{}\">{}</a>", html_escape(url), html_escape(text))
}
