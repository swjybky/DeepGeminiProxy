use crate::proxy::{
    server::AppState,
    upstream::gemini_app::{AccountView, GeminiApp, LoginStart, LoginView},
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

#[tauri::command]
pub(crate) fn gemini_app_gateway_status() -> Result<Option<&'static str>, &'static str> {
    Ok(GeminiApp::shared()?.gateway_status())
}
#[tauri::command]
pub(crate) async fn gemini_app_accounts() -> Result<Vec<AccountView>, &'static str> {
    GeminiApp::shared()?.managed_accounts().await
}
#[tauri::command]
pub(crate) async fn gemini_app_login_start() -> Result<LoginStart, &'static str> {
    GeminiApp::shared()?.managed_start().await
}
#[tauri::command]
pub(crate) async fn gemini_app_login_status(id: Uuid) -> Result<LoginView, &'static str> {
    GeminiApp::shared()?.managed_login_status(id).await
}
#[tauri::command]
pub(crate) async fn gemini_app_login_cancel(id: Uuid) -> Result<(), &'static str> {
    GeminiApp::shared()?.managed_cancel(id).await
}
#[tauri::command]
pub(crate) async fn gemini_app_account_remove(id: Uuid) -> Result<(), &'static str> {
    GeminiApp::shared()?.managed_remove(id).await
}

#[tauri::command]
pub(crate) async fn gemini_app_confirm_terms(
    id: Uuid,
    confirmed: bool,
) -> Result<AccountView, &'static str> {
    GeminiApp::shared()?
        .managed_confirm_terms(id, confirmed)
        .await
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConfirmTermsRequest {
    confirmed: bool,
}

type WebError = (StatusCode, Json<Value>);
fn web_provider(state: AppState) -> Result<Arc<GeminiApp>, WebError> {
    state
        .gemini_app
        .ok_or_else(|| web_error("account_store_unavailable"))
}
fn web_error(code: &'static str) -> WebError {
    let status = match code {
        "oauth_profile_required"
        | "oauth_profile_invalid"
        | "account_store_unavailable"
        | "gemini_app_embedded_unavailable"
        | "gemini_app_embedded_in_use" => StatusCode::SERVICE_UNAVAILABLE,
        "oauth_login_pending"
        | "gemini_app_gateway_managed"
        | "gemini_app_terms_login_required" => StatusCode::CONFLICT,
        "oauth_flow_not_found" | "account_not_found" => StatusCode::NOT_FOUND,
        "gemini_app_terms_unsupported" => StatusCode::NOT_IMPLEMENTED,
        "gemini_app_terms_timeout" => StatusCode::GATEWAY_TIMEOUT,
        "gemini_app_terms_version_missing" | "gemini_app_embedded_request_failed" => {
            StatusCode::BAD_GATEWAY
        }
        _ => StatusCode::BAD_REQUEST,
    };
    (status, Json(json!({"error": code})))
}

pub(crate) async fn web_gateway_status(
    State(state): State<AppState>,
) -> Result<Json<Option<&'static str>>, WebError> {
    Ok(Json(web_provider(state)?.gateway_status()))
}
pub(crate) async fn web_list(
    State(state): State<AppState>,
) -> Result<Json<Vec<AccountView>>, WebError> {
    web_provider(state)?
        .managed_accounts()
        .await
        .map(Json)
        .map_err(web_error)
}
pub(crate) async fn web_start(State(state): State<AppState>) -> Result<Json<LoginStart>, WebError> {
    web_provider(state)?
        .managed_start()
        .await
        .map(Json)
        .map_err(web_error)
}
pub(crate) async fn web_status(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<LoginView>, WebError> {
    web_provider(state)?
        .managed_login_status(id)
        .await
        .map(Json)
        .map_err(web_error)
}
pub(crate) async fn web_cancel(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, WebError> {
    web_provider(state)?
        .managed_cancel(id)
        .await
        .map_err(web_error)?;
    Ok(StatusCode::NO_CONTENT)
}
pub(crate) async fn web_confirm_terms(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(request): Json<ConfirmTermsRequest>,
) -> Result<Json<AccountView>, WebError> {
    web_provider(state)?
        .managed_confirm_terms(id, request.confirmed)
        .await
        .map(Json)
        .map_err(web_error)
}

pub(crate) async fn web_remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, WebError> {
    web_provider(state)?
        .managed_remove(id)
        .await
        .map_err(web_error)?;
    Ok(StatusCode::NO_CONTENT)
}
