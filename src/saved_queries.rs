//! 保存済みカスタムクエリ(Redmine機能ギャップ対応、2026-07-31追加)。
//! `GET /api/tickets`が受け付ける絞り込み条件(`status`/`project_id`/
//! `tracker`/`assignee`)の組み合わせを、名前を付けて保存・再実行できる
//! ようにする。フィルタ条件自体の評価ロジックは`main.rs::list_tickets`と
//! 重複させず、`main.rs`側から`SavedQuery`のフィールドをそのまま
//! `list_tickets`が使うのと同じフィルタ処理に渡す設計とする
//! (`main.rs::run_saved_query`参照)。

use crate::storage::StorageBackend;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedQuery {
    pub id: u64,
    /// 作成者のメールアドレス(このクエリを一覧・削除できるのは作成者
    /// 本人または管理者のみ)。
    pub owner_email: String,
    pub name: String,
    #[serde(default)]
    pub project_id: Option<u64>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub tracker: Option<String>,
    #[serde(default)]
    pub assignee: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SavedQueryStore {
    pub next_id: u64,
    pub queries: Vec<SavedQuery>,
}

impl SavedQueryStore {
    pub fn find(&self, id: u64) -> Option<&SavedQuery> {
        self.queries.iter().find(|q| q.id == id)
    }

    /// `owner_email`が作成した保存済みクエリのみを返す(他人のクエリは
    /// 見えない、管理者であっても——保存済みクエリはあくまで個人用の
    /// ショートカットという位置づけ、正直な開示: プロジェクト共有の
    /// クエリという概念はRedmine本家にあるが今回は個人用のみ)。
    pub fn owned_by<'a>(&'a self, owner_email: &'a str) -> Vec<&'a SavedQuery> {
        self.queries.iter().filter(|q| q.owner_email == owner_email).collect()
    }
}

fn saved_queries_path(data_root: &Path) -> PathBuf {
    data_root.join("saved_queries.json")
}

pub async fn load(data_root: &Path, backend: &dyn StorageBackend) -> SavedQueryStore {
    let path = saved_queries_path(data_root).to_string_lossy().to_string();
    match backend.read(&path).await {
        Ok(bytes) => crate::rustjson::parse_typed(&bytes).unwrap_or_default(),
        Err(_) => SavedQueryStore::default(),
    }
}

pub async fn save(data_root: &Path, store: &SavedQueryStore, backend: &dyn StorageBackend) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(store).expect("SavedQueryStore serialization is infallible");
    let path = saved_queries_path(data_root).to_string_lossy().to_string();
    backend.write(&path, &bytes).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn save_and_load_round_trips() {
        let dir = std::env::temp_dir().join(format!("rschiketto-savedqueries-test-{}", std::process::id()));
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let mut store = SavedQueryStore::default();
        let id = store.next_id;
        store.next_id += 1;
        store.queries.push(SavedQuery {
            id,
            owner_email: "alice@example.com".to_string(),
            name: "my open bugs".to_string(),
            project_id: Some(1),
            status: Some("open".to_string()),
            tracker: Some("bug".to_string()),
            assignee: None,
            created_at: crate::project::now_rfc3339(),
        });
        save(&dir, &store, &crate::storage::LocalFsBackend).await.unwrap();

        let loaded = load(&dir, &crate::storage::LocalFsBackend).await;
        assert_eq!(loaded.queries.len(), 1);
        assert_eq!(loaded.queries[0].name, "my open bugs");

        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[test]
    fn owned_by_filters_to_a_single_owner() {
        let mut store = SavedQueryStore::default();
        store.queries.push(SavedQuery {
            id: 0,
            owner_email: "alice@example.com".to_string(),
            name: "a".to_string(),
            project_id: None,
            status: None,
            tracker: None,
            assignee: None,
            created_at: String::new(),
        });
        store.queries.push(SavedQuery {
            id: 1,
            owner_email: "bob@example.com".to_string(),
            name: "b".to_string(),
            project_id: None,
            status: None,
            tracker: None,
            assignee: None,
            created_at: String::new(),
        });
        let alice_queries = store.owned_by("alice@example.com");
        assert_eq!(alice_queries.len(), 1);
        assert_eq!(alice_queries[0].name, "a");
    }
}
