//! チケット間の関連(`IssueRelation`)。Redmineの「関連するチケット」機能の
//! うち、実用上価値が高く実装コストが妥当な3種類(`blocks`/`blocked_by`は
//! 実質的に向きが逆の同じ関係のため`Blocks`の1バリアントで表現し、表示側で
//! `from`/`to`どちらの立場かにより「ブロックする/ブロックされている」を
//! 判定する、`duplicates`/`precedes`)にスコープを絞る(Redmine本家にある
//! `copied_to`/`relates`等の全種類は今回は対象外——正直な開示)。
//! 永続化は既存の`comments.rs`/`project.rs`と同じJSONファイルパターン。

use crate::storage::StorageBackend;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    /// `from_ticket_id`が`to_ticket_id`をブロックしている
    /// (`to_ticket_id`から見れば「ブロックされている」)。
    Blocks,
    /// `from_ticket_id`は`to_ticket_id`の重複である。
    Duplicates,
    /// `from_ticket_id`は`to_ticket_id`より先に完了すべき(先行関係)。
    Precedes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueRelation {
    pub id: u64,
    pub from_ticket_id: u64,
    pub to_ticket_id: u64,
    pub kind: RelationKind,
    pub created_at: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RelationStore {
    pub next_id: u64,
    pub relations: Vec<IssueRelation>,
}

impl RelationStore {
    /// 指定したチケットが`from`側・`to`側いずれかとして関わる関連を全て返す。
    pub fn for_ticket(&self, ticket_id: u64) -> Vec<&IssueRelation> {
        self.relations.iter().filter(|r| r.from_ticket_id == ticket_id || r.to_ticket_id == ticket_id).collect()
    }

    pub fn find(&self, id: u64) -> Option<&IssueRelation> {
        self.relations.iter().find(|r| r.id == id)
    }

    /// 同じ`(from, to, kind)`の組み合わせが既に存在するか(重複登録防止)。
    pub fn duplicate_exists(&self, from: u64, to: u64, kind: RelationKind) -> bool {
        self.relations.iter().any(|r| r.from_ticket_id == from && r.to_ticket_id == to && r.kind == kind)
    }
}

fn relations_path(data_root: &Path) -> PathBuf {
    data_root.join("relations.json")
}

pub async fn load(data_root: &Path, backend: &dyn StorageBackend) -> RelationStore {
    let path = relations_path(data_root).to_string_lossy().to_string();
    match backend.read(&path).await {
        Ok(bytes) => crate::rustjson::parse_typed(&bytes).unwrap_or_default(),
        Err(_) => RelationStore::default(),
    }
}

pub async fn save(data_root: &Path, store: &RelationStore, backend: &dyn StorageBackend) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(store).expect("RelationStore serialization is infallible");
    let path = relations_path(data_root).to_string_lossy().to_string();
    backend.write(&path, &bytes).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn save_and_load_round_trips() {
        let dir = std::env::temp_dir().join(format!("rschiketto-relation-test-{}", std::process::id()));
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let mut store = RelationStore::default();
        let id = store.next_id;
        store.next_id += 1;
        store.relations.push(IssueRelation {
            id,
            from_ticket_id: 1,
            to_ticket_id: 2,
            kind: RelationKind::Blocks,
            created_at: crate::project::now_rfc3339(),
        });
        save(&dir, &store, &crate::storage::LocalFsBackend).await.unwrap();

        let loaded = load(&dir, &crate::storage::LocalFsBackend).await;
        assert_eq!(loaded.relations.len(), 1);
        assert_eq!(loaded.for_ticket(1).len(), 1);
        assert_eq!(loaded.for_ticket(2).len(), 1);
        assert_eq!(loaded.for_ticket(999).len(), 0);
        assert!(loaded.find(id).is_some());

        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[test]
    fn duplicate_exists_matches_exact_triple_only() {
        let mut store = RelationStore::default();
        store.relations.push(IssueRelation {
            id: 0,
            from_ticket_id: 1,
            to_ticket_id: 2,
            kind: RelationKind::Blocks,
            created_at: "unix:0".to_string(),
        });
        assert!(store.duplicate_exists(1, 2, RelationKind::Blocks));
        assert!(!store.duplicate_exists(2, 1, RelationKind::Blocks));
        assert!(!store.duplicate_exists(1, 2, RelationKind::Duplicates));
    }

    #[tokio::test]
    async fn load_missing_file_returns_default() {
        let dir = std::env::temp_dir().join(format!("rschiketto-relation-missing-{}", std::process::id()));
        let store = load(&dir, &crate::storage::LocalFsBackend).await;
        assert_eq!(store.relations.len(), 0);
        assert_eq!(store.next_id, 0);
    }
}
