use std::sync::Arc;
use grammers_client::message::InputMessage;
use crate::app::AppState;

pub fn cmd_settings(state: &Arc<AppState>, user_id: i64, args: &str) -> InputMessage {
    let (cmd, val) = args.trim().split_once(' ').unwrap_or((args.trim(), ""));

    match cmd {
        "auto" => match val {
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
                InputMessage::new().text(format!(
                    "Auto mode: {status}\nUsage: /settings auto <on|off>"
                ))
            }
        },
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
