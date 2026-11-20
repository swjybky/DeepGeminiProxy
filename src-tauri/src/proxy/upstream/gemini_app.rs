//! In-process Gemini App provider; never uses the Antigravity token pool.
mod accounts;
mod embedded;
mod gateway;

pub(crate) use embedded::set_desktop_runtime;
static INSTANCE: OnceLock<Arc<GeminiApp>> = OnceLock::new();

pub(crate) async fn stop_embedded_if_started() {
    if let Some(gemini) = INSTANCE.get() {
        gemini.stop_embedded().await;
    }
}
mod robin;

pub(crate) use accounts::{AccountView, LoginStart, LoginView};
use axum::{
    body::Body,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

const MODEL_PREFIX: &str = "gemini-app-";
const REQUEST_LIMIT: usize = 4 * 1024 * 1024;

pub(crate) const MODELS: &[AppModel] = &[
    AppModel {
        public: "gemini-app-neon",
        gateway_id: "gemini-neon",
        mode: 3,
        attribution: Some("neon"),
        context: Some(512_000),
    },
    AppModel {
        public: "gemini-app-hydrogen",
        gateway_id: "gemini-hydrogen",
        mode: 1,
        attribution: Some("hydrogen"),
        context: Some(512_000),
    },
    AppModel {
        public: "gemini-app-flash-lite",
        gateway_id: "gemini-flash-lite",
        mode: 6,
        attribution: None,
        context: None,
    },
];
pub(crate) struct AppModel {
    pub(crate) public: &'static str,
    gateway_id: &'static str,
    mode: u64,
    attribution: Option<&'static str>,
    context: Option<u32>,
}
impl AppModel {
    pub(crate) fn metadata(&self) -> Value {
        let mut model = json!({"id": self.public, "object": "model", "owned_by": "gemini-app"});
        if let Some(context) = self.context {
            model["context_window"] = json!(context);
        }
        model
    }
}
fn normalize_id(id: &str) -> &str {
    let id = id.trim();
    id.strip_prefix("models/").unwrap_or(id)
}
pub(crate) fn is_app_model_id(id: &str) -> bool {
    normalize_id(id)
        .get(..MODEL_PREFIX.len())
        .is_some_and(|p| p.eq_ignore_ascii_case(MODEL_PREFIX))
}
pub(crate) fn find_model(id: &str) -> Option<&'static AppModel> {
    MODELS
        .iter()
        .find(|m| m.public.eq_ignore_ascii_case(normalize_id(id)))
}

#[derive(Clone, Copy)]
pub(crate) enum Endpoint {
    Chat,
    Responses,
}

pub(crate) struct GeminiApp {
    gateway: Result<Option<gateway::Gateway>, &'static str>,
    embedded: Option<embedded::Embedded>,
    pool: Result<accounts::Pool, &'static str>,
    client: reqwest::Client,
    capacity: Arc<tokio::sync::Semaphore>,
}
impl GeminiApp {
    pub(crate) fn shared() -> Result<Arc<Self>, &'static str> {
        static INIT: std::sync::Mutex<()> = std::sync::Mutex::new(());
        if let Some(provider) = INSTANCE.get() {
            return Ok(provider.clone());
        }
        let _guard = INIT.lock().map_err(|_| "account_store_unavailable")?;
        if let Some(provider) = INSTANCE.get() {
            return Ok(provider.clone());
        }
        let root =
            crate::modules::account::get_data_dir().map_err(|_| "account_store_unavailable")?;
        let gateway = gateway::Gateway::configured();
        let embedded = if matches!(gateway, Ok(None)) {
            embedded::Embedded::configured(&root.join("gemini-app2api"))
        } else {
            None
        };
        let provider = Arc::new(Self::with_backend(
            root.join("gemini-app-accounts.json"),
            gateway,
            embedded,
        )?);
        let _ = INSTANCE.set(provider.clone());
        Ok(provider)
    }
    #[cfg(test)]
    fn new(
        path: std::path::PathBuf,
        gateway: Result<Option<gateway::Gateway>, &'static str>,
    ) -> Result<Self, &'static str> {
        Self::with_backend(path, gateway, None)
    }
    fn with_backend(
        path: std::path::PathBuf,
        gateway: Result<Option<gateway::Gateway>, &'static str>,
        embedded: Option<embedded::Embedded>,
    ) -> Result<Self, &'static str> {
        let pool = if matches!(gateway, Ok(None)) && embedded.is_none() {
            Ok(accounts::Pool::new(path)?)
        } else {
            // Do not load or change any old local Google grants in App2API mode.
            Err("gemini_app_gateway_managed")
        };
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(180))
            .http2_keep_alive_interval(Duration::from_secs(15))
            .user_agent("DeepGeminiProxy/GeminiApp")
            .build()
            .map_err(|_| "gemini_app_client_unavailable")?;
        Ok(Self {
            gateway,
            embedded,
            pool,
            client,
            capacity: Arc::new(tokio::sync::Semaphore::new(8)),
        })
    }
    pub(crate) fn available(&self) -> bool {
        matches!(self.gateway, Ok(Some(_)))
            || self.embedded.is_some()
            || self.pool.as_ref().is_ok_and(accounts::Pool::available)
    }
    pub(crate) fn gateway_status(&self) -> Option<&'static str> {
        match self.gateway {
            Ok(Some(_)) => Some("configured"),
            Err(_) => Some("invalid"),
            Ok(None) => None,
        }
    }
    pub(crate) async fn managed_accounts(&self) -> Result<Vec<AccountView>, &'static str> {
        match &self.embedded {
            Some(engine) => engine.accounts().await,
            None => Ok(self.accounts()),
        }
    }
    pub(crate) async fn managed_remove(&self, id: uuid::Uuid) -> Result<(), &'static str> {
        match &self.embedded {
            Some(engine) => engine.remove_account(id).await,
            None => self.remove_account(id),
        }
    }
    pub(crate) async fn managed_login_status(
        &self,
        id: uuid::Uuid,
    ) -> Result<LoginView, &'static str> {
        match &self.embedded {
            Some(engine) => engine.login_status(id).await,
            None => self.login_status(id),
        }
    }
    pub(crate) async fn managed_cancel(&self, id: uuid::Uuid) -> Result<(), &'static str> {
        match &self.embedded {
            Some(engine) => engine.cancel_login(id).await,
            None => {
                self.cancel_login(id);
                Ok(())
            }
        }
    }
    pub(crate) async fn managed_start(self: &Arc<Self>) -> Result<LoginStart, &'static str> {
        match &self.embedded {
            Some(engine) => engine.begin_login().await,
            None => self.begin_login().await,
        }
    }
    pub(crate) async fn stop_embedded(&self) {
        if let Some(engine) = &self.embedded {
            engine.stop().await;
        }
    }
    fn native_pool(&self) -> Result<&accounts::Pool, &'static str> {
        self.pool.as_ref().map_err(|&code| code)
    }
}

#[derive(Debug)]
struct AppError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
}
impl AppError {
    fn new(status: StatusCode, code: &'static str, message: &'static str) -> Self {
        Self {
            status,
            code,
            message,
        }
    }
    fn upstream(code: &'static str, message: &'static str) -> Self {
        Self::new(StatusCode::BAD_GATEWAY, code, message)
    }
    fn invalid(code: &'static str, message: &'static str) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }
    fn no_accounts() -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "gemini_app_no_accounts",
            "Authorize a Gemini App account in DeepGeminiProxy.",
        )
    }
    fn busy() -> Self {
        Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            "gemini_app_busy",
            "Gemini App account capacity is busy.",
        )
    }
    fn quota() -> Self {
        Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            "gemini_app_quota",
            "The Gemini App upstream reported a quota limit.",
        )
    }
    fn timeout() -> Self {
        Self::new(
            StatusCode::GATEWAY_TIMEOUT,
            "gemini_app_timeout",
            "Gemini App generation timed out.",
        )
    }
    fn forbidden() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "gemini_app_upstream_denied",
            "The Google grant or account is not authorized for Gemini App.",
        )
    }
    fn model_mismatch() -> Self {
        Self::upstream(
            "gemini_app_model_mismatch",
            "Gemini App did not verify the requested model.",
        )
    }
    fn detail(&self) -> Value {
        let kind = if self.status.is_client_error() {
            "invalid_request_error"
        } else {
            "server_error"
        };
        json!({"message": self.message, "type": kind, "param": null, "code": self.code})
    }
    fn response(self) -> Response {
        (
            self.status,
            [("x-upstream-provider", "gemini-app")],
            Json(json!({"error":self.detail()})),
        )
            .into_response()
    }
}
pub(crate) fn model_not_found() -> Response {
    AppError::new(
        StatusCode::NOT_FOUND,
        "model_not_found",
        "Gemini App model is unknown or disabled.",
    )
    .response()
}

/// `None` delegates only models outside the reserved App namespace.
pub(crate) async fn dispatch(
    provider: Option<&GeminiApp>,
    enabled_models: &tokio::sync::RwLock<Option<Vec<String>>>,
    endpoint: Option<Endpoint>,
    body: &Value,
) -> Option<Response> {
    let id = body.get("model")?.as_str()?;
    if !is_app_model_id(id) {
        return None;
    }
    let Some(model) = find_model(id) else {
        return Some(model_not_found());
    };
    if !crate::proxy::common::model_gate::is_model_enabled(
        enabled_models.read().await.as_deref(),
        model.public,
    ) {
        return Some(model_not_found());
    }
    let Some(endpoint) = endpoint else {
        return Some(
            AppError::invalid(
                "gemini_app_unsupported_endpoint",
                "Use HTTP POST /v1/chat/completions or /v1/responses for Gemini App.",
            )
            .response(),
        );
    };
    let Some(provider) = provider else {
        return Some(AppError::no_accounts().response());
    };
    let result = match &provider.gateway {
        Ok(Some(gateway)) => gateway.generate(endpoint, model, body).await,
        Ok(None) => match &provider.embedded {
            Some(engine) => match engine.gateway().await {
                Ok(gateway) => gateway.generate(endpoint, model, body).await,
                Err(code) => Err(AppError::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    code,
                    "Could not start the bundled Gemini App2API engine.",
                )),
            },
            None => provider.generate(endpoint, model, body).await,
        },
        Err(code) => Err(AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            code,
            "Set a valid local Gemini App2API address and client key, then restart DeepGeminiProxy.",
        )),
    };
    Some(result.unwrap_or_else(AppError::response))
}

fn unsupported() -> AppError {
    AppError::invalid("gemini_app_unsupported_parameter", "Gemini App supports text-only requests with full history; tools, media, stored responses and sampling limits are not implemented.")
}
fn normalize(endpoint: Endpoint, body: &Value) -> Result<(String, u64, bool), AppError> {
    let chat = matches!(endpoint, Endpoint::Chat);
    let obj = body.as_object().ok_or_else(unsupported)?;
    let allowed: &[&str] = if chat {
        &[
            "model",
            "messages",
            "stream",
            "store",
            "reasoning_effort",
            "thinking_level",
        ]
    } else {
        &[
            "model",
            "input",
            "instructions",
            "stream",
            "store",
            "reasoning",
            "thinking_level",
        ]
    };
    if obj.keys().any(|k| !allowed.contains(&k.as_str()))
        || obj.get("store").is_some_and(|s| s.as_bool() != Some(false))
    {
        return Err(unsupported());
    }
    let stream = obj
        .get("stream")
        .map(|s| s.as_bool().ok_or_else(unsupported))
        .transpose()?
        .unwrap_or(false);
    let effort = if chat {
        obj.get("reasoning_effort")
    } else if let Some(reasoning) = obj.get("reasoning") {
        let options = reasoning.as_object().ok_or_else(unsupported)?;
        if options.len() != 1 || !options.contains_key("effort") {
            return Err(unsupported());
        }
        options.get("effort")
    } else {
        None
    };
    let effort = effort
        .map(|e| match e.as_str() {
            Some("none" | "minimal") => Ok(0),
            Some("low") => Ok(1),
            Some("medium" | "high" | "xhigh") => Ok(2),
            _ => Err(unsupported()),
        })
        .transpose()?;
    let explicit = obj
        .get("thinking_level")
        .map(|value| {
            value
                .as_u64()
                .filter(|n| matches!(n, 0 | 1 | 2 | 4))
                .ok_or_else(unsupported)
        })
        .transpose()?;
    if explicit.zip(effort).is_some_and(|(a, b)| a != b) {
        return Err(unsupported());
    }
    let mut transcript = Vec::new();
    if let Some(instructions) = obj.get("instructions") {
        transcript
            .push(json!({"role":"system", "text": instructions.as_str().ok_or_else(unsupported)?}));
    }
    let input = obj
        .get(if chat { "messages" } else { "input" })
        .ok_or_else(unsupported)?;
    if let Some(text) = input.as_str().filter(|_| !chat) {
        transcript.push(json!({"role":"user", "text":text}));
    } else {
        let messages = input
            .as_array()
            .filter(|messages| !messages.is_empty() && messages.len() <= 1024)
            .ok_or_else(unsupported)?;
        for value in messages {
            let item = value.as_object().ok_or_else(unsupported)?;
            if item
                .keys()
                .any(|k| !matches!(k.as_str(), "role" | "content" | "type"))
                || item
                    .get("type")
                    .is_some_and(|t| t.as_str() != Some("message"))
            {
                return Err(unsupported());
            }
            let role = value
                .get("role")
                .and_then(Value::as_str)
                .filter(|role| matches!(*role, "system" | "developer" | "user" | "assistant"))
                .ok_or_else(unsupported)?;
            let content = value.get("content").ok_or_else(unsupported)?;
            let text = if let Some(text) = content.as_str() {
                text.to_owned()
            } else {
                let mut text = String::new();
                for part in content.as_array().ok_or_else(unsupported)? {
                    let expected = if chat {
                        "text"
                    } else if role == "assistant" {
                        "output_text"
                    } else {
                        "input_text"
                    };
                    if part.get("type").and_then(Value::as_str) != Some(expected)
                        || part.as_object().is_none_or(|part| {
                            part.keys().any(|k| !matches!(k.as_str(), "type" | "text"))
                        })
                    {
                        return Err(unsupported());
                    }
                    text.push_str(
                        part.get("text")
                            .and_then(Value::as_str)
                            .ok_or_else(unsupported)?,
                    );
                }
                text
            };
            transcript.push(json!({"role":role, "text":text}));
        }
    }
    if !transcript.iter().any(|t| {
        t["role"] == "user"
            && t["text"]
                .as_str()
                .is_some_and(|text| !text.trim().is_empty())
    }) {
        return Err(AppError::invalid(
            "gemini_app_user_input_required",
            "A nonempty user message is required.",
        ));
    }
    let prompt = format!("Continue the following conversation with the next assistant reply. The JSON array records messages in order and separates role from text.\n{}", Value::Array(transcript));
    if prompt.len() > REQUEST_LIMIT {
        return Err(AppError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "gemini_app_request_too_large",
            "Gemini App prompt exceeds 4 MiB.",
        ));
    }
    Ok((prompt, explicit.or(effort).unwrap_or(2), stream))
}

struct Delivery {
    id: String,
    item: String,
    model: &'static str,
    now: i64,
    endpoint: Endpoint,
    sequence: u32,
}
impl Delivery {
    fn new(model: &'static str, endpoint: Endpoint) -> Self {
        Self {
            id: format!(
                "{}{}",
                if matches!(endpoint, Endpoint::Chat) {
                    "chatcmpl-"
                } else {
                    "resp_"
                },
                uuid::Uuid::new_v4().simple()
            ),
            item: format!("msg_{}", uuid::Uuid::new_v4().simple()),
            model,
            now: chrono::Utc::now().timestamp(),
            endpoint,
            sequence: 0,
        }
    }
    fn item(&self, status: &str, content: Value) -> Value {
        json!({"id":self.item,"type":"message","role":"assistant","status":status,"content":content})
    }
    fn response(&self, status: &str, output: Value, error: Value) -> Value {
        json!({"id":self.id,"object":"response","created_at":self.now,"model":self.model,"status":status,"output":output,"error":error,"incomplete_details":if status == "incomplete" {json!({"reason":"upstream_interrupted"})} else {Value::Null}})
    }
    fn text(text: &str) -> Value {
        json!({"type":"output_text","text":text,"annotations":[]})
    }
    fn final_json(&self, full: &str) -> Value {
        match self.endpoint {
            Endpoint::Chat => {
                json!({"id":self.id,"object":"chat.completion","created":self.now,"model":self.model,"choices":[{"index":0,"message":{"role":"assistant","content":full},"finish_reason":"stop"}]})
            }
            Endpoint::Responses => self.response(
                "completed",
                json!([self.item("completed", json!([Self::text(full)]))]),
                Value::Null,
            ),
        }
    }
    fn chat(&self, delta: Value, reason: Option<&str>) -> String {
        format!(
            "data: {}\n\n",
            json!({"id":self.id,"object":"chat.completion.chunk","created":self.now,"model":self.model,"choices":[{"index":0,"delta":delta,"finish_reason":reason}]})
        )
    }
    fn event(&mut self, kind: &str, mut payload: Value) -> String {
        payload["type"] = json!(kind);
        payload["sequence_number"] = json!(self.sequence);
        self.sequence += 1;
        format!("event: {kind}\ndata: {payload}\n\n")
    }
    fn start(&mut self) -> String {
        match self.endpoint {
            Endpoint::Chat => self.chat(json!({"role":"assistant"}), None),
            Endpoint::Responses => {
                let created =
                    json!({"response":self.response("in_progress",json!([]),Value::Null)});
                let item = json!({"output_index":0,"item":self.item("in_progress",json!([]))});
                let part = json!({"item_id":self.item,"output_index":0,"content_index":0,"part":Self::text("")});
                self.event("response.created", created.clone())
                    + &self.event("response.in_progress", created)
                    + &self.event("response.output_item.added", item)
                    + &self.event("response.content_part.added", part)
            }
        }
    }
    fn delta(&mut self, text: &str) -> String {
        match self.endpoint {
            Endpoint::Chat => self.chat(json!({"content":text}), None),
            Endpoint::Responses => self.event(
                "response.output_text.delta",
                json!({"item_id":self.item,"output_index":0,"content_index":0,"delta":text}),
            ),
        }
    }
    fn finish(&mut self, full: &str, error: Option<AppError>) -> String {
        match (self.endpoint, error) {
            (Endpoint::Chat, None) => self.chat(json!({}), Some("stop")) + "data: [DONE]\n\n",
            (Endpoint::Chat, Some(error)) => format!(
                "data: {}\n\ndata: [DONE]\n\n",
                json!({"error":error.detail(),"incomplete":true})
            ),
            (Endpoint::Responses, error) => {
                let status = if error.is_some() {
                    "incomplete"
                } else {
                    "completed"
                };
                let kind = if error.is_some() {
                    "response.incomplete"
                } else {
                    "response.completed"
                };
                let item = self.item(status, json!([Self::text(full)]));
                let final_ev = json!({"response":self.response(status,json!([item.clone()]),error.map_or(Value::Null,|e|e.detail()))});
                let text =
                    json!({"item_id":self.item,"output_index":0,"content_index":0,"text":full});
                let part = json!({"item_id":self.item,"output_index":0,"content_index":0,"part":Self::text(full)});
                self.event("response.output_text.done", text)
                    + &self.event("response.content_part.done", part)
                    + &self.event(
                        "response.output_item.done",
                        json!({"output_index":0,"item":item}),
                    )
                    + &self.event(kind, final_ev)
            }
        }
    }
}

impl GeminiApp {
    async fn generate(
        &self,
        endpoint: Endpoint,
        model: &'static AppModel,
        body: &Value,
    ) -> Result<Response, AppError> {
        let (prompt, thinking, streaming) = normalize(endpoint, body)?;
        let permit = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| AppError::busy())?;
        let (token, account) = self
            .native_pool()
            .map_err(|_| AppError::no_accounts())?
            .acquire()
            .await?;
        let mut turn = robin::open(&self.client, &token, account, model, &prompt, thinking).await?;
        let mut delivery = Delivery::new(model.public, endpoint);
        let mut full = turn.next(model).await?.ok_or_else(|| {
            AppError::upstream("gemini_app_incomplete", "Gemini App returned no text.")
        })?;
        let headers = [
            ("cache-control", "no-store"),
            ("x-upstream-provider", "gemini-app"),
        ];
        if !streaming {
            while let Some(delta) = turn.next(model).await? {
                full.push_str(&delta);
            }
            return Ok((headers, Json(delivery.final_json(&full))).into_response());
        }
        let stream = async_stream::stream! {
            let _permit = permit;
            yield Ok::<_, std::convert::Infallible>(delivery.start() + &delivery.delta(&full));
            let error = loop {
                match turn.next(model).await {
                    Ok(Some(delta)) => { full.push_str(&delta); yield Ok(delivery.delta(&delta)); }
                    Ok(None) => break None,
                    Err(error) => break Some(error),
                }
            };
            yield Ok(delivery.finish(&full,error));
        };
        Ok((
            headers,
            [("content-type", "text/event-stream")],
            Body::from_stream(stream),
        )
            .into_response())
    }
}

#[cfg(test)]
mod tests;
