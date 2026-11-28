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
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    return tokio::task::spawn_blocking(move || desktop::status(&proxy_url))
        .await
        .map_err(|err| err.to_string())?;
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = proxy_url;
        Ok(DeepWriteStatus::default())
    }
}

#[tauri::command]
pub async fn execute_deepwrite_sync(state: State<'_, ProxyServiceState>) -> Result<(), String> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let server = state
            .instance
            .read()
            .await
            .as_ref()
            .ok_or("请先启动 Antigravity 网关。")?
            .axum_server
            .clone();
        return desktop::connect(server.port, &server.integration_api_key().await).await;
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = state;
        Err("DeepWrite 一键接入目前支持 macOS 和 Windows。".into())
    }
}

#[tauri::command]
pub async fn execute_deepwrite_clear() -> Result<(), String> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    return tokio::task::spawn_blocking(desktop::disconnect)
        .await
        .map_err(|err| err.to_string())?;
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    Err("DeepWrite 一键接入目前支持 macOS 和 Windows。".into())
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod desktop {
    use super::DeepWriteStatus;
    #[cfg(target_os = "macos")]
    use aes::cipher::{BlockEncrypt, KeyInit};
    use base64::{engine::general_purpose::STANDARD, Engine};
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    use std::{
        collections::{BTreeSet, HashSet},
        fs::{self, OpenOptions},
        io::{ErrorKind, Write},
        path::{Path, PathBuf},
        time::Duration,
    };
    #[cfg(target_os = "macos")]
    use std::{
        fs::File,
        io::{Read, Seek, SeekFrom},
        os::unix::fs::OpenOptionsExt,
        process::Stdio,
    };
    use zeroize::Zeroizing;

    const PROVIDER_ID: &str = "DeepGemini";
    #[cfg(target_os = "macos")]
    const ASAR_HEADER_LIMIT: u32 = 16 * 1024 * 1024;
    #[cfg(target_os = "macos")]
    const PACKAGE_JSON_LIMIT: u64 = 1024 * 1024;
    static SYNC_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[cfg(target_os = "macos")]
    fn installed_app(home: &Path) -> Option<PathBuf> {
        [PathBuf::from("/Applications"), home.join("Applications")]
            .into_iter()
            .map(|directory| directory.join("DeepWrite.app"))
            .find(|app| app.join("Contents/MacOS/DeepWrite").is_file())
    }

    #[cfg(target_os = "macos")]
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

    #[cfg(target_os = "macos")]
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

    fn model_path(data_dir: &Path) -> Result<PathBuf, String> {
        let parent = data_dir.join("@deepwrite");
        let profile = match read_json(&parent.join(".desktop.storage-location.json"))? {
            None => parent.join("desktop"),
            Some(state) => {
                if !state["pending"].is_null() {
                    return Err("请先在 DeepWrite 完成数据目录迁移。".into());
                }
                let path = state["currentPath"]
                    .as_str()
                    .filter(|path| !path.contains('\0'))
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute())
                    .ok_or("DeepWrite 的数据目录配置无效。")?;
                path
            }
        };
        Ok(profile.join("config/models.json"))
    }

    fn installed_model_path() -> Result<PathBuf, String> {
        let path = model_path(&data_dir()?)?;
        if !path.is_file() {
            return Err("请先在 DeepWrite 中初始化模型配置。".into());
        }
        Ok(path)
    }

    fn data_dir() -> Result<PathBuf, String> {
        dirs::data_dir().ok_or_else(|| "无法定位当前用户的应用数据目录。".into())
    }

    struct ModelConfig {
        settings: Value,
        legacy_secrets: Option<Value>,
    }

    fn load_models(path: &Path) -> Result<ModelConfig, String> {
        let value = read_json(path)?.ok_or("DeepWrite 模型配置不存在。")?;
        if !value["models"].is_array() {
            return Err("DeepWrite 模型配置中的模型列表无效。".into());
        }
        let legacy_secrets = match value.get("encryptedApiKeys") {
            Some(keys) if keys.is_object() => None,
            Some(_) => return Err("DeepWrite 的模型密钥配置格式无效。".into()),
            None => {
                let secrets = match read_json(&path.with_file_name("model-secrets.json"))? {
                    Some(secrets) => secrets,
                    // Only these layouts can be identified without either key map.
                    None if matches!(value["version"].as_u64(), Some(1 | 2)) => {
                        json!({ "version": 1, "encryptedApiKeys": {} })
                    }
                    None => return Err("无法识别 DeepWrite 的模型密钥存储格式。".into()),
                };
                if !secrets["encryptedApiKeys"].is_object() {
                    return Err("DeepWrite 的独立密钥配置格式无效。".into());
                }
                Some(secrets)
            }
        };
        Ok(ModelConfig {
            settings: value,
            legacy_secrets,
        })
    }

    fn ensure_profile_can_be_edited(path: &Path, version: Option<&str>) -> Result<(), String> {
        let config = load_models(path)?;
        let lacks_shared_lock = config.legacy_secrets.is_some()
            || version.is_some_and(|version| {
                let parts: Vec<_> = version
                    .trim_start_matches(['v', 'V'])
                    .split(['-', '+'])
                    .next()
                    .unwrap_or_default()
                    .split('.')
                    .take(3)
                    .map(str::parse::<u64>)
                    .collect();
                matches!(parts.as_slice(), [Ok(major), Ok(minor), Ok(patch)] if (*major, *minor, *patch) < (1, 8, 3))
            });
        if lacks_shared_lock && crate::modules::process::is_process_running_by_name("DeepWrite") {
            return Err(
                "此版本的 DeepWrite 不支持同时编辑模型配置，请退出 DeepWrite 后重试。".into(),
            );
        }
        Ok(())
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
        let mut file = private_file_options()
            .open(&path)
            .map_err(|err| match err.kind() {
                ErrorKind::AlreadyExists => "DeepWrite 模型配置正被占用，请稍后重试。".into(),
                _ => format!("无法获取 DeepWrite 配置锁：{err}"),
            })?;
        let lock = ConfigLock(path);
        let acquired_at = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|err| format!("无法生成配置锁时间：{err}"))?;
        let owner = json!({
            "pid": std::process::id(),
            "token": uuid::Uuid::new_v4().simple().to_string()[..8],
            "acquiredAt": acquired_at
        });
        writeln!(file, "{owner}")
            .and_then(|()| file.sync_all())
            .map_err(|err| format!("无法写入 DeepWrite 配置锁：{err}"))?;
        Ok(lock)
    }

    fn private_file_options() -> OpenOptions {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(target_os = "macos")]
        options.mode(0o600);
        options
    }

    fn save_json(path: &Path, value: &Value) -> Result<(), String> {
        let tmp = path.with_file_name(format!(".models-antigravity-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| -> std::io::Result<()> {
            let mut file = private_file_options().open(&tmp)?;
            serde_json::to_writer_pretty(&mut file, value)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            drop(file);
            fs::rename(&tmp, path)
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
        settings: &mut Value,
        secrets: Option<&mut Value>,
        models: &BTreeSet<String>,
        base_url: &str,
        secret: &str,
    ) -> Result<(), String> {
        // Some v1 readers reject the entire configuration above 50 models.
        let max_models = if secrets.is_some() && settings["version"] == 1 {
            50
        } else {
            100
        };
        let entries = settings["models"].as_array_mut().unwrap();
        let (managed, manual): (Vec<_>, Vec<_>) =
            entries.iter().partition(|model| is_managed(model));
        let ids: HashSet<_> = manual
            .iter()
            .filter_map(|model| model["id"].as_str())
            .collect();
        if manual.len() + models.len() > max_models && !models.is_empty() {
            return Err("DeepWrite 模型数量超过上限，请移除不使用的模型后重试。".into());
        }
        if models
            .iter()
            .any(|id| ids.contains(managed_id(id).as_str()))
        {
            return Err("DeepWrite 中已有同 ID 的自定义模型，未覆盖现有配置。".into());
        }
        let managed_ids: HashSet<_> = managed
            .into_iter()
            .filter_map(|model| model["id"].as_str().map(str::to_owned))
            .collect();
        entries.retain(|model| !is_managed(model));
        for model in models {
            entries.push(json!({
                "id": managed_id(model), "label": model, "provider": PROVIDER_ID,
                "api": "openai-completions", "baseUrl": base_url, "modelId": model,
                "reasoning": false, "defaultThinkingLevel": "off"
            }));
        }
        let keys = secrets.unwrap_or(&mut *settings)["encryptedApiKeys"]
            .as_object_mut()
            .unwrap();
        keys.retain(|id, _| !managed_ids.contains(id));
        for model in models {
            keys.insert(managed_id(model), json!(secret));
        }
        if settings["defaultModelId"].as_str().is_some_and(|id| {
            managed_ids.contains(id) && !models.iter().any(|model| managed_id(model) == id)
        }) {
            settings["defaultModelId"] = json!("");
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
        let ModelConfig {
            mut settings,
            legacy_secrets,
        } = load_models(path)?;
        let mut next_secrets = legacy_secrets.clone();
        update_models(
            &mut settings,
            next_secrets.as_mut(),
            models,
            base_url,
            secret,
        )?;
        if let (Some(previous), Some(next)) = (legacy_secrets, next_secrets) {
            let secrets_path = path.with_file_name("model-secrets.json");
            let had_secrets = secrets_path.try_exists().map_err(|err| err.to_string())?;
            // Keep removed credentials available until the old model list is replaced.
            let mut staged = previous.clone();
            staged["encryptedApiKeys"]
                .as_object_mut()
                .unwrap()
                .extend(next["encryptedApiKeys"].as_object().unwrap().clone());
            if staged != previous {
                save_json(&secrets_path, &staged)?;
            }
            if let Err(error) = save_json(path, &settings) {
                if staged != previous {
                    let restore = if had_secrets {
                        save_json(&secrets_path, &previous)
                    } else {
                        fs::remove_file(&secrets_path).map_err(|err| err.to_string())
                    };
                    restore.map_err(|err| format!("{error}；还原独立密钥配置失败：{err}"))?;
                }
                return Err(error);
            }
            if next != staged {
                save_json(&secrets_path, &next)
                    .map_err(|err| format!("DeepWrite 模型配置已更新，但清理旧密钥失败：{err}"))?;
            }
        } else {
            save_json(path, &settings)?;
        }
        #[cfg(target_os = "macos")]
        File::open(path.parent().expect("configuration has a parent"))
            .and_then(|directory| directory.sync_all())
            .map_err(|err| format!("无法同步 DeepWrite 配置目录：{err}"))?;
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn installed_version() -> Result<(bool, Option<String>), String> {
        let home = dirs::home_dir().ok_or("无法定位当前用户的目录。")?;
        let Some(app) = installed_app(&home) else {
            return Ok((false, None));
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
        Ok((true, version))
    }

    #[cfg(target_os = "windows")]
    fn installed_version() -> Result<(bool, Option<String>), String> {
        Ok(super::super::windows_agent_install::find_nsis_installation(
            "DeepWrite",
            "DeepWrite.exe",
        ))
    }

    pub(super) fn status(proxy_url: &str) -> Result<DeepWriteStatus, String> {
        let (installed, version) = installed_version()?;
        let status = DeepWriteStatus {
            installed: Some(installed),
            integration_supported: true,
            version,
            ..DeepWriteStatus::default()
        };
        if !installed {
            return Ok(status);
        }
        let path = model_path(&data_dir()?)?;
        if !path.try_exists().map_err(|err| err.to_string())? {
            return Ok(status);
        }
        let ModelConfig {
            settings,
            legacy_secrets,
        } = load_models(&path)?;
        let keys = &legacy_secrets.as_ref().unwrap_or(&settings)["encryptedApiKeys"];
        let managed: Vec<_> = settings["models"]
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
                    // 专用入口优先；兼容改造前写入的 /v1 接入。
                    matches!(url.path(), "/deepwrite/v1" | "/v1")
                        && url.scheme() == "http"
                        && url.host_str() == Some("127.0.0.1")
                        && url.query().is_none()
                        && url.fragment().is_none()
                        && url.username().is_empty()
                        && url.password().is_none()
                })
            })
            .map(str::to_owned);
        Ok(DeepWriteStatus {
            is_synced: !managed.is_empty()
                && managed.iter().all(|model| {
                    model["baseUrl"] == proxy_url
                        && model["id"]
                            .as_str()
                            .is_some_and(|id| keys[id].as_str().is_some_and(|key| !key.is_empty()))
                }),
            integration_enabled: !managed.is_empty(),
            current_base_url,
            current_model: managed
                .first()
                .and_then(|model| model["modelId"].as_str())
                .map(str::to_owned),
            ..status
        })
    }

    #[cfg(target_os = "macos")]
    async fn encrypt_gateway_key(key: &str, _model_path: &Path) -> Result<String, String> {
        let home = dirs::home_dir().ok_or("无法定位当前用户的目录。")?;
        let app = installed_app(&home).ok_or("未检测到 DeepWrite 桌面版。")?;
        let storage_name = electron_storage_name(&app)?;
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

    #[cfg(target_os = "macos")]
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

    #[cfg(target_os = "windows")]
    async fn encrypt_gateway_key(key: &str, model_path: &Path) -> Result<String, String> {
        let local_state = model_path
            .parent()
            .and_then(Path::parent)
            .ok_or("无法定位 DeepWrite 的 Windows 加密配置。")?
            .join("Local State");
        let state =
            read_json(&local_state)?.ok_or("请先启动一次 DeepWrite，初始化 Windows 安全存储。")?;
        let mut wrapped_key = state["os_crypt"]["encrypted_key"]
            .as_str()
            .and_then(|value| STANDARD.decode(value).ok())
            .ok_or("DeepWrite 的 Windows 安全存储缺少加密密钥。")?;
        if !wrapped_key.starts_with(b"DPAPI") || wrapped_key.len() == 5 {
            return Err("无法识别 DeepWrite 的 Windows 加密密钥格式。".into());
        }
        let storage_key = unprotect_windows_key(&mut wrapped_key[5..])?;
        encrypt_windows_v10(key.as_bytes(), &storage_key)
    }

    #[cfg(target_os = "windows")]
    fn unprotect_windows_key(encrypted: &mut [u8]) -> Result<Zeroizing<[u8; 32]>, String> {
        use std::ptr::{null, null_mut};
        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::Cryptography::{
                CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
            },
        };
        use zeroize::Zeroize;

        let input = CRYPT_INTEGER_BLOB {
            cbData: encrypted
                .len()
                .try_into()
                .map_err(|_| "Windows 加密密钥过大。")?,
            pbData: encrypted.as_mut_ptr(),
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: null_mut(),
        };
        // SAFETY: input points to encrypted for the duration of the call; DPAPI allocates output.
        let success = unsafe {
            CryptUnprotectData(
                &input,
                null_mut(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if success == 0 || output.pbData.is_null() {
            return Err("无法在当前 Windows 用户下解锁 DeepWrite 的安全存储。".into());
        }
        let mut key = Zeroizing::new([0; 32]);
        // SAFETY: On success DPAPI returns cbData bytes owned by this call; free them with LocalFree.
        let valid_length = unsafe {
            let bytes = std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize);
            let valid = bytes.len() == key.len();
            if valid {
                key.copy_from_slice(bytes);
            }
            bytes.zeroize();
            LocalFree(output.pbData.cast());
            valid
        };
        if !valid_length {
            return Err("DeepWrite 的 Windows 加密密钥长度无效。".into());
        }
        Ok(key)
    }

    #[cfg(target_os = "windows")]
    fn encrypt_windows_v10(secret: &[u8], key: &[u8; 32]) -> Result<String, String> {
        use aes_gcm::{
            aead::{Aead, AeadCore, OsRng},
            Aes256Gcm, KeyInit,
        };

        // Electron synchronous safeStorage on Windows reads the Chromium v10 AES-GCM envelope.
        let cipher = Aes256Gcm::new(key.into());
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = cipher
            .encrypt(&nonce, secret)
            .map_err(|_| "无法加密 DeepWrite 的网关密钥。")?;
        let mut payload = b"v10".to_vec();
        payload.extend_from_slice(&nonce);
        payload.extend_from_slice(&ciphertext);
        Ok(STANDARD.encode(payload))
    }

    pub(super) async fn connect(port: u16, key: &str) -> Result<(), String> {
        let _sync = SYNC_LOCK
            .try_lock()
            .map_err(|_| "DeepWrite 接入设置正在更新。")?;
        let (installed, version) = installed_version()?;
        if !installed {
            return Err("未检测到 DeepWrite 桌面版。".into());
        }
        let path = installed_model_path()?;
        ensure_profile_can_be_edited(&path, version.as_deref())?;
        if key.trim().is_empty() {
            return Err("请先为 Antigravity 网关配置 API Key。".into());
        }
        // 走 DeepWrite 专用入口：与 /v1 共用同一套处理器与上游转换，
        // 只把会话身份与通用入口隔离，便于单独对照出站组包。
        let base_url = format!("http://127.0.0.1:{port}/deepwrite/v1");
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
        let secret = encrypt_gateway_key(key, &path).await?;
        if installed_model_path()? != path {
            return Err("DeepWrite 的数据目录已变更，请重新接入。".into());
        }
        ensure_profile_can_be_edited(&path, version.as_deref())?;
        write_integration(&path, &models, &base_url, &secret)
    }

    pub(super) fn disconnect() -> Result<(), String> {
        let _sync = SYNC_LOCK
            .try_lock()
            .map_err(|_| "DeepWrite 接入设置正在更新。")?;
        let path = installed_model_path()?;
        let (_, version) = installed_version()?;
        ensure_profile_can_be_edited(&path, version.as_deref())?;
        write_integration(&path, &BTreeSet::new(), "", "")
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[cfg(target_os = "macos")]
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
        #[cfg(target_os = "macos")]
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
        #[cfg(target_os = "macos")]
        fn encrypts_key_in_electron_safe_storage_format() {
            assert_eq!(
                encrypt_v10(b"invalid-fixture-key", b"fixture-password"),
                "djEwmV3BoBYpeDTPGK7SdM/7NLC8KVBnwPSxALana2VWRQs="
            );
        }

        #[tokio::test]
        #[cfg(target_os = "windows")]
        async fn encrypts_key_in_windows_electron_safe_storage_format() {
            use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
            use std::ptr::{null, null_mut};
            use windows_sys::Win32::{
                Foundation::LocalFree,
                Security::Cryptography::{
                    CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
                },
            };

            let mut key = [42; 32];
            let input = CRYPT_INTEGER_BLOB {
                cbData: key.len() as u32,
                pbData: key.as_mut_ptr(),
            };
            let mut output = CRYPT_INTEGER_BLOB {
                cbData: 0,
                pbData: null_mut(),
            };
            // SAFETY: input points to the fixture key; output is initialized by DPAPI.
            let success = unsafe {
                CryptProtectData(
                    &input,
                    null(),
                    null(),
                    null(),
                    null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
            };
            assert_ne!(success, 0);
            assert!(!output.pbData.is_null());
            let mut wrapped = b"DPAPI".to_vec();
            // SAFETY: output owns cbData bytes after success and must be released with LocalFree.
            unsafe {
                wrapped.extend_from_slice(std::slice::from_raw_parts(
                    output.pbData,
                    output.cbData as usize,
                ));
                LocalFree(output.pbData.cast());
            }
            let profile = tempfile::tempdir().unwrap();
            save_json(
                &profile.path().join("Local State"),
                &json!({
                    "os_crypt": { "encrypted_key": STANDARD.encode(wrapped) }
                }),
            )
            .unwrap();
            let models_path = profile.path().join("config/models.json");
            let encoded = encrypt_gateway_key("fixture-gateway-key", &models_path)
                .await
                .unwrap();
            let bytes = STANDARD.decode(encoded).unwrap();
            assert_eq!(&bytes[..3], b"v10");
            let decoded = Aes256Gcm::new((&key).into())
                .decrypt(Nonce::from_slice(&bytes[3..15]), &bytes[15..])
                .unwrap();
            assert_eq!(decoded, b"fixture-gateway-key");
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
            save_json(&path, &existing).unwrap();

            let models = BTreeSet::from(["gemini-test".to_string()]);
            let connect =
                |secret| write_integration(&path, &models, "http://example.test/v1", secret);
            let disconnect = || write_integration(&path, &BTreeSet::new(), "", "");

            connect("stale-managed-ciphertext").unwrap();
            connect("new-managed-ciphertext").unwrap();
            let updated = load_models(&path).unwrap().settings;
            assert_eq!(updated["models"][1]["modelId"], "gemini-test");
            assert_eq!(updated["defaultModelId"], "manual");
            assert_eq!(updated["encryptedApiKeys"]["manual"], "test-ciphertext");
            assert_eq!(
                updated["encryptedApiKeys"][managed_id("gemini-test")],
                "new-managed-ciphertext"
            );
            connect("new-managed-ciphertext").unwrap();
            assert_eq!(load_models(&path).unwrap().settings, updated);

            let lock = acquire_config_lock(&path).unwrap();
            let owner = read_json(&lock.0).unwrap().unwrap();
            assert_eq!(owner["token"].as_str().unwrap().len(), 8);
            assert_eq!(
                disconnect().unwrap_err(),
                "DeepWrite 模型配置正被占用，请稍后重试。"
            );
            assert_eq!(load_models(&path).unwrap().settings, updated);
            drop(lock);

            disconnect().unwrap();
            assert_eq!(load_models(&path).unwrap().settings, existing);
        }

        #[test]
        fn preserves_legacy_settings_and_secrets_layouts() {
            for (version, secrets_version) in [(1, 1), (2, 1), (90, 91)] {
                let profile = tempfile::tempdir().unwrap();
                let path = profile.path().join("models.json");
                let secrets_path = path.with_file_name("model-secrets.json");
                let settings = json!({
                    "version": version, "defaultModelId": "manual",
                    "models": [{ "id": "manual", "modelId": "gemini-manual" }],
                    "extra": [1, 2]
                });
                let secrets = json!({
                    "version": secrets_version, "encryptedApiKeys": { "manual": "fixture-secret" },
                    "extra": true
                });
                save_json(&path, &settings).unwrap();
                save_json(&secrets_path, &secrets).unwrap();
                let models = BTreeSet::from(["gemini-test".to_string()]);
                write_integration(&path, &models, "http://example.test/v1", "fixture").unwrap();
                let current = load_models(&path).unwrap();
                assert_eq!(current.settings["version"], version);
                assert_eq!(current.settings["models"][1]["modelId"], "gemini-test");
                assert!(current.settings.get("encryptedApiKeys").is_none());
                assert_eq!(
                    current.legacy_secrets.unwrap()["encryptedApiKeys"][managed_id("gemini-test")],
                    "fixture"
                );

                write_integration(&path, &BTreeSet::new(), "", "").unwrap();
                assert_eq!(read_json(&path).unwrap().unwrap(), settings);
                assert_eq!(read_json(&secrets_path).unwrap().unwrap(), secrets);
            }
        }

        #[test]
        fn syncs_inline_keys_without_a_known_config_version() {
            let profile = tempfile::tempdir().unwrap();
            let path = profile.path().join("models.json");
            let models = BTreeSet::from(["gemini-test".to_string()]);
            for version in [None, Some(4), Some(500)] {
                let mut settings = json!({
                    "models": [], "encryptedApiKeys": {}, "anotherSetting": [true]
                });
                if let Some(version) = version {
                    settings["version"] = json!(version);
                }
                save_json(&path, &settings).unwrap();
                write_integration(&path, &models, "http://example.test/v1", "fixture").unwrap();

                let current = load_models(&path).unwrap();
                assert_eq!(current.settings.get("version"), settings.get("version"));
                assert_eq!(current.settings["models"][0]["modelId"], "gemini-test");
                assert_eq!(
                    current.settings["encryptedApiKeys"][managed_id("gemini-test")],
                    "fixture"
                );
                assert!(current.legacy_secrets.is_none());

                write_integration(&path, &BTreeSet::new(), "", "").unwrap();
                assert_eq!(read_json(&path).unwrap().unwrap(), settings);
            }
        }

        #[test]
        fn keeps_unrecognized_config_untouched() {
            let profile = tempfile::tempdir().unwrap();
            let path = profile.path().join("models.json");
            let unknown = json!({ "version": 4, "models": [], "keys": {} });
            save_json(&path, &unknown).unwrap();
            let models = BTreeSet::from(["gemini-test".to_string()]);
            assert!(
                write_integration(&path, &models, "http://example.test/v1", "fixture").is_err()
            );
            assert_eq!(read_json(&path).unwrap().unwrap(), unknown);

            let secrets_path = path.with_file_name("model-secrets.json");
            let secrets = json!({ "version": 1, "encryptedApiKeys": {} });
            save_json(&secrets_path, &secrets).unwrap();
            let malformed = json!({ "version": 4, "models": [], "encryptedApiKeys": [] });
            save_json(&path, &malformed).unwrap();
            assert!(
                write_integration(&path, &models, "http://example.test/v1", "fixture").is_err()
            );
            assert_eq!(read_json(&path).unwrap().unwrap(), malformed);
            assert_eq!(read_json(&secrets_path).unwrap().unwrap(), secrets);
        }

        #[test]
        fn resolves_relocated_profile_and_rejects_pending_migration() {
            let profile = tempfile::tempdir().unwrap();
            let root = profile.path().join("@deepwrite");
            fs::create_dir(&root).unwrap();
            assert_eq!(
                model_path(profile.path()).unwrap(),
                root.join("desktop/config/models.json")
            );

            let pointer = root.join(".desktop.storage-location.json");
            let relocated = profile.path().join("relocated");
            let mut state = json!({ "version": 42, "currentPath": relocated });
            save_json(&pointer, &state).unwrap();
            assert_eq!(
                model_path(profile.path()).unwrap(),
                relocated.join("config/models.json")
            );

            state["pending"] = json!({});
            save_json(&pointer, &state).unwrap();
            assert!(model_path(profile.path()).is_err());
        }
    }
}
