//! 登録済みメールアドレスの管理、および「誰でも申請できる」アクセス
//! リクエストの受付。[`RGit`](https://github.com/aon-co-jp/RGit)の
//! `src/accounts.rs`と同じ設計思想(管理者による直接登録、および
//! 自己申請→管理者審査の2経路)。

use crate::storage::StorageBackend;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessRequest {
    pub id: String,
    pub email: String,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AccountStore {
    pub emails: HashSet<String>,
    pub pending_requests: Vec<AccessRequest>,
}

fn accounts_path(data_root: &Path) -> PathBuf {
    data_root.join(".rschiketto-accounts.json")
}

pub async fn load(data_root: &Path, backend: &dyn StorageBackend) -> AccountStore {
    let path = accounts_path(data_root).to_string_lossy().to_string();
    match backend.read(&path).await {
        Ok(bytes) => crate::rustjson::parse_typed(&bytes).unwrap_or_default(),
        Err(_) => AccountStore::default(),
    }
}

pub async fn save(data_root: &Path, store: &AccountStore, backend: &dyn StorageBackend) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(store).expect("AccountStore serialization is infallible");
    let path = accounts_path(data_root).to_string_lossy().to_string();
    backend.write(&path, &bytes).await
}

pub fn generate_request_id() -> String {
    use rand::RngExt;
    let mut rng = rand::rng();
    let bytes: [u8; 12] = rng.random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_request_id_produces_distinct_hex_ids() {
        let a = generate_request_id();
        let b = generate_request_id();
        assert_ne!(a, b);
        assert_eq!(a.len(), 24);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[tokio::test]
    async fn store_round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("rschiketto-accounts-test-{}", generate_request_id()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let mut store = AccountStore::default();
        store.emails.insert("member@example.com".to_string());
        store.pending_requests.push(AccessRequest { id: "abc".to_string(), email: "pending@example.com".to_string(), message: None });
        save(&dir, &store, &crate::storage::LocalFsBackend).await.unwrap();
        let loaded = load(&dir, &crate::storage::LocalFsBackend).await;
        assert!(loaded.emails.contains("member@example.com"));
        assert_eq!(loaded.pending_requests.len(), 1);
        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[tokio::test]
    async fn missing_store_file_loads_as_default() {
        let dir = std::env::temp_dir().join(format!("rschiketto-accounts-test-missing-{}", generate_request_id()));
        // deliberately do not create the directory/file
        let loaded = load(&dir, &crate::storage::LocalFsBackend).await;
        assert!(loaded.emails.is_empty());
        assert!(loaded.pending_requests.is_empty());
    }
}
