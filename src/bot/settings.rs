use std::sync::Arc;

use dashmap::DashMap;
use grammers_client::message::InputMessage;

use crate::app::AppState;

#[derive(Debug, Clone)]
pub struct UserSettings {
    pub auto_mode: bool,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self { auto_mode: true }
    }
}

pub struct SettingsMap {
    inner: DashMap<i64, UserSettings>,
}

impl SettingsMap {
    pub fn new() -> Self {
        Self {
            inner: DashMap::new(),
        }
    }

    pub fn get(&self, user_id: i64) -> UserSettings {
        self.inner.get(&user_id).map(|s| s.clone()).unwrap_or_default()
    }

    pub fn set(&self, user_id: i64, settings: UserSettings) {
        self.inner.insert(user_id, settings);
    }

    pub fn set_auto(&self, user_id: i64, enabled: bool) {
        let mut s = self.get(user_id);
        s.auto_mode = enabled;
        self.inner.insert(user_id, s);
    }
}

pub fn cmd_settings(
    state: &Arc<AppState>,
    user_id: i64,
    args: &str,
) -> InputMessage {
    let (cmd, val) = args.trim().split_once(' ').unwrap_or((args.trim(), ""));

    match cmd {
        "auto" => {
            match val {
                "on" | "true" | "1" => {
                    state.settings.set_auto(user_id, true);
                    InputMessage::new().text("Auto mode: ON\nFormat selection will be skipped.")
                }
                "off" | "false" | "0" => {
                    state.settings.set_auto(user_id, false);
                    InputMessage::new().text("Auto mode: OFF\nFormat selection will appear on /dl.")
                }
                _ => {
                    let current = state.settings.get(user_id);
                    let status = if current.auto_mode { "ON" } else { "OFF" };
                    InputMessage::new().text(format!("Auto mode: {status}\nUsage: /settings auto <on|off>"))
                }
            }
        }
        _ => {
            let s = state.settings.get(user_id);
            let auto_status = if s.auto_mode { "ON" } else { "OFF" };
            InputMessage::new().text(format!(
                "Settings:\n\
                \u{251c} Auto mode: {auto_status}\n\
                \u{2514} Usage: /settings auto <on|off>"
            ))
        }
    }
}
