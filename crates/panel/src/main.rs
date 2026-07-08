use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use shared::config::AppConfig;
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use tracing::info;

#[derive(Clone)]
struct AppState {
    config: Arc<AppConfig>,
}

#[derive(Serialize)]
struct Dashboard {
    futures_equity: f64,
    net_pnl_today: f64,
    pairs: Vec<PairRow>,
    halt_entries_futures: bool,
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
}

async fn dashboard(State(st): State<AppState>) -> Json<Dashboard> {
    Json(Dashboard {
        futures_equity: st.config.capital.initial_futures_deposit_usdt,
        net_pnl_today: 0.0,
        pairs: vec![
            PairRow {
                symbol: "BTCUSDT".into(),
                enabled: true,
                alloc_pct: 0.20,
            },
            PairRow {
                symbol: "ETHUSDT".into(),
                enabled: true,
                alloc_pct: 0.20,
            },
        ],
        halt_entries_futures: false,
    })
}

async fn halt_trading(Json(req): Json<HaltRequest>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "wallet": req.wallet,
        "halt_entries": req.halt_entries,
        "status": "queued"
    }))
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

async fn apply_suggestion(axum::extract::Path(id): axum::extract::Path<String>) -> Json<serde_json::Value> {
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

    let app = Router::new()
        .route("/api/v1/suggestions", get(list_suggestions))
        .route("/api/v1/suggestions/:id/apply", post(apply_suggestion))
        .route("/api/v1/phase3/enable", post(enable_phase3))
        .route("/health", get(|| async { "ok" }))
        .route("/api/v1/dashboard", get(dashboard))
        .route("/api/v1/trading/halt", post(halt_trading))
        .layer(TraceLayer::new_for_http())
        .with_state(AppState { config: cfg });

    info!("control-panel listening on {bind}");
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
