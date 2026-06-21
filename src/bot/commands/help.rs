use grammers_client::message::{Button, InputMessage};

pub fn cmd_help() -> InputMessage {
    let text = "❓ <b>Nautilus Bot Help & Commands Guide</b>\n\n\
                Here is a list of commands you can use:\n\n\
                🛠 <b>Commands:</b>\n\
                • /dl <code>&lt;link&gt;</code> - Download media from a supported URL.\n\
                • /l <code>&lt;link&gt;</code> - Shortcut / alias for /dl.\n\
                • /mp <code>&lt;link&gt;</code> - Download audio / MP3 format only.\n\
                • /audio <code>&lt;link&gt;</code> - Download audio format only.\n\
                • /stats - Check bot status, cache hits, and server health.\n\
                • /about - Read about the project tech stack and links.\n\
                • /help - Display this commands guide.\n\n\
                ✨ <b>Supported Platforms:</b>\n\
                • <b>TikTok</b> (Videos, Slideshows, Music)\n\
                • <b>Instagram</b> (Reels, Videos, Photos, Stories)\n\
                • <b>Facebook</b> (Videos, Photos, Groups)\n\
                • <b>YouTube</b> (Videos, Shorts, Audio)\n\
                • <b>Twitter/X</b> (Videos, Photos)\n\
                • <b>Threads</b> (Videos, Photos)\n\
                • <b>SoundCloud</b> (Audio/Tracks)\n\
                • <b>Spotify</b> (Tracks/Audio metadata)\n\
                • <b>Pinterest</b> (Videos, Photos)\n\
                • <b>LinkedIn</b> (Photos, Documents)\n\
                • <b>TeraBox</b> (Direct file downloads)";

    let buttons = vec![
        vec![
            Button::data("Back to Start", "cmd:start"),
            Button::data("About Project", "cmd:about"),
        ],
    ];

    InputMessage::new()
        .html(text)
        .reply_markup(grammers_client::message::ReplyMarkup::from_buttons(&buttons))
}
