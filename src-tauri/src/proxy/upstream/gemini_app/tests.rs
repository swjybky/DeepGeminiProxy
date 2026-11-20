use super::*;
use axum::body::to_bytes;

#[tokio::test]
async fn app_models_never_fall_back_to_antigravity() {
    let unrestricted = tokio::sync::RwLock::new(None);
    let allowed = tokio::sync::RwLock::new(Some(vec!["gemini-3.8-flash-high".to_owned()]));
    let ordinary = json!({"model":"gemini-3.8-flash-high"});
    let app = json!({"model":"gemini-app-neon"});
    assert!(
        dispatch(None, &unrestricted, Some(Endpoint::Chat), &ordinary)
            .await
            .is_none()
    );
    assert_eq!(
        dispatch(None, &allowed, Some(Endpoint::Chat), &app)
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        dispatch(None, &unrestricted, None, &app)
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    let response = dispatch(None, &unrestricted, Some(Endpoint::Chat), &app)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()["x-upstream-provider"], "gemini-app");
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["error"]["code"],
        "gemini_app_no_accounts"
    );
    assert_eq!(find_model("models/gemini-app-neon").unwrap().mode, 3);
}

#[test]
fn app2api_gateway_requires_a_loopback_api_and_a_client_key() {
    for base in [
        "https://example.test/v1",
        "http://localhost:18989/v1",
        "http://127.0.0.1:18989/admin/",
        "http://fixture@127.0.0.1:18989/v1",
        "http://127.0.0.1:18989/v1?key=fixture",
    ] {
        assert!(
            gateway::Gateway::new(base, "fixture-key").is_err(),
            "{base}"
        );
    }
    assert!(gateway::Gateway::new("http://127.0.0.1:18989/v1", "").is_err());
    assert!(gateway::Gateway::new("http://[::1]:18989/v1/", "fixture-key").is_ok());
}

#[tokio::test]
async fn app2api_gateway_routes_both_endpoints_and_preserves_incremental_delivery() {
    use axum::body::Bytes;
    use axum::{http::HeaderMap, routing::post, Router};
    use futures::StreamExt;
    use std::convert::Infallible;
    let finish = Arc::new(tokio::sync::Notify::new());
    let finish_stream = finish.clone();
    let router = Router::new()
        .route("/v1/responses", post(|headers: HeaderMap, Json(body): Json<Value>| async move {
            assert_eq!(headers["authorization"], "Bearer fixture-app2api-key");
            assert_eq!(body, json!({"model":"gemini-hydrogen","input":"hello","store":false}));
            Json(json!({"object":"response","model":"gemini-hydrogen","status":"completed"}))
        }))
        .route("/v1/chat/completions", post(move |headers: HeaderMap, Json(body): Json<Value>| async move {
            assert_eq!(headers["authorization"], "Bearer fixture-app2api-key");
            assert_eq!(body["model"], "gemini-neon");
            assert_eq!(body["stream"], true);
            let stream = async_stream::stream! {
                yield Ok::<_, Infallible>(Bytes::from_static(b"data: {\"choices\":[]}\n\n"));
                finish_stream.notified().await;
                yield Ok(Bytes::from_static(b"data: [DONE]\n\n"));
            };
            ([("content-type", "text/event-stream")], Body::from_stream(stream))
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let directory = tempfile::tempdir().unwrap();
    // A bad native account store must not block or be modified in gateway mode.
    let saved = directory.path().join("gemini-app-accounts.json");
    std::fs::write(&saved, b"fixture-old-store").unwrap();
    let gateway =
        gateway::Gateway::new(&format!("http://{address}/v1"), "fixture-app2api-key").unwrap();
    let provider = GeminiApp::new(saved.clone(), Ok(Some(gateway))).unwrap();
    assert!(provider.available());
    assert_eq!(provider.gateway_status(), Some("configured"));
    assert!(provider.accounts().is_empty());
    assert_eq!(
        provider.remove_account(uuid::Uuid::new_v4()),
        Err("gemini_app_gateway_managed")
    );
    let unrestricted = tokio::sync::RwLock::new(None);

    let response = dispatch(
        Some(&provider),
        &unrestricted,
        Some(Endpoint::Responses),
        &json!({"model":"gemini-app-hydrogen","input":"hello","store":false}),
    )
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-upstream-provider"], "gemini-app");
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["status"],
        "completed"
    );

    let response = dispatch(Some(&provider), &unrestricted, Some(Endpoint::Chat),
        &json!({"model":"gemini-app-neon","messages":[{"role":"user","content":"hello"}],"stream":true}))
        .await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    let mut stream = response.into_body().into_data_stream();
    let frame = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(&frame[..], b"data: {\"choices\":[]}\n\n");
    finish.notify_one();
    let tail = Body::from_stream(stream);
    assert_eq!(
        &to_bytes(tail, 4096).await.unwrap()[..],
        b"data: [DONE]\n\n"
    );
    assert_eq!(std::fs::read(saved).unwrap(), b"fixture-old-store");
    server.abort();
}

#[tokio::test]
async fn app2api_gateway_reports_key_rejection_without_claiming_local_auth_failed() {
    use axum::{routing::post, Router};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error":"fixture-private-error"})),
            )
        }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let gateway =
        gateway::Gateway::new(&format!("http://{address}"), "fixture-invalid-key").unwrap();
    let err = gateway
        .generate(
            Endpoint::Chat,
            &MODELS[0],
            &json!({"model":"gemini-app-neon"}),
        )
        .await
        .unwrap_err();
    assert_eq!(err.status, StatusCode::BAD_GATEWAY);
    assert_eq!(err.code, "gemini_app_gateway_auth_failed");
    assert!(!err.detail().to_string().contains("fixture-private-error"));
    server.abort();
}

#[test]
fn normalization_preserves_full_order_and_rejects_unsupported_features() {
    let messages = json!({"model":"gemini-app-neon","messages":[{"role":"system","content":"policy"},{"role":"user","content":"hello"},{"role":"assistant","content":"hi"},{"role":"user","content":[{"type":"text","text":"again"}]}],"stream":true});
    let (prompt, thinking, stream) = normalize(Endpoint::Chat, &messages).unwrap();
    let history: Value = serde_json::from_str(prompt.split_once('\n').unwrap().1).unwrap();
    assert_eq!(history[0]["role"], "system");
    assert_eq!(history[1]["text"], "hello");
    assert_eq!(history[2]["role"], "assistant");
    assert_eq!(history[3]["text"], "again");
    assert_eq!(thinking, 2);
    assert!(stream);
    let (_, thinking, _) = normalize(
        Endpoint::Responses,
        &json!({"input":"hi","store":false,"reasoning":{"effort":"low"}}),
    )
    .unwrap();
    assert_eq!(thinking, 1);
    for request in [
        json!({"input":"hi","previous_response_id":"resp_example"}),
        json!({"input":"hi","tools":[]}),
        json!({"input":"hi","thinking_level":3}),
        json!({"input":[{"role":"user","content":[{"type":"input_image","image_url":"https://example.test/image.png"}]}]}),
    ] {
        assert!(normalize(Endpoint::Responses, &request).is_err());
    }
}

#[test]
fn terminal_events_do_not_claim_completion_after_an_error() {
    let mut responses = Delivery::new(MODELS[0].public, Endpoint::Responses);
    assert!(responses.start().contains("response.created"));
    assert!(responses
        .delta("first")
        .contains("response.output_text.delta"));
    let terminal = responses.finish("first", Some(AppError::timeout()));
    assert!(terminal.contains("event: response.incomplete\n"));
    assert!(terminal.contains("gemini_app_timeout"));
    assert!(!terminal.contains("response.completed"));
    let mut chat = Delivery::new(MODELS[0].public, Endpoint::Chat);
    let terminal = chat.finish("first", Some(AppError::timeout()));
    assert!(terminal.contains("\"incomplete\":true"));
    assert!(!terminal.contains("\"finish_reason\":\"stop\""));
}
