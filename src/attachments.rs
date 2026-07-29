//! チケットへの添付ファイル(`Attachment`)。`comments.rs`と同じ設計
//! (対象チケットが所属する`Project`への編集権限を持つ認証済みアカウント
//! のみがアップロードでき、モデレーションキューは不要)。
//!
//! メタデータ(ファイル名・サイズ・投稿者・作成日時)は既存の
//! `project.rs`/`comments.rs`と同じJSONファイルパターンで永続化する。
//! ファイル本体は`StorageBackend`(local/sftp/gdrive)経由で
//! `attachments/<id>_<sanitized_filename>`に保存する(メタデータの
//! JSONとは別ファイル——サイズの大きいバイナリを都度JSON全体へ
//! シリアライズし直すコストを避けるため)。

use crate::storage::StorageBackend;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub id: u64,
    pub ticket_id: u64,
    pub author_email: String,
    pub file_name: String,
    pub content_type: String,
    pub size_bytes: u64,
    pub created_at: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AttachmentStore {
    pub next_id: u64,
    pub attachments: Vec<Attachment>,
}

impl AttachmentStore {
    pub fn for_ticket(&self, ticket_id: u64) -> Vec<&Attachment> {
        self.attachments.iter().filter(|a| a.ticket_id == ticket_id).collect()
    }

    pub fn find(&self, id: u64) -> Option<&Attachment> {
        self.attachments.iter().find(|a| a.id == id)
    }
}

fn attachments_meta_path(data_root: &Path) -> PathBuf {
    data_root.join("attachments.json")
}

/// ファイル本体の保存パス。ファイル名に含まれうる`/`等のパス区切り文字・
/// 先頭の`.`(隠しファイル・親ディレクトリ参照対策)を取り除いた
/// サニタイズ済み名を使い、IDを前置してファイル名衝突を防ぐ
/// (同名ファイルの複数アップロードに対応するため)。
pub fn attachment_blob_path(data_root: &Path, id: u64, file_name: &str) -> PathBuf {
    let replaced: String = file_name
        .chars()
        .map(|c| if c == '/' || c == '\\' || c == '\0' { '_' } else { c })
        .collect();
    // パストラバーサル対策: `..`が残っているとパス区切り文字置換後も
    // 親ディレクトリ参照になりうるため(例: "..\\.." → "_.._" のような
    // 部分文字列)、`..`という2文字連続自体も明示的に潰す。
    let mut sanitized = replaced;
    while sanitized.contains("..") {
        sanitized = sanitized.replace("..", "__");
    }
    let sanitized = sanitized.trim_start_matches('.');
    let sanitized = if sanitized.is_empty() { "file" } else { sanitized };
    data_root.join("attachments").join(format!("{id}_{sanitized}"))
}

pub async fn load(data_root: &Path, backend: &dyn StorageBackend) -> AttachmentStore {
    let path = attachments_meta_path(data_root).to_string_lossy().to_string();
    match backend.read(&path).await {
        Ok(bytes) => crate::rustjson::parse_typed(&bytes).unwrap_or_default(),
        Err(_) => AttachmentStore::default(),
    }
}

pub async fn save(data_root: &Path, store: &AttachmentStore, backend: &dyn StorageBackend) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(store).expect("AttachmentStore serialization is infallible");
    let path = attachments_meta_path(data_root).to_string_lossy().to_string();
    backend.write(&path, &bytes).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn save_and_load_round_trips() {
        let dir = std::env::temp_dir().join(format!("rschiketto-attachment-test-{}", std::process::id()));
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let mut store = AttachmentStore::default();
        let id = store.next_id;
        store.next_id += 1;
        store.attachments.push(Attachment {
            id,
            ticket_id: 7,
            author_email: "member@example.com".to_string(),
            file_name: "screenshot.png".to_string(),
            content_type: "image/png".to_string(),
            size_bytes: 12345,
            created_at: crate::project::now_rfc3339(),
        });
        save(&dir, &store, &crate::storage::LocalFsBackend).await.unwrap();

        let loaded = load(&dir, &crate::storage::LocalFsBackend).await;
        assert_eq!(loaded.attachments.len(), 1);
        assert_eq!(loaded.for_ticket(7).len(), 1);
        assert_eq!(loaded.for_ticket(999).len(), 0);
        assert!(loaded.find(id).is_some());

        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[tokio::test]
    async fn load_missing_file_returns_default() {
        let dir = std::env::temp_dir().join(format!("rschiketto-attachment-missing-{}", std::process::id()));
        let store = load(&dir, &crate::storage::LocalFsBackend).await;
        assert_eq!(store.attachments.len(), 0);
        assert_eq!(store.next_id, 0);
    }

    #[test]
    fn attachment_blob_path_sanitizes_traversal_and_separators() {
        let root = PathBuf::from("/data");

        let traversal = attachment_blob_path(&root, 3, "../../etc/passwd");
        let traversal_str = traversal.to_string_lossy();
        assert!(!traversal_str.contains(".."), "must not retain any '..' component: {traversal_str}");
        assert!(traversal.starts_with(root.join("attachments")));

        assert_eq!(
            attachment_blob_path(&root, 5, "sub/dir\\file.txt"),
            PathBuf::from("/data/attachments/5_sub_dir_file.txt")
        );
        assert_eq!(attachment_blob_path(&root, 9, ""), PathBuf::from("/data/attachments/9_file"));
    }
}
