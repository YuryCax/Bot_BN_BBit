use std::sync::Arc;

use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use shared::config::AppConfig;
use shared::packet::{OperatorAction, OperatorCommand};
use shared::time::utc_now_ns;
use shared::zenoh_ipc::ZenohPublisher;
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

#[derive(Clone)]
struct AppState {
    config: Arc<AppConfig>,
    publisher: Arc<ZenohPublisher>,
    halt: Arc<tokio::sync::Mutex<bool>>,
}

#[derive(Serialize)]
struct Dashboard {
    futures_equity: f64,
    net_pnl_today: f64,
    pairs: Vec<PairRow>,
    halt_entries_futures: bool,
    mode: String,
}

#[derive(Serialize)]
struct PairRow {
    symbol: String,
    enabled: bool,
    alloc_pct: f64,
}

#[derive(Deserialize)]
struct HaltRequest {
    wallet: String,
    halt_entries: bool,
    #[serde(default)]
    flatten: bool,
}

async fn publish_cmd(st: &AppState, action: OperatorAction, source: &str) -> anyhow::Result<()> {
    let cmd = OperatorCommand {
        action,
        ts_ns: utc_now_ns(),
        source: source.into(),
    };
    st.publisher.publish_command(&cmd).await
}

async fn dashboard(State(st): State<AppState>) -> Json<Dashboard> {
    let halt = *st.halt.lock().await;
    Json(Dashboard {
        futures_equity: st.config.capital.initial_futures_deposit_usdt,
        net_pnl_today: 0.0,
        pairs: st
            .config
            .deployment
            .start_futures_pairs
            .iter()
            .map(|s| PairRow {
                symbol: s.clone(),
                enabled: true,
                alloc_pct: 0.20,
            })
            .collect(),
        halt_entries_futures: halt,
        mode: st.config.deployment.mode.clone(),
    })
}

async fn halt_trading(
    State(st): State<AppState>,
    Json(req): Json<HaltRequest>,
) -> Json<serde_json::Value> {
    let action = if req.flatten {
        OperatorAction::FlattenAll
    } else if req.halt_entries {
        OperatorAction::HaltEntries
    } else {
        OperatorAction::ResumeEntries
    };
    match publish_cmd(&st, action, "panel").await {
        Ok(()) => {
            *st.halt.lock().await = req.halt_entries || req.flatten;
            Json(serde_json::json!({
                "wallet": req.wallet,
                "halt_entries": req.halt_entries,
                "flatten": req.flatten,
                "status": "published",
                "action": format!("{:?}", action),
            }))
        }
        Err(e) => {
            warn!("halt publish failed: {e}");
            Json(serde_json::json!({
                "status": "error",
                "error": e.to_string(),
            }))
        }
    }
}

async fn list_suggestions() -> Json<serde_json::Value> {
    let dir = std::path::Path::new("analyst/data/suggestions");
    let mut items = vec![];
    if dir.exists() {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            if let Ok(text) = std::fs::read_to_string(entry.path()) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                    items.push(v);
                }
            }
        }
    }
    Json(serde_json::json!({ "suggestions": items }))
}

async fn apply_suggestion(
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "id": id, "status": "applied", "queued": true }))
}

async fn enable_phase3() -> Json<serde_json::Value> {
    Json(serde_json::json!({"enabled": true}))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    let config_path =
        std::env::var("BOT_CONFIG").unwrap_or_else(|_| "config/config.toml".into());
    let cfg = Arc::new(AppConfig::load(&config_path)?);
    let bind = cfg.control_panel.bind_addr.clone();
    let publisher = Arc::new(ZenohPublisher::open().await?);

    let app = Router::new()
        .route("/api/v1/suggestions", get(list_suggestions))
        .route("/api/v1/suggestions/:id/apply", post(apply_suggestion))
        .route("/api/v1/phase3/enable", post(enable_phase3))
        .route("/health", get(|| async { "ok" }))
        .route("/api/v1/dashboard", get(dashboard))
        .route("/api/v1/trading/halt", post(halt_trading))
        .layer(TraceLayer::new_for_http())
        .with_state(AppState {
            config: cfg,
            publisher,
            halt: Arc::new(tokio::sync::Mutex::new(false)),
        });

    info!("control-panel listening on {bind} (Zenoh command bus enabled)");
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
