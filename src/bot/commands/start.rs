use grammers_client::Client;
use grammers_client::message::{Button, InputMessage};
use grammers_session::types::PeerRef;

pub fn cmd_start_msg() -> InputMessage {
    let text = "👋 <b>Welcome to Nautilus Bot!</b>\n\
                <i>Direct & zero-disk media downloader powered by Rust & Astra API.</i>\n\n\
                🚀 <b>Quick Start:</b>\n\
                • Send <code>/dl &lt;url&gt;</code> or <code>/l &lt;url&gt;</code> to download media.\n\
                • Send <code>/mp &lt;url&gt;</code> or <code>/audio &lt;url&gt;</code> to download audio only.\n\
                • Send any supported media link directly to check/download.\n\n\
                🤖 <i>Select an option below to navigate:</i>";

    let buttons = vec![
        vec![
            Button::data("Help & Guide", "cmd:help"),
            Button::data("About Project", "cmd:about"),
        ],
    ];

    InputMessage::new()
        .html(text)
        .reply_markup(grammers_client::message::ReplyMarkup::from_buttons(&buttons))
}

pub async fn cmd_start(client: &Client, chat: PeerRef) -> anyhow::Result<()> {
    client
        .send_message(chat, cmd_start_msg())
        .await?;
    Ok(())
}
