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
        _ => BAD_RESPONSE,
    }
}
