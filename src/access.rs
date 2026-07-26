//! プロジェクトごとのアクセス制御(閲覧・編集許可)。
//! [`RGit`](https://github.com/aon-co-jp/RGit)の`src/access.rs`と
//! 同じ設計思想を、Git forgeの「public/group/push」から
//! チケット管理向けの「private/public、閲覧/編集」に簡略化して移植。

use crate::storage::StorageBackend;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Private,
    Public,
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Private
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AccountPermission {
    pub allow_view: bool,
    pub allow_edit: bool,
    /// このプロジェクトのメンバー管理(保留中のアクセス申請の承認/却下・
    /// 権限付与)を許可するか(ロール権限管理の細分化、2026-07-27追加)。
    /// これが`true`のアカウントは、グローバル管理者(`AppState.admin_email`)
    /// でなくても、この`project_id`宛の申請に限り審査できる
    /// (`main.rs`の`decide_access_request`参照)。Redmine本家の
    /// 「プロジェクトのマネージャーロール」相当だが、ロール名の概念自体は
    /// 導入せず、この1フラグのみを追加した最小実装(正直な開示——
    /// 「Manager/Developer/Reporter」といった名前付きロールのプリセットは
    /// まだ無く、既存の`allow_view`/`allow_edit`と同じ生のフラグとして
    /// 扱う)。
    pub allow_manage_members: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AccessConfig {
    pub mode: Mode,
    pub allow_view: bool,
    pub allow_edit: bool,
    pub accounts: HashMap<String, AccountPermission>,
}

impl Default for AccessConfig {
    fn default() -> Self {
        Self { mode: Mode::Private, allow_view: false, allow_edit: false, accounts: HashMap::new() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    View,
    Edit,
    /// このプロジェクトのメンバー管理(アクセス申請の審査・権限付与)。
    /// `Mode::Public`の`allow_view`/`allow_edit`とは独立——プロジェクトを
    /// 公開設定にしても、それだけでは誰もメンバー管理はできない
    /// (アカウント個別の`allow_manage_members`のみが根拠になる、
    /// 2026-07-27追加)。
    ManageMembers,
}

/// `config`と(ログイン中なら)アカウントのメールアドレスから、`need`の
/// 操作が許可されるかを判定する。管理者ログイン済みかどうかは呼び出し側
/// (`main.rs`)が別途見る——この関数は「public公開ルール、またはアカウント
/// 個別許可として許可されるか」だけを見る。
pub fn is_allowed(config: &AccessConfig, need: Need, account_email: Option<&str>) -> bool {
    if let Some(email) = account_email {
        if let Some(perm) = config.accounts.get(email) {
            let flag = match need {
                Need::View => perm.allow_view,
                Need::Edit => perm.allow_edit,
                Need::ManageMembers => perm.allow_manage_members,
            };
            if flag {
                return true;
            }
        }
    }
    if need == Need::ManageMembers {
        // メンバー管理はpublic公開設定からは決して付与されない
        // (アカウント個別の`allow_manage_members`のみが根拠)。
        return false;
    }
    let flag = match need {
        Need::View => config.allow_view,
        Need::Edit => config.allow_edit,
        Need::ManageMembers => unreachable!("handled above"),
    };
    flag && config.mode == Mode::Public
}

fn access_path(data_root: &Path, project_id: u64) -> PathBuf {
    data_root.join(format!("project-{project_id}-access.json"))
}

pub async fn load(data_root: &Path, project_id: u64, backend: &dyn StorageBackend) -> AccessConfig {
    let path = access_path(data_root, project_id).to_string_lossy().to_string();
    match backend.read(&path).await {
        Ok(bytes) => crate::rustjson::parse_typed(&bytes).unwrap_or_default(),
        Err(_) => AccessConfig::default(),
    }
}

pub async fn save(data_root: &Path, project_id: u64, config: &AccessConfig, backend: &dyn StorageBackend) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(config).expect("AccessConfig serialization is infallible");
    let path = access_path(data_root, project_id).to_string_lossy().to_string();
    backend.write(&path, &bytes).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_project_denies_regardless_of_flags() {
        let config = AccessConfig { mode: Mode::Private, allow_view: true, allow_edit: true, accounts: HashMap::new() };
        assert!(!is_allowed(&config, Need::View, None));
        assert!(!is_allowed(&config, Need::Edit, None));
    }

    #[test]
    fn public_project_respects_view_and_edit_flags_independently() {
        let config = AccessConfig { mode: Mode::Public, allow_view: true, allow_edit: false, accounts: HashMap::new() };
        assert!(is_allowed(&config, Need::View, None));
        assert!(!is_allowed(&config, Need::Edit, None));
    }

    #[test]
    fn account_specific_grant_works_even_when_project_is_private() {
        let mut config = AccessConfig { mode: Mode::Private, allow_view: false, allow_edit: false, accounts: HashMap::new() };
        config.accounts.insert("member@example.com".to_string(), AccountPermission { allow_view: true, allow_edit: false, allow_manage_members: false });
        assert!(is_allowed(&config, Need::View, Some("member@example.com")));
        assert!(!is_allowed(&config, Need::Edit, Some("member@example.com")));
        assert!(!is_allowed(&config, Need::View, Some("someone-else@example.com")));
    }

    #[test]
    fn manage_members_requires_explicit_per_account_grant_and_ignores_public_mode() {
        let mut config = AccessConfig { mode: Mode::Public, allow_view: true, allow_edit: true, accounts: HashMap::new() };
        // publicかつview/edit両方許可でも、メンバー管理は別軸のため未許可。
        assert!(!is_allowed(&config, Need::ManageMembers, None));
        assert!(!is_allowed(&config, Need::ManageMembers, Some("nobody@example.com")));

        config.accounts.insert("manager@example.com".to_string(), AccountPermission { allow_view: true, allow_edit: true, allow_manage_members: true });
        assert!(is_allowed(&config, Need::ManageMembers, Some("manager@example.com")));
        // 他のアカウントには影響しない。
        assert!(!is_allowed(&config, Need::ManageMembers, Some("someone-else@example.com")));
    }
}
