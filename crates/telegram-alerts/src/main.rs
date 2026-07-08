use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();
    let token = std::env::var("TELEGRAM_BOT_TOKEN").unwrap_or_default();
    let chat = std::env::var("TELEGRAM_CHAT_ID").unwrap_or_default();
    if token.is_empty() {
        info!("telegram-alerts: TELEGRAM_BOT_TOKEN not set, running idle");
    } else {
        info!("telegram-alerts ready chat_id={chat}");
    }
    tokio::signal::ctrl_c().await?;
    Ok(())
}
