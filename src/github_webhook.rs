//! GitHub Webhook受信によるリアルタイム更新(2026-08-01追加、
//! `src/github.rs`の「次にすべきこと(3) Webhook受信によるリアルタイム
//! 更新」への対応)。
//!
//! GitHubリポジトリ設定画面の「Webhooks」で`push`イベントを
//! `POST /api/github/webhook`へ送るよう設定すると、pushの都度コミット
//! 一覧をローカルにキャッシュする。`GET /api/projects/:id/github/commits`
//! はこのキャッシュが存在すればそれを返し(ポーリング無しで即時反映、
//! GitHub APIのレート制限も回避)、無ければ従来通りGitHub APIへ都度
//! 問い合わせる(後方互換、Webhook未設定のプロジェクトは無変更で動作)。
//!
//! **正直な開示**: 署名検証(`X-Hub-Signature-256`、HMAC-SHA256)は
//! `RSCHIKETTO_GITHUB_WEBHOOK_SECRET`が設定されている場合のみ強制する。
//! 未設定の場合はWebhook自体を受け付けない(`501`)——検証キーが無い
//! 状態で任意のペイロードを信用してキャッシュへ書き込むのは、なりすまし
//! による偽コミット情報の注入を許すことになるため。

use crate::github::{parse_referenced_ticket_ids, GithubCommit};
use crate::storage::StorageBackend;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Default, Deserialize, serde::Serialize)]
pub struct WebhookCommitCache {
    /// `"owner/repo"` -> 直近のpushで受け取ったコミット一覧
    /// (GitHub API同様、最新が先頭になるようpush受信のたびに前置する)。
    pub by_repo: HashMap<String, Vec<GithubCommit>>,
}

fn cache_path(data_root: &Path) -> PathBuf {
    data_root.join("github_webhook_cache.json")
}

pub async fn load(data_root: &Path, backend: &dyn StorageBackend) -> WebhookCommitCache {
    let path = cache_path(data_root).to_string_lossy().to_string();
    match backend.read(&path).await {
        Ok(bytes) => crate::rustjson::parse_typed(&bytes).unwrap_or_default(),
        Err(_) => WebhookCommitCache::default(),
    }
}

pub async fn save(data_root: &Path, cache: &WebhookCommitCache, backend: &dyn StorageBackend) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(cache).expect("WebhookCommitCache serialization is infallible");
    let path = cache_path(data_root).to_string_lossy().to_string();
    backend.write(&path, &bytes).await
}

/// `X-Hub-Signature-256`ヘッダー(`"sha256=<hex>"`形式)を検証する。
/// タイミング攻撃を避けるため、`hmac`クレートの`Mac::verify_slice`
/// (定数時間比較)を使う——自前でのバイト比較は行わない。
pub fn verify_signature(secret: &str, body: &[u8], signature_header: &str) -> bool {
    let Some(hex_sig) = signature_header.strip_prefix("sha256=") else {
        return false;
    };
    let Ok(sig_bytes) = hex_decode(hex_sig) else {
        return false;
    };
    let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&sig_bytes).is_ok()
}

fn hex_decode(s: &str) -> Result<Vec<u8>, ()> {
    if s.len() % 2 != 0 {
        return Err(());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| ()))
        .collect()
}

#[derive(Debug, Deserialize)]
struct PushEventRepository {
    full_name: String,
}

#[derive(Debug, Deserialize)]
struct PushEventCommitAuthor {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PushEventCommit {
    id: String,
    message: String,
    timestamp: Option<String>,
    url: Option<String>,
    author: Option<PushEventCommitAuthor>,
}

#[derive(Debug, Deserialize)]
struct PushEventPayload {
    repository: PushEventRepository,
    commits: Vec<PushEventCommit>,
}

/// GitHub `push`イベントのペイロードをパースし、`(owner/repo,
/// 新規コミット一覧)`を返す。`commits`イベント配列は古い順のため、
/// 一覧表示(新しい順)に合わせて反転する。
pub fn parse_push_event(body: &[u8]) -> anyhow::Result<(String, Vec<GithubCommit>)> {
    let payload: PushEventPayload = serde_json::from_slice(body)?;
    let mut commits: Vec<GithubCommit> = payload
        .commits
        .into_iter()
        .map(|c| GithubCommit {
            referenced_ticket_ids: parse_referenced_ticket_ids(&c.message),
            sha: c.id,
            author_name: c.author.and_then(|a| a.name).unwrap_or_else(|| "unknown".to_string()),
            date: c.timestamp.unwrap_or_default(),
            html_url: c.url.unwrap_or_default(),
            message: c.message,
        })
        .collect();
    commits.reverse();
    Ok((payload.repository.full_name, commits))
}

/// Webhookで受け取った新規コミットをキャッシュへマージする
/// (同じrepoの既存キャッシュの先頭に追加、最大30件まで保持——GitHub API
/// の1ページ分と同じ上限に揃え、無制限増大を防ぐ)。
pub fn merge_commits(cache: &mut WebhookCommitCache, repo_spec: String, mut new_commits: Vec<GithubCommit>) {
    let existing = cache.by_repo.entry(repo_spec).or_default();
    new_commits.append(existing);
    new_commits.truncate(30);
    *existing = new_commits;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_roundtrips_and_rejects_tampering() {
        let secret = "test-secret";
        let body = br#"{"repository":{"full_name":"a/b"},"commits":[]}"#;
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let sig: String = mac.finalize().into_bytes().iter().map(|b| format!("{b:02x}")).collect();
        let header = format!("sha256={sig}");

        assert!(verify_signature(secret, body, &header));
        assert!(!verify_signature("wrong-secret", body, &header));
        assert!(!verify_signature(secret, b"tampered body", &header));
        assert!(!verify_signature(secret, body, "not-even-hex-format"));
    }

    #[test]
    fn parses_push_event_and_orders_oldest_first_reversed_to_newest_first() {
        let body = br#"{
            "repository": {"full_name": "aon-co-jp/open-redmine"},
            "commits": [
                {"id": "aaa", "message": "first commit", "timestamp": "2026-08-01T00:00:00Z", "url": "https://x/aaa", "author": {"name": "alice"}},
                {"id": "bbb", "message": "second, fixes #7", "timestamp": "2026-08-01T00:01:00Z", "url": "https://x/bbb", "author": {"name": "bob"}}
            ]
        }"#;
        let (repo, commits) = parse_push_event(body).unwrap();
        assert_eq!(repo, "aon-co-jp/open-redmine");
        assert_eq!(commits.len(), 2);
        // 新しい順(2番目のコミットが先頭)。
        assert_eq!(commits[0].sha, "bbb");
        assert_eq!(commits[0].referenced_ticket_ids, vec![7]);
        assert_eq!(commits[1].sha, "aaa");
    }

    #[test]
    fn merge_prepends_new_commits_and_caps_at_thirty() {
        let mut cache = WebhookCommitCache::default();
        let make = |sha: &str| GithubCommit {
            sha: sha.to_string(),
            message: String::new(),
            author_name: String::new(),
            date: String::new(),
            html_url: String::new(),
            referenced_ticket_ids: vec![],
        };
        merge_commits(&mut cache, "a/b".to_string(), vec![make("1")]);
        merge_commits(&mut cache, "a/b".to_string(), vec![make("2")]);
        let stored = &cache.by_repo["a/b"];
        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0].sha, "2");
        assert_eq!(stored[1].sha, "1");

        let mut cache2 = WebhookCommitCache::default();
        for i in 0..35 {
            merge_commits(&mut cache2, "a/b".to_string(), vec![make(&i.to_string())]);
        }
        assert_eq!(cache2.by_repo["a/b"].len(), 30);
    }
}
