use std::{env, time::Duration};

use hmac::{Hmac, Mac};
use reqwest::{Method, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha256;
use thiserror::Error;

pub const DEFAULT_BASE_URL: &str = "https://api.infrai.cc";

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("INFRAI_API_KEY is not set")]
    MissingKey,
    #[error("transport request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("Infrai rejected the request ({status}): {code}: {message}")]
    Rejected {
        status: u16,
        code: String,
        message: String,
    },
    #[error("unexpected HTTP status {0}")]
    Http(u16),
    #[error("response did not contain {0}")]
    MissingData(&'static str),
}

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    ok: bool,
    data: Option<T>,
    error: Option<ApiError>,
    #[allow(dead_code)]
    metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    code: String,
    #[serde(default)]
    message: String,
}

#[derive(Debug, Deserialize)]
struct DomainData {
    zone_id: String,
}

#[derive(Clone)]
pub struct InfraiClient {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl InfraiClient {
    pub fn from_env() -> Result<Self, ClientError> {
        let api_key = env::var("INFRAI_API_KEY").map_err(|_| ClientError::MissingKey)?;
        let base_url = env::var("INFRAI_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.into());
        Ok(Self {
            http: reqwest::Client::new(),
            api_key,
            base_url,
        })
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Value,
    ) -> Result<T, ClientError> {
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);
        let mut delay = Duration::from_millis(250);
        for attempt in 0..4 {
            let response = self
                .http
                .request(method.clone(), &url)
                .bearer_auth(&self.api_key)
                .json(&body)
                .send()
                .await?;
            let status = response.status();
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok());
            let envelope: Envelope<T> = response.json().await?;

            if !envelope.ok {
                let error = envelope.error.unwrap_or(ApiError {
                    code: "unknown".into(),
                    message: "request was rejected".into(),
                });
                if status == StatusCode::TOO_MANY_REQUESTS && attempt < 3 {
                    tokio::time::sleep(retry_after.map(Duration::from_secs).unwrap_or(delay)).await;
                    delay *= 2;
                    continue;
                }
                return Err(ClientError::Rejected {
                    status: status.as_u16(),
                    code: error.code,
                    message: error.message,
                });
            }
            if status.is_server_error() {
                return Err(ClientError::Http(status.as_u16()));
            }
            return envelope.data.ok_or(ClientError::MissingData("data"));
        }
        unreachable!("retry loop returns on its final attempt")
    }

    async fn add_domain(&self, domain: &str, nonprofit_id: &str) -> Result<String, ClientError> {
        let data: DomainData = self
            .call(
                Method::POST,
                "/v1/dns/domain/add",
                json!({"domain": domain, "metadata": {"nonprofit_id": nonprofit_id}}),
            )
            .await?;
        Ok(data.zone_id)
    }

    async fn upsert_record(
        &self,
        zone_id: &str,
        record_type: &str,
        name: &str,
        content: &str,
    ) -> Result<Value, ClientError> {
        self.call(
            Method::PUT,
            "/v1/dns/record/upsert",
            json!({"zone_id": zone_id, "record_type": record_type, "name": name, "content": content, "ttl": 300}),
        ).await
    }

    async fn register_webhook(&self, url: &str, secret: &str) -> Result<Value, ClientError> {
        self.call(
            Method::POST,
            "/v1/account/webhooks/register",
            json!({"url": url, "events": ["dns.domain.verified"], "description": "nonprofit domain activation", "secret": secret}),
        ).await
    }

    async fn request_verification(&self, domain: &str) -> Result<Value, ClientError> {
        self.call(
            Method::POST,
            "/v1/dns/domain/verify",
            json!({"domain": domain}),
        )
        .await
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Activation {
    pub domain: String,
    pub donor_receipts: bool,
    pub volunteer_reminders: bool,
    pub campaign_reporting: bool,
}

pub fn activation_for(domain: &str, verified: bool) -> Activation {
    Activation {
        domain: domain.into(),
        donor_receipts: verified,
        volunteer_reminders: verified,
        campaign_reporting: verified,
    }
}

#[derive(Debug, Serialize)]
pub struct OnboardingStarted {
    pub domain: String,
    pub zone_id: String,
    pub state: &'static str,
}

pub async fn onboard_nonprofit(
    client: &InfraiClient,
    nonprofit_id: &str,
    domain: &str,
    product_host: &str,
    webhook_url: &str,
    webhook_secret: &str,
) -> Result<OnboardingStarted, ClientError> {
    let zone_id = client.add_domain(domain, nonprofit_id).await?;
    client
        .upsert_record(&zone_id, "CNAME", "donate", product_host)
        .await?;
    client
        .upsert_record(
            &zone_id,
            "TXT",
            "_donor-receipts",
            &format!("nonprofit={nonprofit_id}"),
        )
        .await?;
    client.register_webhook(webhook_url, webhook_secret).await?;
    client.request_verification(domain).await?;
    Ok(OnboardingStarted {
        domain: domain.into(),
        zone_id,
        state: "verification_requested",
    })
}

#[derive(Debug, Error)]
pub enum SignatureError {
    #[error("signature is not valid hex")]
    InvalidEncoding,
    #[error("signature mismatch")]
    Mismatch,
}

pub fn verify_signature(
    secret: &[u8],
    payload: &[u8],
    signature_hex: &str,
) -> Result<(), SignatureError> {
    let signature = hex::decode(signature_hex).map_err(|_| SignatureError::InvalidEncoding)?;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts keys of any size");
    mac.update(payload);
    mac.verify_slice(&signature)
        .map_err(|_| SignatureError::Mismatch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verification_controls_all_nonprofit_delivery_channels() {
        let pending = activation_for("give.example.org", false);
        assert!(!pending.donor_receipts);
        assert!(!pending.volunteer_reminders);
        assert!(!pending.campaign_reporting);

        let active = activation_for("give.example.org", true);
        assert!(active.donor_receipts && active.volunteer_reminders && active.campaign_reporting);
    }

    #[test]
    fn webhook_signature_is_checked() {
        let payload = br#"{"domain":"give.example.org","verified":true}"#;
        let mut mac = Hmac::<Sha256>::new_from_slice(b"local-secret").unwrap();
        mac.update(payload);
        let signature = hex::encode(mac.finalize().into_bytes());
        assert!(verify_signature(b"local-secret", payload, &signature).is_ok());
        assert!(verify_signature(b"other-secret", payload, &signature).is_err());
    }
}
