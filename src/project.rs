//! プロジェクト(`Project`)のCRUD。v0.1.0時点ではチケットの`project`
//! フィールドが単純な文字列ラベル+`DefaultHasher`によるハッシュ値
//! だった(`access.rs`参照)ものを、実体を持つエンティティに置き換える
//! (`CLAUDE.md`のHANDOFFに記載の宿題「(3) Project自体のCRUD」への対応)。
//! 永続化は既存の`accounts.rs`/`main.rs`のTicketStoreと同じJSONファイル
//! パターンを踏襲。

use crate::storage::StorageBackend;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: u64,
    pub name: String,
    pub description: String,
    /// 親プロジェクトの`id`(サブプロジェクト階層、`None`ならトップレベル)。
    #[serde(default)]
    pub parent_id: Option<u64>,
    /// このプロジェクト配下のチケットが持てるカスタムフィールド名の一覧
    /// (Redmine機能ギャップ対応、2026-07-31追加)。`Ticket::custom_fields`
    /// のキーはここに含まれる名前でなければならない。フィールドの型指定
    /// (数値/真偽値/リスト/日付)は行わない自由文字列のみの最小実装
    /// (正直な開示——Redmine本家のカスタムフィールド管理画面のような
    /// 型ごとのバリデーション・必須指定は今回のスコープ外)。
    #[serde(default)]
    pub custom_field_defs: Vec<String>,
    /// このプロジェクト配下のチケットが選択できるカテゴリ名の一覧
    /// (Redmine本家の「トラッカーの分類」相当、2026-07-31追加)。
    /// `Ticket::category`はここに含まれる名前でなければならない
    /// (`custom_field_defs`と同じ検証パターンを踏襲)。
    #[serde(default)]
    pub category_defs: Vec<String>,
    /// 連携するGitHubリポジトリ(`"owner/repo"`形式、Redmine本家のSCM
    /// (リポジトリ)連携相当、2026-07-31追加)。`None`なら未連携。
    /// GitHub側の認証情報(PAT)は`RSCHIKETTO_GITHUB_TOKEN`環境変数
    /// (任意、未設定なら未認証の公開APIレート制限内で動作)。
    #[serde(default)]
    pub github_repo: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ProjectStore {
    pub next_id: u64,
    pub projects: Vec<Project>,
}

impl ProjectStore {
    pub fn find(&self, id: u64) -> Option<&Project> {
        self.projects.iter().find(|p| p.id == id)
    }

    pub fn exists(&self, id: u64) -> bool {
        self.find(id).is_some()
    }

    pub fn children_of(&self, id: u64) -> Vec<&Project> {
        self.projects.iter().filter(|p| p.parent_id == Some(id)).collect()
    }

    /// `candidate_parent`を`id`の親として設定した場合に循環参照が生じないかを
    /// 判定する(`id`が`candidate_parent`の祖先チェーンに含まれていないか、
    /// 親チェーンを辿って確認する)。`candidate_parent == id`(自分自身を親にする)
    /// も循環として拒否する。
    pub fn would_create_cycle(&self, id: u64, candidate_parent: u64) -> bool {
        if candidate_parent == id {
            return true;
        }
        let mut current = Some(candidate_parent);
        let mut guard = 0usize;
        while let Some(cur) = current {
            if cur == id {
                return true;
            }
            guard += 1;
            if guard > self.projects.len() + 1 {
                // 既存データが壊れて循環している場合の無限ループ防止。
                return true;
            }
            current = self.find(cur).and_then(|p| p.parent_id);
        }
        false
    }
}

fn projects_path(data_root: &Path) -> PathBuf {
    data_root.join("projects.json")
}

pub async fn load(data_root: &Path, backend: &dyn StorageBackend) -> ProjectStore {
    let path = projects_path(data_root).to_string_lossy().to_string();
    match backend.read(&path).await {
        Ok(bytes) => crate::rustjson::parse_typed(&bytes).unwrap_or_default(),
        Err(_) => ProjectStore::default(),
    }
}

pub async fn save(data_root: &Path, store: &ProjectStore, backend: &dyn StorageBackend) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(store).expect("ProjectStore serialization is infallible");
    let path = projects_path(data_root).to_string_lossy().to_string();
    backend.write(&path, &bytes).await
}

pub fn now_rfc3339() -> String {
    // 依存を増やさないため、`chrono`等は使わず簡易なUNIX秒表記のISO風
    // 文字列にとどめる(既存コードに時刻フォーマットの前例が無いため、
    // 最小実装として採用——将来より厳密な形式が必要になれば置き換える)。
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("unix:{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn save_and_load_round_trips() {
        let dir = std::env::temp_dir().join(format!("rschiketto-project-test-{}", std::process::id()));
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let mut store = ProjectStore::default();
        let id = store.next_id;
        store.next_id += 1;
        store.projects.push(Project {
            id,
            name: "demo".to_string(),
            description: "a demo project".to_string(),
            parent_id: None,
            custom_field_defs: Vec::new(),
            category_defs: Vec::new(),
            github_repo: None,
            created_at: now_rfc3339(),
            updated_at: now_rfc3339(),
        });
        save(&dir, &store, &crate::storage::LocalFsBackend).await.unwrap();

        let loaded = load(&dir, &crate::storage::LocalFsBackend).await;
        assert_eq!(loaded.projects.len(), 1);
        assert_eq!(loaded.projects[0].name, "demo");
        assert!(loaded.exists(id));
        assert!(!loaded.exists(id + 1));

        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[test]
    fn would_create_cycle_detects_self_and_ancestor_cycles() {
        let mut store = ProjectStore::default();
        let mk = |id: u64, parent_id: Option<u64>| Project {
            id,
            name: format!("p{id}"),
            description: String::new(),
            parent_id,
            custom_field_defs: Vec::new(),
            category_defs: Vec::new(),
            github_repo: None,
            created_at: now_rfc3339(),
            updated_at: now_rfc3339(),
        };
        // 0 -> 1 -> 2 (0がroot)
        store.projects.push(mk(0, None));
        store.projects.push(mk(1, Some(0)));
        store.projects.push(mk(2, Some(1)));

        // 自分自身を親にするのは循環。
        assert!(store.would_create_cycle(1, 1));
        // 0の親を2にすると、2の祖先チェーンに0が含まれるため循環。
        assert!(store.would_create_cycle(0, 2));
        // 無関係なノードを親にするのは問題ない。
        store.projects.push(mk(3, None));
        assert!(!store.would_create_cycle(3, 0));
    }

    #[tokio::test]
    async fn load_missing_file_returns_default() {
        let dir = std::env::temp_dir().join(format!("rschiketto-project-missing-{}", std::process::id()));
        let store = load(&dir, &crate::storage::LocalFsBackend).await;
        assert_eq!(store.projects.len(), 0);
        assert_eq!(store.next_id, 0);
    }
}
