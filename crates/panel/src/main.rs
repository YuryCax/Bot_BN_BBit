use std::sync::Arc;

use axum::{
    extract::State,
    http::{header, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
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
    jwt_secret: Option<Arc<String>>,
    last_cmd_ns: Arc<tokio::sync::Mutex<u64>>,
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

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    exp: usize,
}

fn jwt_required(cfg: &AppConfig) -> bool {
    cfg.control_panel.auth_mode.eq_ignore_ascii_case("jwt")
}

async fn auth_middleware(
    State(st): State<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = req.uri().path().to_string();
    if path == "/health" || !jwt_required(&st.config) {
        return Ok(next.run(req).await);
    }
    let Some(secret) = st.jwt_secret.as_ref() else {
        warn!("auth_mode=jwt but secret env missing");
        return Err(StatusCode::UNAUTHORIZED);
    };
    let auth = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = auth
        .strip_prefix("Bearer ")
        .or_else(|| auth.strip_prefix("bearer "))
        .unwrap_or("");
    if token.is_empty() {
        return Err(StatusCode::UNAUTHORIZED);
    }
    // Accept raw shared secret (ops) or HS256 JWT
    if token == secret.as_str() {
        return Ok(next.run(req).await);
    }
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    match decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    ) {
        Ok(_) => Ok(next.run(req).await),
        Err(e) => {
            warn!("jwt reject: {e}");
            Err(StatusCode::UNAUTHORIZED)
        }
    }
}

async fn publish_cmd(st: &AppState, action: OperatorAction, source: &str) -> anyhow::Result<()> {
    let cmd = OperatorCommand {
        action,
        ts_ns: utc_now_ns(),
        source: source.into(),
    };
    st.publisher.publish_command(&cmd).await?;
    *st.last_cmd_ns.lock().await = cmd.ts_ns;
    Ok(())
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

async fn health(State(st): State<AppState>) -> impl IntoResponse {
    let last_cmd = *st.last_cmd_ns.lock().await;
    let now = utc_now_ns();
    let age_ms = if last_cmd == 0 {
        -1i64
    } else {
        ((now.saturating_sub(last_cmd)) / 1_000_000) as i64
    };
    Json(serde_json::json!({
        "status": "ok",
        "mode": st.config.deployment.mode,
        "auth_mode": st.config.control_panel.auth_mode,
        "last_command_age_ms": age_ms,
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let config_path = std::env::var("BOT_CONFIG").unwrap_or_else(|_| "config/config.toml".into());
    let cfg = Arc::new(AppConfig::load(&config_path)?);
    let bind = cfg.control_panel.bind_addr.clone();
    let publisher = Arc::new(ZenohPublisher::open().await?);

    let jwt_secret = if jwt_required(&cfg) {
        let env_name = &cfg.control_panel.jwt_secret_env;
        match std::env::var(env_name) {
            Ok(s) if !s.is_empty() => Some(Arc::new(s)),
            _ => {
                anyhow::bail!(
                    "control_panel.auth_mode=jwt requires env {env_name} (see deploy/secrets.env.example)"
                );
            }
        }
    } else {
        None
    };

    let state = AppState {
        config: Arc::clone(&cfg),
        publisher,
        halt: Arc::new(tokio::sync::Mutex::new(false)),
        jwt_secret,
        last_cmd_ns: Arc::new(tokio::sync::Mutex::new(0)),
    };

    let app = Router::new()
        .route("/api/v1/suggestions", get(list_suggestions))
        .route("/api/v1/suggestions/:id/apply", post(apply_suggestion))
        .route("/api/v1/phase3/enable", post(enable_phase3))
        .route("/health", get(health))
        .route("/api/v1/dashboard", get(dashboard))
        .route("/api/v1/trading/halt", post(halt_trading))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    info!(
        "control-panel listening on {bind} auth_mode={}",
        cfg.control_panel.auth_mode
    );
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
