use grammers_client::Client;
use grammers_client::message::InputMessage;
use grammers_session::types::PeerRef;

pub async fn cmd_start(client: &Client, chat: PeerRef) -> anyhow::Result<()> {
    client
        .send_message(
            chat,
            InputMessage::new()
                .text("Orion Bot — zero-disk media downloader.\nSend /dl <url> to download media."),
        )
        .await?;
    Ok(())
}
