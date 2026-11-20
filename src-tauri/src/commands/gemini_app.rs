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
pub(crate) fn gemini_app_accounts() -> Result<Vec<AccountView>, &'static str> {
    Ok(GeminiApp::shared()?.accounts())
}
#[tauri::command]
pub(crate) async fn gemini_app_login_start() -> Result<LoginStart, &'static str> {
    GeminiApp::shared()?.begin_login().await
}
#[tauri::command]
pub(crate) fn gemini_app_login_status(id: Uuid) -> Result<LoginView, &'static str> {
    GeminiApp::shared()?.login_status(id)
}
#[tauri::command]
pub(crate) fn gemini_app_login_cancel(id: Uuid) -> Result<(), &'static str> {
    GeminiApp::shared()?.cancel_login(id);
    Ok(())
}
#[tauri::command]
pub(crate) fn gemini_app_account_remove(id: Uuid) -> Result<(), &'static str> {
    GeminiApp::shared()?.remove_account(id)
}

type WebError = (StatusCode, Json<Value>);
fn web_provider(state: AppState) -> Result<Arc<GeminiApp>, WebError> {
    state
        .gemini_app
        .ok_or_else(|| web_error("account_store_unavailable"))
}
fn web_error(code: &'static str) -> WebError {
    let status = match code {
        "oauth_profile_required" | "oauth_profile_invalid" | "account_store_unavailable" => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        "oauth_login_pending" | "gemini_app_gateway_managed" => StatusCode::CONFLICT,
        "oauth_flow_not_found" => StatusCode::NOT_FOUND,
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
    Ok(Json(web_provider(state)?.accounts()))
}
pub(crate) async fn web_start(State(state): State<AppState>) -> Result<Json<LoginStart>, WebError> {
    web_provider(state)?
        .begin_login()
        .await
        .map(Json)
        .map_err(web_error)
}
pub(crate) async fn web_status(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<LoginView>, WebError> {
    web_provider(state)?
        .login_status(id)
        .map(Json)
        .map_err(web_error)
}
pub(crate) async fn web_cancel(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, WebError> {
    web_provider(state)?.cancel_login(id);
    Ok(StatusCode::NO_CONTENT)
}
pub(crate) async fn web_remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, WebError> {
    web_provider(state)?.remove_account(id).map_err(web_error)?;
    Ok(StatusCode::NO_CONTENT)
}
