use crate::commands::proxy::ProxyServiceState;
use serde::Serialize;
use tauri::State;

#[derive(Default, Serialize)]
pub struct DeepWriteStatus {
    installed: Option<bool>,
    integration_supported: bool,
    version: Option<String>,
    is_synced: bool,
    integration_enabled: bool,
    current_base_url: Option<String>,
    current_model: Option<String>,
    files: Vec<String>,
}

#[tauri::command]
pub async fn get_deepwrite_sync_status(proxy_url: String) -> Result<DeepWriteStatus, String> {
    #[cfg(target_os = "macos")]
    return macos::status(&proxy_url);
    #[cfg(target_os = "windows")]
    {
        let _ = proxy_url;
        let (installed, version) = tokio::task::spawn_blocking(|| {
            super::windows_agent_install::find_nsis_installation("DeepWrite", "DeepWrite.exe")
        })
        .await
        .map_err(|err| err.to_string())?;
        Ok(DeepWriteStatus {
            installed: Some(installed),
            version,
            ..DeepWriteStatus::default()
        })
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = proxy_url;
        Ok(DeepWriteStatus::default())
    }
}

#[tauri::command]
pub async fn execute_deepwrite_sync(state: State<'_, ProxyServiceState>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let server = state
            .instance
            .read()
            .await
            .as_ref()
            .ok_or("请先启动 Antigravity 网关。")?
            .axum_server
            .clone();
        return macos::connect(server.port, &server.integration_api_key().await).await;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = state;
        Err("DeepWrite 一键接入目前仅支持 macOS。".into())
    }
}

#[tauri::command]
pub async fn execute_deepwrite_clear() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return macos::disconnect();
    #[cfg(not(target_os = "macos"))]
    Err("DeepWrite 一键接入目前仅支持 macOS。".into())
}

#[cfg(target_os = "macos")]
mod macos {
    use super::DeepWriteStatus;
    use aes::cipher::{BlockEncrypt, KeyInit};
    use base64::{engine::general_purpose::STANDARD, Engine};
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    use std::{
        collections::{BTreeSet, HashSet},
        fs::{self, File, OpenOptions},
        io::{ErrorKind, Read, Seek, SeekFrom, Write},
        os::unix::fs::OpenOptionsExt,
        path::{Path, PathBuf},
        process::Stdio,
        time::Duration,
    };
    use zeroize::Zeroizing;

    const PROVIDER_ID: &str = "DeepGemini";
    const ASAR_HEADER_LIMIT: u32 = 16 * 1024 * 1024;
    const PACKAGE_JSON_LIMIT: u64 = 1024 * 1024;
    static SYNC_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn installed_app(home: &Path) -> Option<PathBuf> {
        [PathBuf::from("/Applications"), home.join("Applications")]
            .into_iter()
            .map(|directory| directory.join("DeepWrite.app"))
            .find(|app| app.join("Contents/MacOS/DeepWrite").is_file())
    }

    fn read_asar_package(mut archive: File) -> Option<Value> {
        // ASAR starts with a little-endian Pickle containing the header size,
        // then a Pickle containing the JSON file index. Offsets follow both.
        let mut prefix = [0u8; 16];
        archive.read_exact(&mut prefix).ok()?;
        let field = |offset| u32::from_le_bytes(prefix[offset..offset + 4].try_into().unwrap());
        let header_size = field(4);
        let json_size = field(12);
        if !(8..=ASAR_HEADER_LIMIT).contains(&header_size)
            || field(0) != 4
            || field(8) != header_size - 4
            || json_size > header_size - 8
            || header_size - 8 - json_size > 3
            || header_size % 4 != 0
        {
            return None;
        }
        let mut header = vec![0; json_size as usize];
        archive.read_exact(&mut header).ok()?;
        let index: Value = serde_json::from_slice(&header).ok()?;
        let entry = &index["files"]["package.json"];
        let offset: u64 = entry["offset"].as_str()?.parse().ok()?;
        let size = entry["size"].as_u64()?;
        if size > PACKAGE_JSON_LIMIT {
            return None;
        }
        let start = (8 + u64::from(header_size)).checked_add(offset)?;
        let mut contents = vec![0; size as usize];
        archive.seek(SeekFrom::Start(start)).ok()?;
        archive.read_exact(&mut contents).ok()?;
        serde_json::from_slice(&contents).ok()
    }

    fn electron_storage_name(app: &Path) -> Result<String, String> {
        let resources = app.join("Contents/Resources");
        let package = match File::open(resources.join("app.asar")) {
            Ok(archive) => read_asar_package(archive),
            Err(err) if err.kind() == ErrorKind::NotFound => {
                File::open(resources.join("app/package.json"))
                    .ok()
                    .and_then(|file| {
                        let mut bytes = Vec::new();
                        file.take(PACKAGE_JSON_LIMIT + 1)
                            .read_to_end(&mut bytes)
                            .ok()?;
                        if bytes.len() as u64 > PACKAGE_JSON_LIMIT {
                            return None;
                        }
                        serde_json::from_slice(&bytes).ok()
                    })
            }
            Err(_) => None,
        };
        // Electron initializes Safe Storage with its package name (productName
        // first), not the macOS bundle display name. A stale bundle-name item
        // can exist in the keychain and silently produce unreadable ciphertext.
        package
            .and_then(|package| {
                package
                    .get("productName")
                    .filter(|name| !name.is_null())
                    .or_else(|| package.get("name"))?
                    .as_str()
                    .map(str::trim)
                    .filter(|name| !name.is_empty() && !name.chars().any(char::is_control))
                    .map(str::to_owned)
            })
            .ok_or_else(|| "无法识别 DeepWrite 的安全存储名称，请检查 DeepWrite 安装。".into())
    }

    fn read_json(path: &Path) -> Result<Option<Value>, String> {
        match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|_| "无法读取 DeepWrite 配置：JSON 格式无效。".into()),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(format!("无法读取 DeepWrite 配置：{err}")),
        }
    }

    fn model_path(home: &Path) -> Result<PathBuf, String> {
        let parent = home.join("Library/Application Support/@deepwrite");
        let profile = match read_json(&parent.join(".desktop.storage-location.json"))? {
            None => parent.join("desktop"),
            Some(state) => {
                if !state["pending"].is_null() {
                    return Err("请先在 DeepWrite 完成数据目录迁移。".into());
                }
                let path = state["currentPath"]
                    .as_str()
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute() && state["version"] == 1)
                    .ok_or("DeepWrite 的数据目录配置无效。")?;
                path
            }
        };
        Ok(profile.join("config/models.json"))
    }

    fn installed_profile() -> Result<(PathBuf, PathBuf), String> {
        let home = dirs::home_dir().ok_or("无法定位当前用户的目录。")?;
        let app = installed_app(&home).ok_or("未检测到 DeepWrite 桌面版。")?;
        let path = model_path(&home)?;
        if !path.is_file() {
            return Err("请先在 DeepWrite 中初始化模型配置。".into());
        }
        Ok((app, path))
    }

    fn load_models(path: &Path) -> Result<Value, String> {
        let value = read_json(path)?.ok_or("DeepWrite 模型配置不存在。")?;
        if value["version"] != 3
            || !value["models"].is_array()
            || !value["encryptedApiKeys"].is_object()
        {
            return Err(
                "DeepWrite 模型配置版本不受支持。请在新版 DeepWrite 打开一次后重试。".into(),
            );
        }
        Ok(value)
    }

    struct ConfigLock(PathBuf);
    impl Drop for ConfigLock {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn acquire_config_lock(path: &Path) -> Result<ConfigLock, String> {
        // Same path and owner format as DeepWrite's configuration transaction lock.
        let dir = path.with_file_name(".locks").join("models.json/.deepwrite");
        fs::create_dir_all(&dir).map_err(|err| format!("无法创建 DeepWrite 配置锁：{err}"))?;
        let path = dir.join("transaction.lock");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|err| match err.kind() {
                ErrorKind::AlreadyExists => "DeepWrite 模型配置正被占用，请稍后重试。".into(),
                _ => format!("无法获取 DeepWrite 配置锁：{err}"),
            })?;
        let lock = ConfigLock(path);
        let acquired_at = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|err| format!("无法生成配置锁时间：{err}"))?;
        let owner = json!({ "pid": std::process::id(), "token": uuid::Uuid::new_v4().simple().to_string()[..16], "acquiredAt": acquired_at });
        writeln!(file, "{owner}")
            .and_then(|()| file.sync_all())
            .map_err(|err| format!("无法写入 DeepWrite 配置锁：{err}"))?;
        Ok(lock)
    }

    fn save_models(path: &Path, value: &Value) -> Result<(), String> {
        let tmp = path.with_file_name(format!(".models-antigravity-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| -> std::io::Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp)?;
            serde_json::to_writer_pretty(&mut file, value)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            fs::rename(&tmp, path)?;
            File::open(path.parent().expect("configuration has a parent"))?.sync_all()
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result.map_err(|err| format!("无法保存 DeepWrite 模型配置：{err}"))
    }

    fn managed_id(model_id: &str) -> String {
        let digest = Sha256::digest(model_id.as_bytes());
        format!("antigravity-deepgemini-{}", &format!("{digest:x}")[..24])
    }

    fn is_managed(model: &Value) -> bool {
        model["provider"] == PROVIDER_ID
            && model["modelId"]
                .as_str()
                .is_some_and(|model_id| model["id"] == managed_id(model_id))
    }

    fn gemini_chat(id: &str) -> bool {
        id.starts_with("gemini-")
            && id.len() <= 120
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b))
            && !id.split(['-', '.', '_']).any(|part| {
                matches!(
                    part,
                    "image" | "tts" | "live" | "audio" | "embedding" | "embeddings"
                )
            })
    }

    fn update_models(
        config: &mut Value,
        models: &BTreeSet<String>,
        base_url: &str,
        secret: &str,
    ) -> Result<(), String> {
        let entries = config["models"].as_array_mut().unwrap();
        let prior: Vec<_> = entries
            .iter()
            .filter(|model| is_managed(model))
            .cloned()
            .collect();
        let managed_ids: HashSet<_> = prior
            .iter()
            .filter_map(|model| model["id"].as_str())
            .collect();
        let ids: HashSet<_> = entries
            .iter()
            .filter(|model| !is_managed(model))
            .filter_map(|model| model["id"].as_str())
            .collect();
        if entries.len() - prior.len() + models.len() > 100 {
            return Err("DeepWrite 模型数量超过上限，请移除不使用的模型后重试。".into());
        }
        if models
            .iter()
            .any(|id| ids.contains(managed_id(id).as_str()))
        {
            return Err("DeepWrite 中已有同 ID 的自定义模型，未覆盖现有配置。".into());
        }
        entries.retain(|model| !is_managed(model));
        for model in models {
            entries.push(json!({
                "id": managed_id(model), "label": model, "provider": PROVIDER_ID,
                "api": "openai-completions", "baseUrl": base_url, "modelId": model,
                "reasoning": false, "defaultThinkingLevel": "off"
            }));
        }
        let keys = config["encryptedApiKeys"].as_object_mut().unwrap();
        keys.retain(|id, _| !managed_ids.contains(id.as_str()));
        for model in models {
            keys.insert(managed_id(model), json!(secret));
        }
        if config["defaultModelId"].as_str().is_some_and(|id| {
            managed_ids.contains(id) && !models.iter().any(|model| managed_id(model) == id)
        }) {
            config["defaultModelId"] = json!("");
        }
        Ok(())
    }

    fn write_integration(
        path: &Path,
        models: &BTreeSet<String>,
        base_url: &str,
        secret: &str,
    ) -> Result<(), String> {
        let _guard = acquire_config_lock(path)?;
        let mut config = load_models(path)?;
        update_models(&mut config, models, base_url, secret)?;
        save_models(path, &config)
    }

    pub(super) fn status(proxy_url: &str) -> Result<DeepWriteStatus, String> {
        let home = dirs::home_dir().ok_or("无法定位当前用户的目录。")?;
        let Some(app) = installed_app(&home) else {
            return Ok(DeepWriteStatus {
                installed: Some(false),
                integration_supported: true,
                ..DeepWriteStatus::default()
            });
        };
        let version = plist::Value::from_file(app.join("Contents/Info.plist"))
            .ok()
            .and_then(|plist| {
                plist
                    .as_dictionary()?
                    .get("CFBundleShortVersionString")?
                    .as_string()
                    .map(str::to_owned)
            });
        let path = model_path(&home)?;
        let config = if path.is_file() {
            load_models(&path)?
        } else {
            Value::Null
        };
        let managed: Vec<_> = config["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|model| is_managed(model))
            .collect();
        let current_base_url = managed
            .first()
            .and_then(|model| model["baseUrl"].as_str())
            .filter(|url| {
                reqwest::Url::parse(url).is_ok_and(|url| {
                    url.scheme() == "http"
                        && url.host_str() == Some("127.0.0.1")
                        && url.path() == "/v1"
                        && url.query().is_none()
                        && url.fragment().is_none()
                        && url.username().is_empty()
                        && url.password().is_none()
                })
            })
            .map(str::to_owned);
        Ok(DeepWriteStatus {
            installed: Some(true),
            integration_supported: true,
            version,
            is_synced: !managed.is_empty()
                && managed.iter().all(|model| {
                    model["baseUrl"] == proxy_url
                        && model["id"].as_str().is_some_and(|id| {
                            config["encryptedApiKeys"][id]
                                .as_str()
                                .is_some_and(|key| !key.is_empty())
                        })
                }),
            integration_enabled: !managed.is_empty(),
            current_base_url,
            current_model: managed
                .first()
                .and_then(|model| model["modelId"].as_str())
                .map(str::to_owned),
            files: vec![],
        })
    }

    async fn encrypt_gateway_key(key: &str, storage_name: &str) -> Result<String, String> {
        let service = format!("{storage_name} Safe Storage");
        let account = format!("{storage_name} Key");
        let result = tokio::time::timeout(
            Duration::from_secs(60),
            tokio::process::Command::new("/usr/bin/security")
                .args([
                    "find-generic-password",
                    "-s",
                    &service,
                    "-a",
                    &account,
                    "-w",
                ])
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .output(),
        )
        .await;
        let password = match result {
            Ok(Ok(output)) if output.status.success() => Zeroizing::new(output.stdout),
            _ => return Err("无法访问 DeepWrite 的 macOS 钥匙串。请确认已在 DeepWrite 保存过模型密钥，并允许钥匙串访问。".into()),
        };
        let password = password.strip_suffix(b"\n").unwrap_or(&password);
        if password.is_empty() {
            return Err("DeepWrite 的钥匙串记录为空。".into());
        }
        Ok(encrypt_v10(key.as_bytes(), password))
    }

    fn encrypt_v10(secret: &[u8], password: &[u8]) -> String {
        // Electron's synchronous macOS safeStorage format (Chromium OSCrypt v10).
        let mut key = Zeroizing::new([0u8; 16]);
        ring::pbkdf2::derive(
            ring::pbkdf2::PBKDF2_HMAC_SHA1,
            1003.try_into().unwrap(),
            b"saltysalt",
            password,
            &mut *key,
        );
        let cipher = aes::Aes128::new((&*key).into());
        let padding = 16 - secret.len() % 16;
        let mut padded = Zeroizing::new(secret.to_vec());
        padded.resize(secret.len() + padding, padding as u8);
        let mut previous = [b' '; 16];
        let mut payload = b"v10".to_vec();
        for part in padded.chunks_exact(16) {
            for (byte, plain) in previous.iter_mut().zip(part) {
                *byte ^= plain;
            }
            cipher.encrypt_block((&mut previous).into());
            payload.extend_from_slice(&previous);
        }
        STANDARD.encode(payload)
    }

    pub(super) async fn connect(port: u16, key: &str) -> Result<(), String> {
        let _sync = SYNC_LOCK
            .try_lock()
            .map_err(|_| "DeepWrite 接入设置正在更新。")?;
        let (app, path) = installed_profile()?;
        load_models(&path)?;
        let storage_name = electron_storage_name(&app)?;
        if key.trim().is_empty() {
            return Err("请先为 Antigravity 网关配置 API Key。".into());
        }
        let base_url = format!("http://127.0.0.1:{port}/v1");
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| "无法初始化网关连接。")?;
        let listing: Value = client
            .get(format!("{base_url}/models"))
            .bearer_auth(key)
            .send()
            .await
            .and_then(|response| response.error_for_status())
            .map_err(|_| "无法读取网关模型，请检查网关状态和 API Key。")?
            .json()
            .await
            .map_err(|_| "网关返回的模型列表无效。")?;
        let models: BTreeSet<_> = listing["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|model| model["id"].as_str())
            .filter(|id| gemini_chat(id))
            .map(str::to_owned)
            .collect();
        if models.is_empty() {
            return Err("网关没有可用的 Gemini 对话模型。".into());
        }
        let secret = encrypt_gateway_key(key, &storage_name).await?;
        write_integration(&path, &models, &base_url, &secret)
    }

    pub(super) fn disconnect() -> Result<(), String> {
        let _sync = SYNC_LOCK
            .try_lock()
            .map_err(|_| "DeepWrite 接入设置正在更新。")?;
        let (_, path) = installed_profile()?;
        write_integration(&path, &BTreeSet::new(), "", "")
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn test_asar(package: &Value) -> Vec<u8> {
            let contents = serde_json::to_vec(package).unwrap();
            let header = serde_json::to_vec(&json!({
                "files": { "package.json": { "offset": "3", "size": contents.len() } }
            }))
            .unwrap();
            let padded = header.len().next_multiple_of(4);
            let mut archive = Vec::new();
            for size in [4, 8 + padded, 4 + padded, header.len()] {
                archive.extend_from_slice(&(size as u32).to_le_bytes());
            }
            archive.extend_from_slice(&header);
            archive.resize(16 + padded, 0);
            archive.extend_from_slice(b"xxx");
            archive.extend_from_slice(&contents);
            archive
        }

        #[test]
        fn resolves_safe_storage_name_from_packaged_electron_manifest() {
            let fixture = tempfile::tempdir().unwrap();
            let app = fixture.path().join("DeepWrite.app");
            let resources = app.join("Contents/Resources");
            fs::create_dir_all(resources.join("app")).unwrap();
            fs::write(
                resources.join("app/package.json"),
                br#"{"name":"fallback"}"#,
            )
            .unwrap();
            let archive = resources.join("app.asar");
            let package = json!({"name": "@deepwrite/desktop"});
            fs::write(&archive, test_asar(&package)).unwrap();
            assert_eq!(electron_storage_name(&app).unwrap(), "@deepwrite/desktop");

            let package = json!({"name": "@deepwrite/desktop", "productName": " DeepWrite "});
            fs::write(&archive, test_asar(&package)).unwrap();
            assert_eq!(electron_storage_name(&app).unwrap(), "DeepWrite");

            let mut malformed = test_asar(&package);
            malformed[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
            fs::write(&archive, malformed).unwrap();
            assert!(electron_storage_name(&app).is_err());

            fs::remove_file(&archive).unwrap();
            assert_eq!(electron_storage_name(&app).unwrap(), "fallback");
        }

        #[test]
        fn encrypts_key_in_electron_safe_storage_format() {
            assert_eq!(
                encrypt_v10(b"invalid-fixture-key", b"fixture-password"),
                "djEwmV3BoBYpeDTPGK7SdM/7NLC8KVBnwPSxALana2VWRQs="
            );
        }

        #[test]
        fn only_syncs_gemini_chat_models() {
            assert!(gemini_chat("gemini-3.8-flash-high"));
            assert!(!gemini_chat("gemini-3-pro-image-preview"));
            assert!(!gemini_chat("gemini-embedding-001"));
            assert!(!gemini_chat("claude-test"));
        }

        #[test]
        fn adds_and_removes_only_managed_entries_without_touching_other_settings() {
            let existing = json!({ "version": 3, "defaultModelId": "manual", "models": [
                { "id": "manual", "provider": "DeepGemini", "modelId": "gemini-manual" }
            ], "encryptedApiKeys": { "manual": "test-ciphertext" }, "extra": [1, 2] });
            let profile = tempfile::tempdir().unwrap();
            let path = profile.path().join("models.json");
            save_models(&path, &existing).unwrap();

            let models = BTreeSet::from(["gemini-test".to_string()]);
            let connect =
                |secret| write_integration(&path, &models, "http://example.test/v1", secret);
            let disconnect = || write_integration(&path, &BTreeSet::new(), "", "");

            connect("stale-managed-ciphertext").unwrap();
            connect("new-managed-ciphertext").unwrap();
            let updated = load_models(&path).unwrap();
            assert_eq!(updated["models"][1]["modelId"], "gemini-test");
            assert_eq!(updated["defaultModelId"], "manual");
            assert_eq!(updated["encryptedApiKeys"]["manual"], "test-ciphertext");
            assert_eq!(
                updated["encryptedApiKeys"][managed_id("gemini-test")],
                "new-managed-ciphertext"
            );
            connect("new-managed-ciphertext").unwrap();
            assert_eq!(load_models(&path).unwrap(), updated);

            let lock = acquire_config_lock(&path).unwrap();
            assert_eq!(
                disconnect().unwrap_err(),
                "DeepWrite 模型配置正被占用，请稍后重试。"
            );
            assert_eq!(load_models(&path).unwrap(), updated);
            drop(lock);

            disconnect().unwrap();
            assert_eq!(load_models(&path).unwrap(), existing);
        }
    }
}
