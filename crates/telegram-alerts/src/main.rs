//! Telegram operator bridge — publishes halt/resume/flatten on Zenoh `system/command`.
//! Polling getUpdates (long poll). Commands: /status /pause /resume /flatten /cancel

use shared::packet::{OperatorAction, OperatorCommand};
use shared::time::utc_now_ns;
use shared::zenoh_ipc::ZenohPublisher;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();
    let token = std::env::var("TELEGRAM_BOT_TOKEN").unwrap_or_default();
    let chat = std::env::var("TELEGRAM_CHAT_ID").unwrap_or_default();
    if token.is_empty() {
        info!("telegram-alerts: TELEGRAM_BOT_TOKEN not set — idle (Ctrl+C)");
        tokio::signal::ctrl_c().await?;
        return Ok(());
    }

    let publisher = ZenohPublisher::open().await?;
    let client = reqwest::Client::new();
    let mut offset: i64 = 0;
    info!("telegram-alerts ready chat_id={chat}");

    loop {
        let url = format!(
            "https://api.telegram.org/bot{token}/getUpdates?timeout=25&offset={offset}"
        );
        let resp = match client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                warn!("telegram poll: {e}");
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                continue;
            }
        };
        let body: serde_json::Value = match resp.json().await {
            Ok(v) => v,
            Err(e) => {
                warn!("telegram json: {e}");
                continue;
            }
        };
        for upd in body["result"].as_array().cloned().unwrap_or_default() {
            offset = upd["update_id"].as_i64().unwrap_or(offset) + 1;
            let text = upd["message"]["text"].as_str().unwrap_or("").trim();
            let from_chat = upd["message"]["chat"]["id"].as_i64().unwrap_or(0).to_string();
            if !chat.is_empty() && from_chat != chat {
                continue;
            }
            let action = match text {
                "/pause" | "/halt" => Some(OperatorAction::HaltEntries),
                "/resume" => Some(OperatorAction::ResumeEntries),
                "/flatten" | "/flush" | "/cancel" => Some(OperatorAction::FlattenAll),
                "/status" => Some(OperatorAction::StatusPing),
                _ => None,
            };
            if let Some(action) = action {
                let cmd = OperatorCommand {
                    action,
                    ts_ns: utc_now_ns(),
                    source: "telegram".into(),
                };
                match publisher.publish_command(&cmd).await {
                    Ok(()) => {
                        info!("telegram cmd {:?} published", action);
                        let _ = client
                            .post(format!("https://api.telegram.org/bot{token}/sendMessage"))
                            .json(&serde_json::json!({
                                "chat_id": from_chat,
                                "text": format!("OK {:?} published to executor", action),
                            }))
                            .send()
                            .await;
                    }
                    Err(e) => warn!("publish command: {e}"),
                }
            }
        }
    }
}
