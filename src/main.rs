use std::{env, net::SocketAddr};

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use nonprofit_domain_onboarding::onboarding::{
    activation_for, onboard_nonprofit, verify_signature, ClientError, InfraiClient,
};
use serde::Deserialize;
use serde_json::json;

#[derive(Clone)]
struct AppState {
    webhook_secret: String,
}

#[derive(Deserialize)]
struct VerifiedEvent {
    domain: String,
    verified: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.get(1).map(String::as_str) == Some("onboard") {
        let domain = required_arg(&args, 2, "domain")?;
        let nonprofit_id = required_arg(&args, 3, "nonprofit_id")?;
        let webhook_url = required_arg(&args, 4, "webhook_url")?;
        let secret = env::var("WEBHOOK_SECRET")?;
        let client = InfraiClient::from_env()?;
        let result = onboard_nonprofit(
            &client,
            nonprofit_id,
            domain,
            "tenant.product.example",
            webhook_url,
            &secret,
        )
        .await?;
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    let state = AppState {
        webhook_secret: env::var("WEBHOOK_SECRET")?,
    };
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/webhooks/infrai", post(verified_webhook))
        .with_state(state);
    let address: SocketAddr = "127.0.0.1:3000".parse()?;
    println!("listening on http://{address}");
    axum::serve(tokio::net::TcpListener::bind(address).await?, app).await?;
    Ok(())
}

fn required_arg<'a>(args: &'a [String], index: usize, name: &str) -> Result<&'a str, String> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| format!("missing {name}"))
}

async fn verified_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let signature = headers
        .get("x-infrai-signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if verify_signature(state.webhook_secret.as_bytes(), &body, signature).is_err() {
        return (StatusCode::UNAUTHORIZED, Json(json!({"accepted": false}))).into_response();
    }
    let event: VerifiedEvent = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, Json(json!({"accepted": false}))).into_response()
        }
    };
    (
        StatusCode::OK,
        Json(
            json!({"accepted": true, "activation": activation_for(&event.domain, event.verified)}),
        ),
    )
        .into_response()
}

#[allow(dead_code)]
fn map_client_error(error: &ClientError) -> StatusCode {
    match error {
        ClientError::Rejected { status, .. } if *status < 500 => {
            StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_REQUEST)
        }
        ClientError::MissingKey => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::BAD_GATEWAY,
    }
}
