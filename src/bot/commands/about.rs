use grammers_client::message::{Button, InputMessage};

pub fn cmd_about() -> InputMessage {
    let text = "😼 <b>About Nautilus Bot</b>\n\n\
                Nautilus is an open-source Telegram Bot designed for lightning-fast media downloading.\n\n\
                🚀 <b>Tech Stack:</b>\n\
                • <b>Frontend:</b> Rust with <a href=\"https://codeberg.org/Lonami/grammers\">grammers</a> library.\n\
                • <b>Backend API:</b> Go with <a href=\"https://github.com/pavelc4/astra.git\">Astra</a> engine.\n\
                • <b>Design:</b> Zero-disk, streaming upload directly to MTProto.\n\n\
                💡 <b>Fun Fact:</b> Nautilus is written entirely in Rust, using the MTProto protocol via grammers, and a custom scraper API built completely from scratch.\n\n\
                🧑‍💻 <b>Developer:</b> @Pavellc\n\n\
                Feel free to support the developer or explore the source code below!";

    let buttons = vec![
        vec![
            Button::url("Source Bot", "https://github.com/pavelc4/nautilus"),
            Button::url("Source API", "https://github.com/pavelc4/astra.git"),
        ],
        vec![
            Button::url("Support Developer", "https://github.com/sponsors/pavelc4"),
        ],
        vec![
            Button::data("Back to Start", "cmd:start"),
            Button::data("Command Help", "cmd:help"),
        ],
    ];

    InputMessage::new()
        .html(text)
        .reply_markup(grammers_client::message::ReplyMarkup::from_buttons(&buttons))
}
