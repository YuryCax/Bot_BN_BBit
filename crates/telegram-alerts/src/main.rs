//! Telegram operator bridge: /pause, /resume, /flatten and /status → Zenoh.

use shared::packet::{OperatorAction, OperatorCommand};
use shared::time::utc_now_ns;
use shared::zenoh_ipc::ZenohPublisher;
use tracing::{info, warn};

fn parse_command(text: &str) -> Option<OperatorAction> {
    let command = text
        .split_whitespace()
        .next()?
        .split('@')
        .next()?;
    match command {
        "/pause" | "/halt" => Some(OperatorAction::HaltEntries),
        "/resume" => Some(OperatorAction::ResumeEntries),
        "/flatten" | "/flush" | "/cancel" => Some(OperatorAction::FlattenAll),
        "/status" => Some(OperatorAction::StatusPing),
        _ => None,
    }
}

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
    if chat.is_empty() {
        anyhow::bail!("TELEGRAM_CHAT_ID is required when TELEGRAM_BOT_TOKEN is set");
    }

    let publisher = ZenohPublisher::open().await?;
    let client = reqwest::Client::new();
    let mut offset: i64 = 0;
    info!("telegram-alerts ready chat_id={chat}");

    loop {
        let url =
            format!("https://api.telegram.org/bot{token}/getUpdates?timeout=25&offset={offset}");
        let response = match client.get(&url).send().await {
            Ok(response) => response,
            Err(error) => {
                warn!("telegram poll: {error}");
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                continue;
            }
        };
        let body: serde_json::Value = match response.error_for_status() {
            Ok(response) => match response.json().await {
                Ok(value) => value,
                Err(error) => {
                    warn!("telegram JSON: {error}");
                    continue;
                }
            },
            Err(error) => {
                warn!("telegram HTTP: {error}");
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                continue;
            }
        };
        if body["ok"].as_bool() != Some(true) {
            warn!("telegram API error: {body}");
            continue;
        }

        for update in body["result"].as_array().cloned().unwrap_or_default() {
            offset = update["update_id"].as_i64().unwrap_or(offset) + 1;
            let text = update["message"]["text"].as_str().unwrap_or("").trim();
            let from_chat = update["message"]["chat"]["id"]
                .as_i64()
                .unwrap_or_default()
                .to_string();
            if from_chat != chat {
                warn!("ignored Telegram command from unauthorized chat_id={from_chat}");
                continue;
            }
            let Some(action) = parse_command(text) else {
                continue;
            };
            let command = OperatorCommand {
                action,
                ts_ns: utc_now_ns(),
                source: "telegram".into(),
            };
            match publisher.publish_command(&command).await {
                Ok(()) => {
                    info!("telegram command {:?} published", action);
                    if let Err(error) = client
                        .post(format!("https://api.telegram.org/bot{token}/sendMessage"))
                        .json(&serde_json::json!({
                            "chat_id": from_chat,
                            "text": format!("OK: {:?} published to executor", action),
                        }))
                        .send()
                        .await
                    {
                        warn!("telegram acknowledgement: {error}");
                    }
                }
                Err(error) => warn!("publish command: {error}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_operator_commands() {
        assert_eq!(parse_command("/pause"), Some(OperatorAction::HaltEntries));
        assert_eq!(
            parse_command("/resume@my_bot"),
            Some(OperatorAction::ResumeEntries)
        );
        assert_eq!(
            parse_command("/flatten now"),
            Some(OperatorAction::FlattenAll)
        );
        assert_eq!(parse_command("/status"), Some(OperatorAction::StatusPing));
        assert_eq!(parse_command("/unknown"), None);
    }
}
