use super::{AppError, AppModel, Endpoint, REQUEST_LIMIT};
use axum::{
    body::Body,
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::Value;
use std::time::Duration;
use url::{Host, Url};

const DEFAULT_BASE: &str = "http://127.0.0.1:18989/v1";
const CONFIG_INVALID: &str = "gemini_app_gateway_config_invalid";

pub(super) struct Gateway {
    base: Url,
    authorization: HeaderValue,
    client: reqwest::Client,
}

impl Gateway {
    pub(super) fn configured() -> Result<Option<Self>, &'static str> {
        fn setting(name: &str) -> Result<Option<String>, &'static str> {
            match std::env::var(name) {
                Ok(value) => Ok(Some(value)),
                Err(std::env::VarError::NotPresent) => Ok(None),
                Err(_) => Err(CONFIG_INVALID),
            }
        }
        let base = setting("ABV_GEMINI_APP_BASE_URL")?;
        let key = setting("ABV_GEMINI_APP_API_KEY")?;
        if base.is_none() && key.is_none() {
            return Ok(None);
        }
        Self::new(
            base.as_deref().unwrap_or(DEFAULT_BASE),
            key.as_deref().unwrap_or(""),
        )
        .map(Some)
    }

    pub(super) fn new(base: &str, key: &str) -> Result<Self, &'static str> {
        let mut base = Url::parse(base.trim()).map_err(|_| CONFIG_INVALID)?;
        // The key and prompt must never be sent to a non-local server or a system HTTP proxy.
        let loopback = match base.host() {
            Some(Host::Ipv4(addr)) => addr.is_loopback(),
            Some(Host::Ipv6(addr)) => addr.is_loopback(),
            _ => false,
        };
        if base.scheme() != "http"
            || !loopback
            || base.port() == Some(0)
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || !matches!(base.path(), "" | "/" | "/v1" | "/v1/")
            || key.is_empty()
            || key.len() > 4096
            || !key.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(CONFIG_INVALID);
        }
        base.set_path("/v1/");
        let mut authorization =
            HeaderValue::from_str(&format!("Bearer {key}")).map_err(|_| CONFIG_INVALID)?;
        authorization.set_sensitive(true);
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(180))
            .build()
            .map_err(|_| CONFIG_INVALID)?;
        Ok(Self {
            base,
            authorization,
            client,
        })
    }

    pub(super) async fn generate(
        &self,
        endpoint: Endpoint,
        model: &AppModel,
        body: &Value,
    ) -> Result<Response, AppError> {
        let mut request = body.clone();
        request["model"] = Value::String(model.gateway_id.to_owned());
        let payload = serde_json::to_vec(&request)
            .map_err(|_| AppError::invalid("invalid_request", "Invalid JSON request."))?;
        if payload.len() > REQUEST_LIMIT {
            return Err(AppError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "gemini_app_request_too_large",
                "Gemini App request exceeds 4 MiB.",
            ));
        }
        let path = match endpoint {
            Endpoint::Chat => "chat/completions",
            Endpoint::Responses => "responses",
        };
        let upstream = self
            .client
            .post(self.base.join(path).expect("fixed API path"))
            .header(header::AUTHORIZATION, self.authorization.clone())
            .header(header::CONTENT_TYPE, "application/json")
            .body(payload)
            .send()
            .await
            .map_err(|err| {
                if err.is_timeout() {
                    AppError::timeout()
                } else {
                    AppError::upstream(
                        "gemini_app_gateway_unreachable",
                        "Could not connect to the local Gemini App2API gateway.",
                    )
                }
            })?;

        let status = upstream.status();
        if status == StatusCode::UNAUTHORIZED {
            return Err(AppError::upstream(
                "gemini_app_gateway_auth_failed",
                "Gemini App2API rejected its client key. Check the gateway key in DeepGeminiProxy's environment.",
            ));
        }
        let content_type = upstream
            .headers()
            .get(header::CONTENT_TYPE)
            .filter(|value| {
                let media = value
                    .to_str()
                    .unwrap_or_default()
                    .split(';')
                    .next()
                    .unwrap_or_default()
                    .trim();
                !status.is_redirection()
                    && (media.eq_ignore_ascii_case("application/json")
                        || (status.is_success() && media.eq_ignore_ascii_case("text/event-stream")))
            })
            .cloned()
            .ok_or_else(|| {
                AppError::upstream(
                    "gemini_app_gateway_bad_response",
                    "The local gateway did not return an OpenAI API response.",
                )
            })?;
        let retry_after = upstream.headers().get(header::RETRY_AFTER).cloned();
        // Relay bytes incrementally; no local-native retries or synthetic completion events.
        let mut response = (status, Body::from_stream(upstream.bytes_stream())).into_response();
        let headers = response.headers_mut();
        headers.insert(header::CONTENT_TYPE, content_type);
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        headers.insert(
            "x-upstream-provider",
            HeaderValue::from_static("gemini-app"),
        );
        if let Some(value) = retry_after {
            headers.insert(header::RETRY_AFTER, value);
        }
        Ok(response)
    }
}
