use super::{AppError, GeminiApp};
use axum::{
    extract::{Query, State},
    response::IntoResponse,
    routing::get,
    Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};
use tokio::sync::{oneshot, Semaphore};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const MAX_ACCOUNTS: usize = 64;
const AUTH_TIMEOUT: Duration = Duration::from_secs(300);
const GOOGLE_TOKEN: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_USERINFO: &str = "https://openidconnect.googleapis.com/v1/userinfo";

#[derive(Clone)]
struct Profile {
    id: String,
    secret: Option<String>,
    scope: String,
}

#[derive(Default)]
struct ProfileSettings<'a> {
    id: Option<&'a str>,
    scope: Option<&'a str>,
    secret: Option<&'a str>,
}

impl Profile {
    fn configured() -> Result<Self, &'static str> {
        let id = std::env::var("ABV_GEMINI_APP_OAUTH_CLIENT_ID").ok();
        let scope = std::env::var("ABV_GEMINI_APP_OAUTH_SCOPE").ok();
        let secret = std::env::var("ABV_GEMINI_APP_OAUTH_CLIENT_SECRET").ok();
        Self::resolve(
            ProfileSettings {
                id: id.as_deref(),
                scope: scope.as_deref(),
                secret: secret.as_deref(),
            },
            ProfileSettings {
                id: option_env!("ABV_GEMINI_APP_DEFAULT_OAUTH_CLIENT_ID"),
                scope: option_env!("ABV_GEMINI_APP_DEFAULT_OAUTH_SCOPE"),
                // Only a public desktop client parameter may be built into a distributable binary.
                secret: option_env!("ABV_GEMINI_APP_DEFAULT_OAUTH_PUBLIC_CLIENT_SECRET"),
            },
        )
    }

    fn resolve(
        runtime: ProfileSettings<'_>,
        bundled: ProfileSettings<'_>,
    ) -> Result<Self, &'static str> {
        // An override replaces the whole profile; never pair one client's ID with another's scopes or secret.
        match runtime.parse() {
            Err("oauth_profile_required") => bundled.parse(),
            result => result,
        }
    }
}

impl ProfileSettings<'_> {
    fn parse(self) -> Result<Profile, &'static str> {
        let id = self.id.filter(|s| !s.trim().is_empty());
        let scope = self.scope.map(str::trim).filter(|s| !s.is_empty());
        let secret = self.secret.filter(|s| !s.trim().is_empty());
        if id.is_none() && scope.is_none() && secret.is_none() {
            return Err("oauth_profile_required");
        }
        let (id, scope) = id.zip(scope).ok_or("oauth_profile_invalid")?;
        if !id.ends_with(".apps.googleusercontent.com")
            || id.len() > 512
            || id.contains(char::is_whitespace)
            || scope.len() > 2048
            || secret.is_some_and(|value| !valid_token(value))
        {
            return Err("oauth_profile_invalid");
        }
        Ok(Profile {
            id: id.to_owned(),
            secret: secret.map(str::to_owned),
            scope: scope.to_owned(),
        })
    }
}

#[derive(Clone, Deserialize, Serialize)]
struct SavedAccount {
    id: Uuid,
    client_id: String,
    subject: String,
    email: String,
    refresh_token: String,
    #[serde(default)]
    needs_login: bool,
}
struct Account {
    saved: SavedAccount,
    slot: Arc<Semaphore>,
    access: Arc<tokio::sync::Mutex<Option<Access>>>,
}
struct Access {
    token: String,
    expiry: Instant,
}

#[derive(Serialize)]
pub(crate) struct AccountView {
    id: Uuid,
    email: String,
    needs_login: bool,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LoginStatus {
    Pending,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Serialize)]
pub(crate) struct LoginView {
    status: LoginStatus,
    error: Option<&'static str>,
}
#[derive(Serialize)]
pub(crate) struct LoginStart {
    flow_id: Uuid,
    authorization_url: String,
}
struct Flow {
    id: Uuid,
    cancel: CancellationToken,
    view: LoginView,
}

#[derive(Default)]
struct StateData {
    accounts: Vec<Account>,
    flow: Option<Flow>,
    cursor: usize,
}
pub(super) struct Pool {
    path: PathBuf,
    profile: Result<Profile, &'static str>,
    state: Mutex<StateData>,
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct Tokens {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
}
#[derive(Deserialize)]
struct Identity {
    sub: String,
    email: String,
    email_verified: bool,
}

impl Pool {
    pub(super) fn new(path: PathBuf) -> Result<Self, &'static str> {
        let data = match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_file() && meta.len() <= 512 * 1024 => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::{MetadataExt, PermissionsExt};
                    if meta.permissions().mode() & 0o077 != 0 || meta.nlink() != 1 {
                        return Err("account_store_unsafe");
                    }
                }
                let bytes = std::fs::read(&path).map_err(|_| "account_store_unavailable")?;
                serde_json::from_slice::<Vec<SavedAccount>>(&bytes)
                    .map_err(|_| "account_store_invalid")?
            }
            Ok(_) => return Err("account_store_unsafe"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(_) => return Err("account_store_unavailable"),
        };
        if data.len() > MAX_ACCOUNTS {
            return Err("account_store_invalid");
        }
        let accounts = data
            .into_iter()
            .map(|saved| Account::new(saved, None))
            .collect();
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|_| "oauth_client_unavailable")?;
        Ok(Self {
            path,
            profile: Profile::configured(),
            state: Mutex::new(StateData {
                accounts,
                ..StateData::default()
            }),
            client,
        })
    }

    fn state(&self) -> MutexGuard<'_, StateData> {
        self.state.lock().unwrap_or_else(|err| err.into_inner())
    }

    fn save<'a>(
        &self,
        records: impl IntoIterator<Item = &'a SavedAccount>,
    ) -> Result<(), &'static str> {
        let bytes = serde_json::to_vec(&records.into_iter().collect::<Vec<_>>())
            .map_err(|_| "account_store_invalid")?;
        crate::utils::fs::write_private_atomic(&self.path, &bytes)
            .map_err(|_| "account_store_unavailable")
    }

    pub(super) fn available(&self) -> bool {
        let Ok(profile) = &self.profile else {
            return false;
        };
        self.state()
            .accounts
            .iter()
            .any(|account| !account.saved.needs_login && account.saved.client_id == profile.id)
    }

    fn accounts(&self) -> Vec<AccountView> {
        let profile = self.profile.as_ref().ok();
        self.state()
            .accounts
            .iter()
            .map(|account| AccountView {
                id: account.saved.id,
                email: account.saved.email.clone(),
                needs_login: account.saved.needs_login
                    || profile.is_none_or(|p| p.id != account.saved.client_id),
            })
            .collect()
    }

    fn remove(&self, id: Uuid) -> Result<(), &'static str> {
        let mut state = self.state();
        self.save(
            state
                .accounts
                .iter()
                .filter(|a| a.saved.id != id)
                .map(|a| &a.saved),
        )?;
        state.accounts.retain(|a| a.saved.id != id);
        Ok(())
    }

    async fn token(
        &self,
        profile: &Profile,
        mut form: Vec<(&'static str, &str)>,
    ) -> Result<Tokens, &'static str> {
        form.push(("client_id", &profile.id));
        if let Some(secret) = profile.secret.as_deref() {
            form.push(("client_secret", secret));
        }
        let response = self
            .client
            .post(GOOGLE_TOKEN)
            .form(&form)
            .send()
            .await
            .map_err(|_| "oauth_network_error")?;
        if response.status() == reqwest::StatusCode::BAD_REQUEST {
            let error: HashMap<String, serde_json::Value> =
                response.json().await.map_err(|_| "oauth_token_rejected")?;
            if error.get("error").and_then(serde_json::Value::as_str) == Some("invalid_grant") {
                return Err("oauth_invalid_grant");
            }
            return Err("oauth_token_rejected");
        }
        let tokens = response
            .error_for_status()
            .map_err(|_| "oauth_token_rejected")?
            .json::<Tokens>()
            .await
            .map_err(|_| "oauth_token_rejected")?;
        if !valid_token(&tokens.access_token)
            || tokens
                .refresh_token
                .as_deref()
                .is_some_and(|token| !valid_token(token))
        {
            return Err("oauth_token_invalid");
        }
        Ok(tokens)
    }

    async fn authorize(
        &self,
        profile: &Profile,
        redirect: &str,
        verifier: &str,
        code: &str,
    ) -> Result<(SavedAccount, Access), &'static str> {
        let tokens = self
            .token(
                profile,
                vec![
                    ("grant_type", "authorization_code"),
                    ("redirect_uri", redirect),
                    ("code_verifier", verifier),
                    ("code", code),
                ],
            )
            .await?;
        let identity = self
            .client
            .get(GOOGLE_USERINFO)
            .bearer_auth(&tokens.access_token)
            .send()
            .await
            .map_err(|_| "oauth_network_error")?
            .error_for_status()
            .map_err(|_| "oauth_identity_invalid")?
            .json::<Identity>()
            .await
            .map_err(|_| "oauth_identity_invalid")?;
        if !identity.email_verified
            || identity.email.is_empty()
            || identity.email.len() > 320
            || identity.sub.is_empty()
            || identity.sub.len() > 256
        {
            return Err("oauth_identity_invalid");
        }
        let account = SavedAccount {
            id: Uuid::new_v4(),
            client_id: profile.id.clone(),
            subject: identity.sub,
            email: identity.email,
            refresh_token: tokens.refresh_token.ok_or("oauth_refresh_token_missing")?,
            needs_login: false,
        };
        Ok((account, Access::new(tokens.access_token, tokens.expires_in)))
    }

    fn finish_login(&self, id: Uuid, result: Result<(SavedAccount, Access), &'static str>) {
        let mut state = self.state();
        if !state
            .flow
            .as_ref()
            .is_some_and(|flow| flow.id == id && matches!(flow.view.status, LoginStatus::Pending))
        {
            return;
        }
        let result = result.and_then(|(mut record, access)| {
            let existing = state.accounts.iter().position(|a| {
                a.saved.subject == record.subject && a.saved.client_id == record.client_id
            });
            if let Some(index) = existing {
                record.id = state.accounts[index].saved.id;
            } else if state.accounts.len() >= MAX_ACCOUNTS {
                return Err("account_limit");
            }
            self.save(
                state
                    .accounts
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| Some(*index) != existing)
                    .map(|(_, a)| &a.saved)
                    .chain(std::iter::once(&record)),
            )?;
            let mut account = Account::new(record, Some(access));
            if let Some(index) = existing {
                account.slot = state.accounts[index].slot.clone();
                state.accounts[index] = account;
            } else {
                state.accounts.push(account);
            }
            Ok(())
        });
        state.flow.as_mut().unwrap().view = match result {
            Ok(()) => LoginView {
                status: LoginStatus::Completed,
                error: None,
            },
            Err(err) => LoginView {
                status: LoginStatus::Failed,
                error: Some(err),
            },
        };
    }

    pub(super) async fn acquire(
        &self,
    ) -> Result<(String, tokio::sync::OwnedSemaphorePermit), AppError> {
        let profile = self.profile.as_ref().map_err(|_| AppError::no_accounts())?;
        let (record, access, permit) = {
            let mut state = self.state();
            let size = state.accounts.len();
            let mut has_account = false;
            let mut selected = None;
            for offset in 0..size {
                let index = (state.cursor + offset) % size;
                let account = &state.accounts[index];
                if account.saved.needs_login || account.saved.client_id != profile.id {
                    continue;
                }
                has_account = true;
                if let Ok(permit) = account.slot.clone().try_acquire_owned() {
                    selected = Some((account.saved.clone(), account.access.clone(), permit));
                    state.cursor = (index + 1) % size;
                    break;
                }
            }
            selected.ok_or_else(|| {
                if has_account {
                    AppError::busy()
                } else {
                    AppError::no_accounts()
                }
            })?
        };
        let mut current = access.lock().await;
        if let Some(token) = current
            .as_ref()
            .filter(|token| token.expiry > Instant::now())
        {
            return Ok((token.token.clone(), permit));
        }
        let tokens = match self
            .token(
                profile,
                vec![
                    ("grant_type", "refresh_token"),
                    ("refresh_token", &record.refresh_token),
                ],
            )
            .await
        {
            Ok(tokens) => tokens,
            Err("oauth_invalid_grant") => {
                self.update_grant(&record, &access, None, true)?;
                return Err(AppError::no_accounts());
            }
            Err(_) => {
                return Err(AppError::upstream(
                    "gemini_app_refresh_failed",
                    "Could not refresh Gemini App authorization.",
                ))
            }
        };
        if let Some(refresh) = tokens.refresh_token {
            self.update_grant(&record, &access, Some(refresh), false)?;
        }
        let token = tokens.access_token.clone();
        *current = Some(Access::new(tokens.access_token, tokens.expires_in));
        Ok((token, permit))
    }

    fn update_grant(
        &self,
        record: &SavedAccount,
        access: &Arc<tokio::sync::Mutex<Option<Access>>>,
        refresh: Option<String>,
        needs_login: bool,
    ) -> Result<(), AppError> {
        let mut state = self.state();
        let Some(index) = state
            .accounts
            .iter()
            .position(|a| a.saved.id == record.id && Arc::ptr_eq(&a.access, access))
        else {
            return Ok(());
        };
        let mut updated = state.accounts[index].saved.clone();
        if let Some(refresh) = refresh {
            updated.refresh_token = refresh;
        }
        updated.needs_login = needs_login;
        if self
            .save(state.accounts.iter().enumerate().map(|(i, a)| {
                if i == index {
                    &updated
                } else {
                    &a.saved
                }
            }))
            .is_err()
        {
            if needs_login {
                state.accounts[index].saved.needs_login = true;
            }
            return Err(AppError::upstream(
                "gemini_app_store_failed",
                "Could not update Gemini App account storage.",
            ));
        }
        state.accounts[index].saved = updated;
        Ok(())
    }
}

impl Access {
    fn new(token: String, seconds: u64) -> Self {
        Self {
            token,
            expiry: Instant::now()
                + Duration::from_secs(seconds.clamp(1, 86_400).saturating_sub(45)),
        }
    }
}
impl Account {
    fn new(saved: SavedAccount, access: Option<Access>) -> Self {
        Self {
            saved,
            access: Arc::new(tokio::sync::Mutex::new(access)),
            slot: Arc::new(Semaphore::new(2)),
        }
    }
}
fn valid_token(token: &str) -> bool {
    !token.is_empty() && token.len() <= 4096 && token.bytes().all(|b| b.is_ascii_graphic())
}
fn nonce() -> String {
    let mut bytes = [0; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

#[derive(Deserialize)]
struct Callback {
    state: String,
    code: Option<String>,
    error: Option<String>,
}
type CallbackExchange = Arc<(
    String,
    Mutex<Option<oneshot::Sender<Result<String, &'static str>>>>,
)>;
async fn callback(
    State(exchange): State<CallbackExchange>,
    Query(query): Query<Callback>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    let reply = |status, text| {
        (
            status,
            [
                ("cache-control", "no-store"),
                ("referrer-policy", "no-referrer"),
            ],
            text,
        )
    };
    if query.state != exchange.0 {
        return reply(StatusCode::BAD_REQUEST, "Invalid OAuth state.");
    }
    let result = match (query.code, query.error) {
        (Some(code), None) if valid_token(&code) => Ok(code),
        (None, Some(_)) => Err("oauth_denied"),
        _ => return reply(StatusCode::BAD_REQUEST, "Invalid OAuth callback."),
    };
    if let Some(tx) = exchange.1.lock().ok().and_then(|mut slot| slot.take()) {
        let _ = tx.send(result);
        reply(
            StatusCode::OK,
            "Return to DeepGeminiProxy to see the result. You may close this window.",
        )
    } else {
        reply(StatusCode::GONE, "OAuth request expired.")
    }
}

impl GeminiApp {
    pub(crate) fn accounts(&self) -> Vec<AccountView> {
        self.native_pool().map(Pool::accounts).unwrap_or_default()
    }
    pub(crate) fn remove_account(&self, id: Uuid) -> Result<(), &'static str> {
        self.native_pool()?.remove(id)
    }
    pub(crate) fn login_status(&self, id: Uuid) -> Result<LoginView, &'static str> {
        self.native_pool()?
            .state()
            .flow
            .as_ref()
            .filter(|f| f.id == id)
            .map(|f| f.view.clone())
            .ok_or("oauth_flow_not_found")
    }
    pub(crate) fn cancel_login(&self, id: Uuid) {
        let Ok(pool) = self.native_pool() else { return };
        if let Some(flow) = pool
            .state()
            .flow
            .as_mut()
            .filter(|f| f.id == id && matches!(f.view.status, LoginStatus::Pending))
        {
            flow.view.status = LoginStatus::Cancelled;
            flow.cancel.cancel();
        }
    }
    pub(crate) async fn begin_login(self: &Arc<Self>) -> Result<LoginStart, &'static str> {
        let pool = self.native_pool()?;
        let profile = pool.profile.clone()?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|_| "oauth_listener_unavailable")?;
        let port = listener
            .local_addr()
            .map_err(|_| "oauth_listener_unavailable")?
            .port();
        let redirect = format!("http://127.0.0.1:{port}/oauth/callback");
        let (tx, rx) = oneshot::channel();
        let state = nonce();
        let verifier = nonce();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let url = authorization_url(&profile, &redirect, &state, &challenge);
        let id = Uuid::new_v4();
        let cancel = CancellationToken::new();
        {
            let mut pool = pool.state();
            if pool
                .flow
                .as_ref()
                .is_some_and(|f| matches!(f.view.status, LoginStatus::Pending))
            {
                return Err("oauth_login_pending");
            }
            pool.flow = Some(Flow {
                id,
                cancel: cancel.clone(),
                view: LoginView {
                    status: LoginStatus::Pending,
                    error: None,
                },
            });
        }
        let provider = self.clone();
        tokio::spawn(async move {
            let Ok(pool) = provider.native_pool() else {
                return;
            };
            let router = Router::new()
                .route("/oauth/callback", get(callback))
                .with_state(Arc::new((state, Mutex::new(Some(tx)))));
            let stop = CancellationToken::new();
            let http_stop = stop.clone();
            let server = tokio::spawn(async move {
                let _ = axum::serve(listener, router)
                    .with_graceful_shutdown(http_stop.cancelled_owned())
                    .await;
            });
            let result = tokio::select! {
                _ = cancel.cancelled() => Err("oauth_cancelled"),
                result = tokio::time::timeout(AUTH_TIMEOUT, async {
                    let code = rx.await.map_err(|_| "oauth_listener_unavailable")??;
                    stop.cancel();
                    pool.authorize(&profile, &redirect, &verifier, &code).await
                }) => result.unwrap_or(Err("oauth_timeout")),
            };
            pool.finish_login(id, result);
            stop.cancel();
            let mut server = server;
            if tokio::time::timeout(Duration::from_secs(2), &mut server)
                .await
                .is_err()
            {
                server.abort();
            }
        });
        Ok(LoginStart {
            flow_id: id,
            authorization_url: url.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundled_fixture() -> ProfileSettings<'static> {
        ProfileSettings {
            id: Some("bundled-fixture.apps.googleusercontent.com"),
            scope: Some("fixture.bundled"),
            secret: Some("synthetic-bundled"),
        }
    }

    #[test]
    fn runtime_profile_replaces_the_complete_bundled_profile() {
        let profile = Profile::resolve(
            ProfileSettings {
                id: Some("override-fixture.apps.googleusercontent.com"),
                scope: Some("fixture.override"),
                secret: None,
            },
            bundled_fixture(),
        )
        .unwrap();
        assert_eq!(profile.id, "override-fixture.apps.googleusercontent.com");
        assert_eq!(profile.scope, "fixture.override");
        assert!(profile.secret.is_none());
    }

    #[test]
    fn missing_or_invalid_profiles_are_distinct_and_do_not_mix_sources() {
        assert_eq!(
            Profile::resolve(ProfileSettings::default(), ProfileSettings::default()).err(),
            Some("oauth_profile_required")
        );
        for partial in [
            ProfileSettings {
                id: Some("override-fixture.apps.googleusercontent.com"),
                ..Default::default()
            },
            ProfileSettings {
                secret: Some("synthetic-override"),
                ..Default::default()
            },
            ProfileSettings {
                id: Some("not-a-google-client-id"),
                scope: Some("fixture.override"),
                secret: None,
            },
        ] {
            assert_eq!(
                Profile::resolve(partial, bundled_fixture()).err(),
                Some("oauth_profile_invalid")
            );
        }
        assert_eq!(
            Profile::resolve(
                ProfileSettings::default(),
                ProfileSettings {
                    scope: Some("fixture.bundled"),
                    ..Default::default()
                },
            )
            .err(),
            Some("oauth_profile_invalid")
        );
    }

    #[tokio::test]
    async fn bundled_profile_starts_real_loopback_login_without_runtime_overrides() {
        let directory = tempfile::tempdir().unwrap();
        let account_path = directory.path().join("gemini-app-accounts.json");
        let mut provider = GeminiApp::new(account_path.clone(), Ok(None)).unwrap();
        provider.pool.as_mut().unwrap().profile =
            Profile::resolve(ProfileSettings::default(), bundled_fixture());
        let provider = Arc::new(provider);
        let flow = provider.begin_login().await.unwrap();
        let url = url::Url::parse(&flow.authorization_url).unwrap();
        let fields = url.query_pairs().collect::<HashMap<_, _>>();
        assert_eq!(url.host_str(), Some("accounts.google.com"));
        assert_eq!(
            fields["client_id"],
            "bundled-fixture.apps.googleusercontent.com"
        );
        assert_eq!(fields["scope"], "openid email fixture.bundled");
        assert_eq!(fields["code_challenge_method"], "S256");
        assert!(!url.as_str().contains("synthetic-bundled"));

        let mut redirect = url::Url::parse(&fields["redirect_uri"]).unwrap();
        assert_eq!(redirect.host_str(), Some("127.0.0.1"));
        redirect
            .query_pairs_mut()
            .append_pair("state", &fields["state"])
            .append_pair("error", "access_denied");
        // Only send a synthetic denial to the local listener, so no code is exchanged with Google.
        let response = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(redirect)
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let failed = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let status = provider.login_status(flow.flow_id).unwrap();
                if !matches!(status.status, LoginStatus::Pending) {
                    break status;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(matches!(failed.status, LoginStatus::Failed));
        assert_eq!(failed.error, Some("oauth_denied"));
        assert!(provider.accounts().is_empty());
        assert!(!account_path.exists());
    }

    #[tokio::test]
    async fn loopback_rejects_wrong_state_without_consuming_the_callback() {
        let (tx, mut rx) = oneshot::channel();
        let exchange: CallbackExchange = Arc::new(("fixture-state".into(), Mutex::new(Some(tx))));
        let wrong = Callback {
            state: "wrong".into(),
            code: Some("fake-code".into()),
            error: None,
        };
        assert_eq!(
            callback(State(exchange.clone()), Query(wrong))
                .await
                .into_response()
                .status(),
            400
        );
        assert!(matches!(
            rx.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        let right = Callback {
            state: "fixture-state".into(),
            code: Some("fake-code".into()),
            error: None,
        };
        assert_eq!(
            callback(State(exchange), Query(right))
                .await
                .into_response()
                .status(),
            200
        );
        assert_eq!(rx.await.unwrap(), Ok("fake-code".to_owned()));
    }

    #[test]
    fn login_persists_separately_and_reconnect_preserves_account_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("gemini-app-accounts.json");
        let pool = Pool::new(path.clone()).unwrap();
        let login = |refresh: &str| {
            let id = Uuid::new_v4();
            pool.state().flow = Some(Flow {
                id,
                cancel: CancellationToken::new(),
                view: LoginView {
                    status: LoginStatus::Pending,
                    error: None,
                },
            });
            let saved = SavedAccount {
                id: Uuid::new_v4(),
                client_id: "fixture.apps.googleusercontent.com".into(),
                subject: "fixture-subject".into(),
                email: "fixture@example.test".into(),
                refresh_token: refresh.into(),
                needs_login: false,
            };
            pool.finish_login(id, Ok((saved, Access::new("synthetic-token".into(), 60))));
            assert!(matches!(
                pool.state().flow.as_ref().unwrap().view.status,
                LoginStatus::Completed
            ));
        };
        login("synthetic-first");
        let stable_id = pool.accounts()[0].id;
        login("synthetic-second");
        assert_eq!(pool.accounts().len(), 1);
        assert_eq!(pool.accounts()[0].id, stable_id);
        let loaded = Pool::new(path).unwrap();
        assert_eq!(
            loaded.state().accounts[0].saved.refresh_token,
            "synthetic-second"
        );
        assert!(loaded.state().accounts[0]
            .access
            .try_lock()
            .unwrap()
            .is_none());
        assert!(!directory.path().join("accounts").exists());
        loaded.remove(stable_id).unwrap();
        assert!(loaded.accounts().is_empty());
    }

    #[test]
    fn authorization_url_uses_s256_and_never_contains_the_secret() {
        let profile = Profile {
            id: "fixture.apps.googleusercontent.com".into(),
            secret: Some("synthetic-not-public".into()),
            scope: "fixture.scope".into(),
        };
        let url = authorization_url(
            &profile,
            "http://127.0.0.1:12345/oauth/callback",
            "fixture-state",
            "fixture-challenge",
        );
        let fields = url.query_pairs().collect::<HashMap<_, _>>();
        assert_eq!(fields["code_challenge_method"], "S256");
        assert_eq!(fields["access_type"], "offline");
        assert_eq!(fields["state"], "fixture-state");
        assert!(!url.as_str().contains("synthetic-not-public"));
    }
}

fn authorization_url(profile: &Profile, redirect: &str, state: &str, challenge: &str) -> url::Url {
    let mut url = url::Url::parse("https://accounts.google.com/o/oauth2/v2/auth").unwrap();
    url.query_pairs_mut().extend_pairs([
        ("client_id", profile.id.as_str()),
        ("redirect_uri", redirect),
        ("response_type", "code"),
        ("scope", &format!("openid email {}", profile.scope)),
        ("access_type", "offline"),
        ("prompt", "consent select_account"),
        ("state", state),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
    ]);
    url
}
