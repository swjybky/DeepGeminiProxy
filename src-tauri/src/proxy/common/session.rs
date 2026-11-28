// [NEW v4.1.24] Tools for deriving stable session identifiers

/// From account ID string to a stable negative signed integer session ID
/// Implements FNV-1a hash which matches the official client behavior of sending
/// a large negative integer for `sessionId`.
///
/// 折叠顺序是「先异或再乘」，与原版 Stravia 的 `native_session_id` 逐位一致。
/// 空输入因此得到 offset basis 本身 —— 官方抓包里看到的 `-3750763034362895579`
/// 就是这个值，可用作算法对齐的锚点。
pub fn derive_session_id(account_id: &str) -> String {
    let hash = account_id
        .bytes()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x1_0000_01b3)
        });
    (hash as i64).to_string()
}

/// 上一轮出站 `labels.trajectory_id` 的账本。
///
/// IDE 抓包的 `labels` 里第一个键是 `last_execution_id`，值是**上一轮真实**的
/// 轨迹 UUID。它记录的是既成事实，不是可推导的常量：没有上一轮就不发，
/// 有上一轮就如实回填。因此这里只存真发出过的值，绝不合成。
static LAST_TRAJECTORY: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, String>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn trajectory_key(account_id: &str, store_key: &str) -> String {
    format!("{}::{}", account_id, store_key)
}

/// 读取该会话上一轮发出的轨迹 ID。没有上一轮时返回 `None` —— 调用方此时不应写该字段。
pub fn last_trajectory_id(account_id: &str, store_key: &str) -> Option<String> {
    LAST_TRAJECTORY
        .lock()
        .ok()
        .and_then(|map| map.get(&trajectory_key(account_id, store_key)).cloned())
}

/// 记下本轮真正发出的轨迹 ID，供下一轮作为 `last_execution_id` 回填。
pub fn remember_trajectory_id(account_id: &str, store_key: &str, trajectory_id: &str) {
    if let Ok(mut map) = LAST_TRAJECTORY.lock() {
        map.insert(
            trajectory_key(account_id, store_key),
            trajectory_id.to_string(),
        );
    }
}

// [FIX session-1M] Upstream accumulates conversation input server-side per sessionId.
// A session that drives many tool loops can push the accumulated input past 1M tokens,
// after which EVERY request with the same sessionId fails with
// 400 "The input token count exceeds the maximum number of tokens allowed 1048576"
// until the upstream session expires. Bumping the sessionId generation forces the
// upstream to start a fresh session, transparently recovering the conversation.

/// Monotonic generation counter per (account_id, conversation fingerprint).
static SESSION_BUMPS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, u64>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn bump_key(account_id: &str, fingerprint: &str) -> String {
    format!("{}::{}", account_id, fingerprint)
}

/// Current sessionId generation for (account, conversation). Starts at 0.
pub fn current_bump(account_id: &str, fingerprint: &str) -> u64 {
    if let Ok(map) = SESSION_BUMPS.lock() {
        map.get(&bump_key(account_id, fingerprint))
            .copied()
            .unwrap_or(0)
    } else {
        0
    }
}

/// Advance the sessionId generation for (account, conversation) and return the new value.
pub fn bump_session(account_id: &str, fingerprint: &str) -> u64 {
    if let Ok(mut map) = SESSION_BUMPS.lock() {
        let counter = map.entry(bump_key(account_id, fingerprint)).or_insert(0);
        *counter += 1;
        tracing::warn!(
            "[Session] Bumped sessionId generation to {} for account {} fingerprint {}",
            counter,
            &account_id[..account_id.len().min(8)],
            &fingerprint[..fingerprint.len().min(16)]
        );
        *counter
    } else {
        0
    }
}

/// Derive the upstream sessionId for (account, conversation fingerprint, generation).
/// Stable within one conversation generation (keeps upstream server-side cache hits),
/// but distinct across conversations and after a bump.
pub fn derive_session_scoped(account_id: &str, fingerprint: &str, generation: u64) -> String {
    if fingerprint.is_empty() && generation == 0 {
        return derive_session_id(account_id);
    }
    derive_session_id(&format!("{}|{}|{}", account_id, fingerprint, generation))
}

/// 上游 `sessionId` 跟思维库的 `store_key` 走。账号粘性是另一把键。
pub fn apply_upstream_session(inner: &mut serde_json::Value, account_id: &str, store_key: &str) {
    let generation = current_bump(account_id, store_key);
    inner["sessionId"] =
        serde_json::json!(derive_session_scoped(account_id, store_key, generation));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_session_id() {
        let x = derive_session_id("my_account@gmail.com");
        let y = derive_session_id("my_account@gmail.com");
        assert_eq!(x, y);
    }

    /// 算法锚点：官方抓包的 sessionId 就是 FNV-1a64 的 offset basis，
    /// 说明折叠顺序是「先异或再乘」。换成别的顺序这一条立刻失败。
    #[test]
    fn test_derive_session_id_matches_the_captured_offset_basis() {
        assert_eq!(derive_session_id(""), "-3750763034362895579");
    }

    #[test]
    fn test_bump_isolated_per_account_and_fingerprint() {
        let a1 = current_bump("acc1", "fp1");
        let a2 = current_bump("acc2", "fp1");
        let a3 = current_bump("acc1", "fp2");
        assert_eq!(a1, 0);
        assert_eq!(a2, 0);
        assert_eq!(a3, 0);

        assert_eq!(bump_session("acc1", "fp1"), 1);
        assert_eq!(current_bump("acc1", "fp1"), 1);
        // other keys untouched
        assert_eq!(current_bump("acc2", "fp1"), 0);
        assert_eq!(current_bump("acc1", "fp2"), 0);

        assert_eq!(bump_session("acc1", "fp1"), 2);
        assert_eq!(current_bump("acc1", "fp1"), 2);
    }

    #[test]
    fn test_derive_session_scoped() {
        // Generation 0 with empty fingerprint keeps legacy behavior
        assert_eq!(
            derive_session_scoped("acc", "", 0),
            derive_session_id("acc")
        );
        // Same inputs -> stable
        assert_eq!(
            derive_session_scoped("acc", "fp", 0),
            derive_session_scoped("acc", "fp", 0)
        );
        // Different fingerprint or generation -> distinct sessions
        assert_ne!(
            derive_session_scoped("acc", "fp1", 0),
            derive_session_scoped("acc", "fp2", 0)
        );
        assert_ne!(
            derive_session_scoped("acc", "fp", 0),
            derive_session_scoped("acc", "fp", 1)
        );
        // Still a valid signed-integer string (official client format)
        assert!(derive_session_scoped("acc", "fp", 1).parse::<i64>().is_ok());
    }
}
