use super::{accounts::LoginStatus, gateway::Gateway, AccountView, LoginStart, LoginView};
use serde::{de::DeserializeOwned, Deserialize};
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
    sync::Mutex,
};
use uuid::Uuid;

const UNAVAILABLE: &str = "gemini_app_embedded_unavailable";
const BAD_RESPONSE: &str = "gemini_app_embedded_request_failed";
const NODE: &str = if cfg!(windows) { "node.exe" } else { "node" };
static DESKTOP_RUNTIME: OnceLock<PathBuf> = OnceLock::new();

pub(crate) fn set_desktop_runtime(resource_dir: PathBuf) {
    let _ = DESKTOP_RUNTIME.set(resource_dir.join("app2api-runtime"));
}

pub(super) struct Embedded {
    runtime: PathBuf,
    data: PathBuf,
    process: Mutex<Option<Process>>,
}

struct Process {
    child: Child,
    gateway: Arc<Gateway>,
    _lock: File,
}

#[derive(Deserialize)]
struct Ready {
    port: u16,
}

impl Embedded {
    pub(super) fn configured(data: &Path) -> Option<Self> {
        if std::env::var("ABV_GEMINI_APP_BACKEND").as_deref() == Ok("native") {
            return None;
        }
        let adjacent = std::env::current_exe()
            .ok()?
            .parent()?
            .join("app2api-runtime");
        let mut candidates = vec![adjacent];
        if let Some(packaged) = DESKTOP_RUNTIME.get() {
            candidates.insert(0, packaged.clone());
        }
        #[cfg(debug_assertions)]
        candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/app2api-runtime"));
        let runtime = candidates
            .into_iter()
            .find(|path| path.join(NODE).is_file() && path.join("embedded.mjs").is_file())?;
        Some(Self::new(runtime, data.to_owned()))
    }

    fn new(runtime: PathBuf, data: PathBuf) -> Self {
        Self {
            runtime,
            data,
            process: Mutex::new(None),
        }
    }

    pub(super) async fn gateway(&self) -> Result<Arc<Gateway>, &'static str> {
        let mut process = self.process.lock().await;
        if let Some(current) = process.as_mut() {
            if matches!(current.child.try_wait(), Ok(None)) {
                return Ok(current.gateway.clone());
            }
        }
        process.take();
        let next = self.spawn().await?;
        let gateway = next.gateway.clone();
        *process = Some(next);
        Ok(gateway)
    }

    async fn spawn(&self) -> Result<Process, &'static str> {
        // A separate, process-held lock works even before App2API reads its own state.
        let lock_path = self.data.with_extension("lock");
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).write(true).read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let lock = options.open(lock_path).map_err(|_| UNAVAILABLE)?;
        if !lock.metadata().is_ok_and(|metadata| metadata.is_file()) {
            return Err(UNAVAILABLE);
        }
        lock.try_lock().map_err(|_| "gemini_app_embedded_in_use")?;
        // A fresh private key is conveyed over the pipe, never through argv/env or the frontend.
        let key = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let mut command = Command::new(self.runtime.join(NODE));
        command
            .arg(self.runtime.join("embedded.mjs"))
            .arg(&self.data)
            .current_dir(&self.runtime)
            .env_remove("NODE_OPTIONS")
            .env_remove("NODE_PATH")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        let mut child = command.spawn().map_err(|_| UNAVAILABLE)?;
        let bootstrap = async {
            child
                .stdin
                .as_mut()
                .ok_or(UNAVAILABLE)?
                .write_all(format!("{key}\n").as_bytes())
                .await
                .map_err(|_| UNAVAILABLE)?;
            let stdout = child.stdout.take().ok_or(UNAVAILABLE)?;
            let mut lines = BufReader::new(stdout).lines();
            let line = lines
                .next_line()
                .await
                .map_err(|_| UNAVAILABLE)?
                .ok_or(UNAVAILABLE)?;
            let ready: Ready = serde_json::from_str(&line).map_err(|_| UNAVAILABLE)?;
            if ready.port == 0 {
                return Err(UNAVAILABLE);
            }
            Gateway::new(&format!("http://127.0.0.1:{}/v1", ready.port), &key)
        };
        let gateway = match tokio::time::timeout(Duration::from_secs(8), bootstrap).await {
            Ok(Ok(gateway)) => gateway,
            _ => {
                let _ = child.kill().await;
                return Err(UNAVAILABLE);
            }
        };
        Ok(Process {
            child,
            gateway: Arc::new(gateway),
            _lock: lock,
        })
    }

    pub(super) async fn stop(&self) {
        if let Some(mut process) = self.process.lock().await.take() {
            process.child.stdin.take();
            if tokio::time::timeout(Duration::from_secs(3), process.child.wait())
                .await
                .is_err()
            {
                let _ = process.child.kill().await;
            }
        }
    }

    async fn control<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
    ) -> Result<T, &'static str> {
        let gateway = self.gateway().await?;
        gateway.control(method, path).await
    }

    pub(super) async fn accounts(&self) -> Result<Vec<AccountView>, &'static str> {
        self.control(reqwest::Method::GET, "/__dgp/accounts").await
    }
    pub(super) async fn begin_login(&self) -> Result<LoginStart, &'static str> {
        self.control(reqwest::Method::POST, "/__dgp/login").await
    }
    pub(super) async fn login_status(&self, id: Uuid) -> Result<LoginView, &'static str> {
        #[derive(Deserialize)]
        struct Status {
            status: LoginStatus,
            error: Option<String>,
        }
        let status: Status = self
            .control(reqwest::Method::GET, &format!("/__dgp/login/{id}"))
            .await?;
        Ok(LoginView {
            status: status.status,
            error: status.error.as_deref().map(public_error),
        })
    }
    pub(super) async fn cancel_login(&self, id: Uuid) -> Result<(), &'static str> {
        self.control(reqwest::Method::DELETE, &format!("/__dgp/login/{id}"))
            .await
    }
    pub(super) async fn remove_account(&self, id: Uuid) -> Result<(), &'static str> {
        self.control(reqwest::Method::DELETE, &format!("/__dgp/accounts/{id}"))
            .await
    }
}

#[cfg(test)]
#[tokio::test]
#[ignore = "requires npm run app2api:prepare on the host architecture"]
async fn embedded_runtime_starts_and_owns_the_login_flow() {
    let work = tempfile::tempdir().unwrap();
    // The upstream store refuses symlink ancestors (macOS temporary paths may contain /var).
    let data = work.path().canonicalize().unwrap().join("state");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/app2api-runtime");
    let engine = Embedded::new(runtime.clone(), data.clone());
    assert!(engine.accounts().await.unwrap().is_empty());
    let second = Embedded::new(runtime, data);
    assert_eq!(
        second.gateway().await.err(),
        Some("gemini_app_embedded_in_use")
    );

    let gateway = engine.gateway().await.unwrap();
    assert!(gateway
        .control::<serde_json::Value>(reqwest::Method::GET, "/admin/")
        .await
        .is_err());
    let response = gateway
        .generate(
            super::Endpoint::Chat,
            &super::MODELS[0],
            &serde_json::json!({
                "model": "gemini-app-neon", "messages": [{"role":"user", "content":"synthetic"}],
            }),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    );

    let login = engine.begin_login().await.unwrap();
    let url = url::Url::parse(&login.authorization_url).unwrap();
    assert_eq!(url.host_str(), Some("accounts.google.com"));
    let redirect = url
        .query_pairs()
        .find(|(key, _)| key == "redirect_uri")
        .map(|(_, value)| value)
        .unwrap();
    let redirect = url::Url::parse(&redirect).unwrap();
    assert!(matches!(
        redirect.host_str(),
        Some("127.0.0.1" | "localhost")
    ));
    assert!(redirect.port().is_some());
    assert!(matches!(
        engine.login_status(login.flow_id).await.unwrap().status,
        LoginStatus::Pending
    ));
    engine.cancel_login(login.flow_id).await.unwrap();
    assert!(matches!(
        engine.login_status(login.flow_id).await.unwrap().status,
        LoginStatus::Cancelled
    ));
    engine.stop().await;
    assert!(second.accounts().await.unwrap().is_empty());
    second.stop().await;
}

#[cfg(test)]
#[tokio::test]
#[ignore = "requires npm run app2api:prepare on the host architecture"]
async fn embedded_terms_diagnostic_is_redacted_and_consent_is_explicit() {
    let work = tempfile::tempdir().unwrap();
    let data = work.path().canonicalize().unwrap().join("state");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/app2api-runtime");
    let engine = Embedded::new(runtime, data.clone());
    assert!(engine.accounts().await.unwrap().is_empty());
    engine.stop().await;
    let path = data.join("state.json");
    let mut state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    state["accounts"] = serde_json::json!([{
        "id": "acc_0123456789abcdef1234", "label": "account@example.test", "enabled": true,
        "status": "auth_error", "lastError": {
            "at": 123456, "status": 502, "code": "NATIVE_PRECONDITION_REQUIRED", "model": "gemini-neon",
            "nativeEvidence": {"upstreamHttpStatus": 200, "grpcStatus": 9,
                "upstreamMessage": "Please accept our terms of service. fixture@example.test Bearer synthetic-fixture"}
        }
    }]);
    let fixture = serde_json::to_vec(&state).unwrap();
    std::fs::write(&path, &fixture).unwrap();
    let accounts = engine.accounts().await.unwrap();
    let account = &accounts[0];
    assert_eq!(
        account.terms.as_ref().unwrap().status,
        super::accounts::TermsStatus::Unknown
    );
    let error = account.last_error.as_ref().unwrap();
    assert_eq!(error.code, "NATIVE_PRECONDITION_REQUIRED");
    let diagnostic = error.diagnostic.as_ref().unwrap();
    assert_eq!(diagnostic.grpc_status, Some(9));
    let message = diagnostic.message.as_deref().unwrap();
    assert!(message.contains("Please accept our terms of service."));
    assert!(!message.contains("fixture@example.test") && !message.contains("synthetic-fixture"));

    let id = Uuid::parse_str("00000000-0000-0123-4567-89abcdef1234").unwrap();
    let gateway = engine.gateway().await.unwrap();
    let rejected = gateway
        .control::<serde_json::Value>(
            reqwest::Method::POST,
            &format!("/__dgp/accounts/{id}/terms"),
        )
        .await;
    assert_eq!(
        rejected.err(),
        Some("gemini_app_terms_confirmation_required")
    );
    assert_eq!(
        gateway.confirm_terms(id).await.err(),
        Some("gemini_app_terms_login_required")
    );
    assert_eq!(std::fs::read(&path).unwrap(), fixture);
    engine.stop().await;
}

pub(super) fn public_error(code: &str) -> &'static str {
    match code {
        "account_limit" => "account_limit",
        "account_not_found" => "account_not_found",
        "account_store_unavailable" => "account_store_unavailable",
        "oauth_login_pending" => "oauth_login_pending",
        "oauth_flow_not_found" => "oauth_flow_not_found",
        "oauth_profile_required" => "oauth_profile_required",
        "oauth_listener_unavailable" => "oauth_listener_unavailable",
        "oauth_refresh_token_missing" => "oauth_refresh_token_missing",
        "oauth_denied" => "oauth_denied",
        "gemini_app_terms_confirmation_required" => "gemini_app_terms_confirmation_required",
        "gemini_app_terms_login_required" => "gemini_app_terms_login_required",
        "gemini_app_terms_timeout" => "gemini_app_terms_timeout",
        "gemini_app_terms_version_missing" => "gemini_app_terms_version_missing",
        _ => BAD_RESPONSE,
    }
}
