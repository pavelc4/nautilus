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

pub fn format_user_link(user: &grammers_client::peer::User) -> String {
    let raw_name = match (user.first_name(), user.last_name()) {
        (Some(f), Some(l)) if !f.trim().is_empty() && !l.trim().is_empty() => {
            format!("{f} {l}")
        }
        (Some(f), _) if !f.trim().is_empty() => f.to_string(),
        (_, Some(l)) if !l.trim().is_empty() => l.to_string(),
        _ => user.username().unwrap_or("User").to_string(),
    };
    let escaped_name = html_escape(&raw_name);

    if let Some(username) = user.username() {
        format!("<a href=\"https://t.me/{username}\">{escaped_name}</a>")
    } else {
        let user_id = user.id().bare_id_unchecked();
        format!("<a href=\"tg://user?id={user_id}\">{escaped_name}</a>")
    }
}

