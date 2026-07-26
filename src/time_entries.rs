//! チケットへの作業時間記録(`TimeEntry`)。Redmineの時間トラッキング機能の
//! うち、記録(投稿・一覧・削除)にスコープを絞る(Redmine本家にある
//! 「作業分類(Activity)のプロジェクト単位カスタマイズ」「時間集計レポート
//! 画面」は今回は対象外——正直な開示。`activity`は自由入力の文字列として
//! 保持する簡易実装)。永続化は既存の`comments.rs`と同じJSONファイル
//! パターン。

use crate::storage::StorageBackend;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeEntry {
    pub id: u64,
    pub ticket_id: u64,
    pub author_email: String,
    /// 作業時間(時間単位、Redmineの`hours`フィールドと同じ単位)。
    pub hours: f64,
    /// 作業分類(例: "Development"/"Design"、自由入力)。
    pub activity: String,
    #[serde(default)]
    pub comments: String,
    /// 作業日(`YYYY-MM-DD`形式の文字列保持、既存の`start_date`等と同じ
    /// 単純なパターンを踏襲)。
    pub spent_on: String,
    pub created_at: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TimeEntryStore {
    pub next_id: u64,
    pub entries: Vec<TimeEntry>,
}

impl TimeEntryStore {
    pub fn for_ticket(&self, ticket_id: u64) -> Vec<&TimeEntry> {
        self.entries.iter().filter(|e| e.ticket_id == ticket_id).collect()
    }

    pub fn find(&self, id: u64) -> Option<&TimeEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// プロジェクト内の指定チケット群に対する合計作業時間
    /// (`ticket_ids`はプロジェクトに属する全チケットIDを呼び出し側で渡す)。
    pub fn total_hours_for(&self, ticket_ids: &[u64]) -> f64 {
        self.entries.iter().filter(|e| ticket_ids.contains(&e.ticket_id)).map(|e| e.hours).sum()
    }
}

fn time_entries_path(data_root: &Path) -> PathBuf {
    data_root.join("time_entries.json")
}

pub async fn load(data_root: &Path, backend: &dyn StorageBackend) -> TimeEntryStore {
    let path = time_entries_path(data_root).to_string_lossy().to_string();
    match backend.read(&path).await {
        Ok(bytes) => crate::rustjson::parse_typed(&bytes).unwrap_or_default(),
        Err(_) => TimeEntryStore::default(),
    }
}

pub async fn save(data_root: &Path, store: &TimeEntryStore, backend: &dyn StorageBackend) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(store).expect("TimeEntryStore serialization is infallible");
    let path = time_entries_path(data_root).to_string_lossy().to_string();
    backend.write(&path, &bytes).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn save_and_load_round_trips() {
        let dir = std::env::temp_dir().join(format!("rschiketto-timeentry-test-{}", std::process::id()));
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let mut store = TimeEntryStore::default();
        let id = store.next_id;
        store.next_id += 1;
        store.entries.push(TimeEntry {
            id,
            ticket_id: 5,
            author_email: "member@example.com".to_string(),
            hours: 2.5,
            activity: "Development".to_string(),
            comments: "fixed the bug".to_string(),
            spent_on: "2026-07-26".to_string(),
            created_at: crate::project::now_rfc3339(),
        });
        save(&dir, &store, &crate::storage::LocalFsBackend).await.unwrap();

        let loaded = load(&dir, &crate::storage::LocalFsBackend).await;
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.for_ticket(5).len(), 1);
        assert_eq!(loaded.for_ticket(999).len(), 0);
        assert!(loaded.find(id).is_some());
        assert!((loaded.total_hours_for(&[5]) - 2.5).abs() < f64::EPSILON);

        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[tokio::test]
    async fn load_missing_file_returns_default() {
        let dir = std::env::temp_dir().join(format!("rschiketto-timeentry-missing-{}", std::process::id()));
        let store = load(&dir, &crate::storage::LocalFsBackend).await;
        assert_eq!(store.entries.len(), 0);
        assert_eq!(store.next_id, 0);
    }

    #[test]
    fn total_hours_for_sums_only_matching_ticket_ids() {
        let mut store = TimeEntryStore::default();
        store.entries.push(TimeEntry {
            id: 0,
            ticket_id: 1,
            author_email: "a@example.com".to_string(),
            hours: 1.0,
            activity: "Development".to_string(),
            comments: String::new(),
            spent_on: "2026-07-26".to_string(),
            created_at: "unix:0".to_string(),
        });
        store.entries.push(TimeEntry {
            id: 1,
            ticket_id: 2,
            author_email: "a@example.com".to_string(),
            hours: 3.0,
            activity: "Design".to_string(),
            comments: String::new(),
            spent_on: "2026-07-26".to_string(),
            created_at: "unix:0".to_string(),
        });
        assert!((store.total_hours_for(&[1, 2]) - 4.0).abs() < f64::EPSILON);
        assert!((store.total_hours_for(&[1]) - 1.0).abs() < f64::EPSILON);
        assert!((store.total_hours_for(&[999]) - 0.0).abs() < f64::EPSILON);
    }
}
