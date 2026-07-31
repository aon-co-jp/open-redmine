//! # RS-Chiketto (v0.1.0)
//!
//! [Redmine](https://redmine.org/)(実際にはRuby on Rails製)の、
//! ハイスピード・ハイセキュリティ・省メモリなRust+[poem](https://github.com/poem-web/poem)版を目指す。
//!
//! ## 正直な開示(最重要、`RGit`/`aruaru-llm`と同じ流儀)
//!
//! **v0.1.0時点では、チケット(Issue)・プロジェクトのCRUD、プロジェクトの
//! サブプロジェクト階層、チケットへのコメント、プロジェクト単位Wiki
//! (改訂履歴保持)を実装している。**
//! Redmineが持つ以下の機能は**まだ一切無い**:
//!
//! - ガントチャート・カレンダー
//! - フォーラム
//! - リポジトリ連携(SCM閲覧、[`RGit`](https://github.com/aon-co-jp/RGit)との連携は将来検討)
//! - カスタムフィールド・ワークフロー
//!
//! 認証は[`RGit`](https://github.com/aon-co-jp/RGit)で先行実装した
//! OTPログイン(固定管理者+登録アカウント)をそのまま移植して使用。
//! ストレージは現時点でJSONファイル永続化(`aruaru-db`/PostgreSQL
//! DUAL DB構成への移行は未着手、`CLAUDE.md`のHANDOFF参照)。

mod access;
mod accounts;
mod attachments;
mod auth;
mod comments;
mod ddns;
mod mail;
mod project;
mod relations;
mod rustjson;
mod saved_queries;
mod storage;
mod time_entries;
mod wiki;

use std::path::PathBuf;
use std::sync::Arc;

use poem::listener::TcpListener;
use poem::middleware::Tracing;
use poem::web::Data;
use poem::{
    delete, get, handler, post,
    web::Path as PathExtractor,
    EndpointExt, Request, Response, Result as PoemResult, Route, Server,
};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
struct AppState {
    data_root: PathBuf,
    auth: Arc<auth::AuthStore>,
    admin_email: String,
    smtp: Option<mail::SmtpConfig>,
    /// `RSCHIKETTO_ACCOUNTS_LOCKED`(既定`true`)。`RGit`と同じ方針で、
    /// ロック中は管理者以外のアカウント登録・申請承認を拒否する。
    accounts_locked: bool,
    /// データ/DB永続化の実I/O先(`RSCHIKETTO_STORAGE_BACKEND`で選択、
    /// 既定は`LocalFsBackend`)。全`Store`の`load`/`save`はこれ経由で
    /// I/Oを行う(`storage.rs`参照)。
    backend: Arc<dyn storage::StorageBackend>,
}

fn require_admin_session(req: &Request, state: &AppState) -> PoemResult<()> {
    let header = req.header(poem::http::header::AUTHORIZATION).unwrap_or("");
    let token = header.strip_prefix("Bearer ").unwrap_or("");
    match state.auth.session_email(token) {
        Some(email) if email == state.admin_email => Ok(()),
        _ => Err(poem::Error::from_string("admin login required", poem::http::StatusCode::UNAUTHORIZED)),
    }
}

/// リクエストの`Authorization: Bearer`ヘッダからログイン中のメール
/// アドレスを取得する(未ログインなら`None`、管理者・一般アカウント
/// いずれも区別しない)。
fn session_email(req: &Request, state: &AppState) -> Option<String> {
    let header = req.header(poem::http::header::AUTHORIZATION).unwrap_or("");
    let token = header.strip_prefix("Bearer ").unwrap_or("");
    state.auth.session_email(token)
}

/// グローバル管理者、または`project_id`に対する
/// `access::Need::ManageMembers`許可を持つアカウントのみを通す
/// (ロール権限管理の細分化、2026-07-27追加——Redmine本家の
/// 「プロジェクトマネージャーロール」相当、`decide_access_request`で使用)。
/// `project_id`が指定されていない申請(プロジェクト非紐付けのアカウント
/// 登録のみの申請)はスコープを判定できないため、管理者のみに限定する。
/// 成功時は実際に許可されたアカウントのメールアドレスを返す(呼び出し側が
/// 「このメールアドレスはグローバル管理者本人か」を追加判定できるように
/// するため)。
async fn require_admin_or_project_manager(req: &Request, state: &AppState, project_id: Option<u64>) -> PoemResult<String> {
    let email = session_email(req, state);
    if let Some(email) = &email {
        if *email == state.admin_email {
            return Ok(email.clone());
        }
        if let Some(pid) = project_id {
            let config = access::load(&state.data_root, pid, state.backend.as_ref()).await;
            if access::is_allowed(&config, access::Need::ManageMembers, Some(email.as_str())) {
                return Ok(email.clone());
            }
        }
    }
    match email {
        Some(_) => Err(poem::Error::from_string("insufficient permission", poem::http::StatusCode::FORBIDDEN)),
        None => Err(poem::Error::from_string("login required", poem::http::StatusCode::UNAUTHORIZED)),
    }
}

/// チケットが所属する`project`に対して`need`の操作が許可されているかを
/// 判定する(`access.rs`の`is_allowed`を利用)。管理者は常に許可。
/// 未ログインは`401`、ログイン済みだが権限不足は`403`
/// (`RGit`と同じ401/403の使い分け)。
async fn check_project_access(req: &Request, state: &AppState, project_id: u64, need: access::Need) -> PoemResult<()> {
    let email = session_email(req, state);
    if let Some(email) = &email {
        if *email == state.admin_email {
            return Ok(());
        }
    }
    let config = access::load(&state.data_root, project_id, state.backend.as_ref()).await;
    if access::is_allowed(&config, need, email.as_deref()) {
        return Ok(());
    }
    if email.is_none() {
        Err(poem::Error::from_string("login required", poem::http::StatusCode::UNAUTHORIZED))
    } else {
        Err(poem::Error::from_string("insufficient permission", poem::http::StatusCode::FORBIDDEN))
    }
}

#[derive(Deserialize)]
struct CreateProjectRequest {
    name: String,
    #[serde(default)]
    description: String,
    /// 親プロジェクトの`id`(サブプロジェクト階層、管理者のみ設定可能——
    /// 他のプロジェクト操作と同じ「管理者のみが構造を作れる」方針)。
    #[serde(default)]
    parent_id: Option<u64>,
    /// このプロジェクト配下のチケットが持てるカスタムフィールド名一覧
    /// (2026-07-31追加)。
    #[serde(default)]
    custom_field_defs: Vec<String>,
}

/// `POST /api/projects` — プロジェクトを新規作成する(管理者のみ、
/// `RGit`/`access.rs`と同じ「管理者のみが構造を作れる」方針)。
/// `parent_id`を指定する場合は実在するプロジェクトである必要がある
/// (新規作成時点では循環は起こり得ないため、循環チェックは`update_project`
/// のみで行う)。
#[handler]
async fn create_project(req: &Request, state: Data<&AppState>, body: poem::web::Json<CreateProjectRequest>) -> PoemResult<Response> {
    require_admin_session(req, &state)?;
    if body.name.trim().is_empty() {
        return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("name must not be empty"));
    }
    let mut store = project::load(&state.data_root, state.backend.as_ref()).await;
    if let Some(parent_id) = body.parent_id {
        if !store.exists(parent_id) {
            return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("parent_id does not refer to an existing project"));
        }
    }
    let id = store.next_id;
    store.next_id += 1;
    let now = project::now_rfc3339();
    let proj = project::Project {
        id,
        name: body.name.clone(),
        description: body.description.clone(),
        parent_id: body.parent_id,
        custom_field_defs: body.custom_field_defs.clone(),
        created_at: now.clone(),
        updated_at: now,
    };
    store.projects.push(proj.clone());
    project::save(&state.data_root, &store, state.backend.as_ref())
        .await
        .map_err(|e| poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;
    Ok(Response::builder()
        .status(poem::http::StatusCode::CREATED)
        .content_type("application/json")
        .body(serde_json::to_vec(&proj).unwrap_or_default()))
}

/// `GET /api/projects` — プロジェクト一覧(全ユーザーに公開、
/// プロジェクト自体の存在は隠す情報ではないという方針。チケットの
/// 中身は`access.rs`のアクセス制御で個別に守られる)。
#[handler]
async fn list_projects(state: Data<&AppState>) -> PoemResult<Response> {
    let store = project::load(&state.data_root, state.backend.as_ref()).await;
    Ok(Response::builder().status(poem::http::StatusCode::OK).content_type("application/json").body(serde_json::to_vec(&store.projects).unwrap_or_default()))
}

/// `GET /api/projects/:id` — プロジェクト詳細。
#[handler]
async fn get_project(PathExtractor(id): PathExtractor<u64>, state: Data<&AppState>) -> PoemResult<Response> {
    let store = project::load(&state.data_root, state.backend.as_ref()).await;
    match store.find(id) {
        Some(proj) => Ok(Response::builder().status(poem::http::StatusCode::OK).content_type("application/json").body(serde_json::to_vec(proj).unwrap_or_default())),
        None => Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("project not found")),
    }
}

#[derive(Deserialize)]
struct UpdateProjectRequest {
    name: Option<String>,
    description: Option<String>,
    /// `Some(Some(id))`で親を設定、`Some(None)`で親を解除(トップレベル化)、
    /// フィールド自体が省略された場合(`None`)は変更しない——`serde`の
    /// 二重`Option`パターン(既存コードに前例が無いため今回導入)。
    #[serde(default, deserialize_with = "deserialize_double_option")]
    parent_id: Option<Option<u64>>,
    /// カスタムフィールド定義の置き換え(指定した場合のみ、`custom_fields`
    /// を利用中のチケット側の既存値は削除しない——定義から外れたキーが
    /// 残っても新規作成/更新時にバリデーションで弾かれるだけで、既存の
    /// 値自体は保持される)。
    #[serde(default)]
    custom_field_defs: Option<Vec<String>>,
}

/// 二重`Option`のデシリアライズ補助: フィールド省略は`None`
/// (変更なし)、`null`は`Some(None)`(親解除)、値ありは`Some(Some(v))`。
fn deserialize_double_option<'de, D>(deserializer: D) -> Result<Option<Option<u64>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::deserialize(deserializer)?))
}

/// `PUT /api/projects/:id` — プロジェクトの名前・説明・親(サブプロジェクト
/// 階層)を更新する(管理者のみ)。`parent_id`の変更は循環参照
/// (自分自身や自分の子孫を親に設定すること)を`ProjectStore::would_create_cycle`
/// で検出し、`400`で拒否する。
#[handler]
async fn update_project(
    req: &Request,
    PathExtractor(id): PathExtractor<u64>,
    state: Data<&AppState>,
    body: poem::web::Json<UpdateProjectRequest>,
) -> PoemResult<Response> {
    require_admin_session(req, &state)?;
    let mut store = project::load(&state.data_root, state.backend.as_ref()).await;
    if !store.exists(id) {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("project not found"));
    }
    if let Some(new_parent) = body.parent_id {
        if let Some(parent_id) = new_parent {
            if !store.exists(parent_id) {
                return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("parent_id does not refer to an existing project"));
            }
            if store.would_create_cycle(id, parent_id) {
                return Ok(Response::builder()
                    .status(poem::http::StatusCode::BAD_REQUEST)
                    .body("parent_id would create a cycle (a project cannot be its own ancestor)"));
            }
        }
    }
    let Some(proj) = store.projects.iter_mut().find(|p| p.id == id) else {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("project not found"));
    };
    if let Some(name) = &body.name {
        proj.name = name.clone();
    }
    if let Some(description) = &body.description {
        proj.description = description.clone();
    }
    if let Some(new_parent) = body.parent_id {
        proj.parent_id = new_parent;
    }
    if let Some(defs) = &body.custom_field_defs {
        proj.custom_field_defs = defs.clone();
    }
    proj.updated_at = project::now_rfc3339();
    let updated = proj.clone();
    project::save(&state.data_root, &store, state.backend.as_ref())
        .await
        .map_err(|e| poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;
    Ok(Response::builder().status(poem::http::StatusCode::OK).content_type("application/json").body(serde_json::to_vec(&updated).unwrap_or_default()))
}

/// `GET /api/projects/:id/children` — 直接の子プロジェクト一覧
/// (孫以降は含まない、`parent_id == :id`のプロジェクトのみ)。
/// 認証不要(`list_projects`/`get_project`と同じ「存在自体は隠さない」方針)。
#[handler]
async fn list_project_children(PathExtractor(id): PathExtractor<u64>, state: Data<&AppState>) -> PoemResult<Response> {
    let store = project::load(&state.data_root, state.backend.as_ref()).await;
    if !store.exists(id) {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("project not found"));
    }
    let children: Vec<&project::Project> = store.children_of(id);
    Ok(Response::builder().status(poem::http::StatusCode::OK).content_type("application/json").body(serde_json::to_vec(&children).unwrap_or_default()))
}

/// `DELETE /api/projects/:id` — プロジェクトを削除する(管理者のみ)。
/// このプロジェクトを参照しているチケットが残っていても削除自体は
/// 妨げない(参照側`ticket.project_id`が指す先が無くなるだけで、
/// チケット一覧・詳細は引き続き既存の`project_id`のまま返る——将来的に
/// 「カスケード削除」や「参照防止」を検討する余地がある正直な開示)。
#[handler]
async fn delete_project(req: &Request, PathExtractor(id): PathExtractor<u64>, state: Data<&AppState>) -> PoemResult<Response> {
    require_admin_session(req, &state)?;
    let mut store = project::load(&state.data_root, state.backend.as_ref()).await;
    let before = store.projects.len();
    store.projects.retain(|p| p.id != id);
    if store.projects.len() == before {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("project not found"));
    }
    project::save(&state.data_root, &store, state.backend.as_ref())
        .await
        .map_err(|e| poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;
    Ok(Response::builder().status(poem::http::StatusCode::OK).body("deleted"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TicketStatus {
    Open,
    InProgress,
    /// 作業自体は完了したが、報告者による確認前(Redmine本家の
    /// 「解決」相当、2026-07-31追加)。`Closed`とは区別し、確認後に
    /// 改めて`Closed`へ遷移させる運用を想定する。
    Resolved,
    Closed,
}

/// チケットの種別(Redmineの「トラッカー」相当、Bug/Feature/Support/Task
/// の固定4種にスコープを絞る——Redmine本家はプロジェクト単位でトラッカー
/// 自体を管理者が自由に追加・削除できるが、その管理画面までは今回は
/// 対象外。既存チケットは`#[serde(default)]`で`Bug`扱いとして後方互換を保つ)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Tracker {
    Bug,
    Feature,
    Support,
    Task,
}

impl Default for Tracker {
    fn default() -> Self {
        Tracker::Bug
    }
}

/// チケットの優先度(Redmine本家と同じ5段階、2026-07-31追加)。既存
/// チケットは`#[serde(default)]`で`Normal`扱いとして後方互換を保つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Priority {
    Low,
    Normal,
    High,
    Urgent,
    Immediate,
}

impl Default for Priority {
    fn default() -> Self {
        Priority::Normal
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Ticket {
    id: u64,
    title: String,
    description: String,
    status: TicketStatus,
    /// チケット種別(Redmine機能ギャップ対応、2026-07-26追加)。
    #[serde(default)]
    tracker: Tracker,
    /// 優先度(Redmine機能ギャップ対応、2026-07-31追加)。
    #[serde(default)]
    priority: Priority,
    /// チケットが所属する`Project`の`id`(実体を持つ`project.rs`の
    /// `Project`エンティティを参照、旧`project: String`+ハッシュの
    /// 置き換え——CLAUDE.md HANDOFF「(3) Project自体のCRUD」対応)。
    project_id: u64,
    /// ガントチャート・カレンダー用フィールド(Redmine機能ギャップ対応、
    /// 2026-07-23追加)。日付は`YYYY-MM-DD`形式の文字列で保持し、
    /// パース・タイムゾーン変換は行わない(既存の`created_at`等と同じ
    /// 単純な文字列保持パターンを踏襲)。
    #[serde(default)]
    start_date: Option<String>,
    #[serde(default)]
    due_date: Option<String>,
    /// 進捗率(0-100)。範囲外の値はハンドラ側で`400`として拒否する。
    #[serde(default)]
    done_ratio: u8,
    /// 担当者(Redmine機能ギャップ対応、2026-07-27追加)。登録済みメール
    /// アドレス(`accounts::AccountStore::emails`)または管理者メール
    /// アドレスのいずれかでなければならず、ハンドラ側で`400`として拒否
    /// する(`assignee_email_is_valid`参照)。プロジェクトメンバーシップ
    /// という概念自体はまだ存在しない(`project.rs`参照)ため、
    /// 「登録済みアカウントかどうか」までを検証範囲とする——正直な開示。
    #[serde(default)]
    assignee: Option<String>,
    /// カスタムフィールド(Redmine機能ギャップ対応、2026-07-31追加)。
    /// キーは所属プロジェクトの`Project::custom_field_defs`に含まれる
    /// フィールド名でなければならない(ハンドラ側で`400`拒否)。値は
    /// 自由入力の文字列のみ(Redmine本家のような型指定〈数値/真偽値/
    /// リスト/日付〉別バリデーションは対象外、正直な開示)。
    #[serde(default)]
    custom_fields: std::collections::HashMap<String, String>,
    /// 作成日時・更新日時(Redmine本家のチケット一覧「更新日」列相当、
    /// 2026-07-31追加)。`project::now_rfc3339()`と同じ形式で保持する。
    /// 既存チケットは`#[serde(default)]`で空文字列扱いとして後方互換を
    /// 保つ(表示側は空文字列を`-`として扱う)。
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct TicketStore {
    next_id: u64,
    tickets: Vec<Ticket>,
}

fn tickets_path(data_root: &std::path::Path) -> PathBuf {
    data_root.join("tickets.json")
}

async fn load_tickets(data_root: &std::path::Path) -> TicketStore {
    match tokio::fs::read(tickets_path(data_root)).await {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => TicketStore::default(),
    }
}

async fn save_tickets(data_root: &std::path::Path, store: &TicketStore) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(store).expect("TicketStore serialization is infallible");
    tokio::fs::write(tickets_path(data_root), bytes).await
}

#[derive(Deserialize)]
struct CreateTicketRequest {
    title: String,
    description: String,
    /// 所属`Project`の`id`(実在確認は`create_ticket`内で行う)。
    project_id: u64,
    #[serde(default)]
    tracker: Option<Tracker>,
    #[serde(default)]
    priority: Option<Priority>,
    #[serde(default)]
    start_date: Option<String>,
    #[serde(default)]
    due_date: Option<String>,
    #[serde(default)]
    done_ratio: Option<u8>,
    #[serde(default)]
    assignee: Option<String>,
    /// カスタムフィールドの値(キーは所属プロジェクトの
    /// `custom_field_defs`に含まれていなければならない、2026-07-31追加)。
    #[serde(default)]
    custom_fields: std::collections::HashMap<String, String>,
}

fn done_ratio_out_of_range(ratio: u8) -> bool {
    ratio > 100
}

/// `email`が担当者として指定可能かどうか(管理者メールアドレス、または
/// `accounts::AccountStore`に登録済みのメールアドレスのいずれか)。
fn assignee_email_is_valid(email: &str, accounts: &accounts::AccountStore, admin_email: &str) -> bool {
    email == admin_email || accounts.emails.contains(email)
}

/// `fields`の全キーが`allowed`(プロジェクトの`custom_field_defs`)に
/// 含まれているかを確認する。未定義のキーが1つでもあれば`false`
/// (呼び出し側で`400`として拒否する)。
fn custom_fields_are_defined(fields: &std::collections::HashMap<String, String>, allowed: &[String]) -> bool {
    fields.keys().all(|k| allowed.iter().any(|a| a == k))
}

/// `POST /api/tickets` — チケットを新規作成する。所属`project_id`への
/// `Need::Edit`権限が必要(管理者は常に許可、`access.rs`参照)。
/// `project_id`が実在しない場合は`400`で拒否する。
#[handler]
async fn create_ticket(req: &Request, state: Data<&AppState>, body: poem::web::Json<CreateTicketRequest>) -> PoemResult<Response> {
    let projects = project::load(&state.data_root, state.backend.as_ref()).await;
    let Some(project) = projects.find(body.project_id) else {
        return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("project_id does not refer to an existing project"));
    };
    if !custom_fields_are_defined(&body.custom_fields, &project.custom_field_defs) {
        return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("custom_fields contains a key not defined on the project (see Project.custom_field_defs)"));
    }
    check_project_access(req, &state, body.project_id, access::Need::Edit).await?;
    if body.title.trim().is_empty() {
        return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("title must not be empty"));
    }
    let done_ratio = body.done_ratio.unwrap_or(0);
    if done_ratio_out_of_range(done_ratio) {
        return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("done_ratio must be between 0 and 100"));
    }
    if let Some(assignee) = &body.assignee {
        let accounts = accounts::load(&state.data_root, state.backend.as_ref()).await;
        if !assignee_email_is_valid(assignee, &accounts, &state.admin_email) {
            return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("assignee must be a registered account email"));
        }
    }
    let mut store = load_tickets(&state.data_root).await;
    let id = store.next_id;
    store.next_id += 1;
    let now = project::now_rfc3339();
    let ticket = Ticket {
        id,
        title: body.title.clone(),
        description: body.description.clone(),
        status: TicketStatus::Open,
        tracker: body.tracker.unwrap_or_default(),
        priority: body.priority.unwrap_or_default(),
        project_id: body.project_id,
        start_date: body.start_date.clone(),
        due_date: body.due_date.clone(),
        done_ratio,
        assignee: body.assignee.clone(),
        custom_fields: body.custom_fields.clone(),
        created_at: now.clone(),
        updated_at: now,
    };
    store.tickets.push(ticket.clone());
    save_tickets(&state.data_root, &store)
        .await
        .map_err(|e| poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;
    Ok(Response::builder()
        .status(poem::http::StatusCode::CREATED)
        .content_type("application/json")
        .body(serde_json::to_vec(&ticket).unwrap_or_default()))
}

/// `GET /api/tickets` — チケット一覧。各チケットは所属`project`への
/// `Need::View`権限がある場合のみ結果に含める(管理者は全件、
/// 未ログインは基本的に空配列——`RGit`と同じprivate既定の考え方)。
/// クエリパラメータ`status`(`open`/`in_progress`/`closed`)・
/// `project_id`(数値)・`tracker`(`bug`/`feature`/`support`/`task`)・
/// `assignee`(メールアドレス完全一致)で絞り込み可能(Redmine機能ギャップ
/// 対応、2026-07-23追加。`assignee`は2026-07-27にチケットへ担当者
/// フィールドを追加した際にあわせて対応)。
/// クエリ文字列を自前でパースする(`url`クレートへの新規依存を避けるため、
/// 値に`%XX`エンコードが必要な入力〈`status`/`project_id`は英数字のみ
/// 想定〉は今回サポートしない——正直な開示)。
fn parse_query_string(req: &Request) -> std::collections::HashMap<String, String> {
    req.uri()
        .query()
        .map(|q| {
            q.split('&')
                .filter_map(|pair| {
                    let mut it = pair.splitn(2, '=');
                    let key = it.next()?;
                    let value = it.next().unwrap_or("");
                    Some((key.to_string(), value.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_ticket_status(s: &str) -> Option<TicketStatus> {
    match s {
        "open" => Some(TicketStatus::Open),
        "in_progress" => Some(TicketStatus::InProgress),
        "resolved" => Some(TicketStatus::Resolved),
        "closed" => Some(TicketStatus::Closed),
        _ => None,
    }
}

fn parse_tracker(s: &str) -> Option<Tracker> {
    match s {
        "bug" => Some(Tracker::Bug),
        "feature" => Some(Tracker::Feature),
        "support" => Some(Tracker::Support),
        "task" => Some(Tracker::Task),
        _ => None,
    }
}

/// `list_tickets`(クエリパラメータ経由)と`run_saved_query`(保存済み
/// フィルタ経由)の両方から共有される絞り込み+アクセス制御ロジック
/// (2026-07-31、保存済みクエリ機能追加時に`list_tickets`から抽出)。
async fn filter_visible_tickets(
    state: &AppState,
    email: Option<&str>,
    is_admin: bool,
    status_filter: Option<TicketStatus>,
    project_filter: Option<u64>,
    tracker_filter: Option<Tracker>,
    assignee_filter: Option<&str>,
) -> Vec<Ticket> {
    let store = load_tickets(&state.data_root).await;
    let mut visible = Vec::new();
    for ticket in &store.tickets {
        if let Some(want) = &status_filter {
            if want != &ticket.status {
                continue;
            }
        }
        if let Some(pid) = project_filter {
            if ticket.project_id != pid {
                continue;
            }
        }
        if let Some(want) = tracker_filter {
            if ticket.tracker != want {
                continue;
            }
        }
        if let Some(want) = assignee_filter {
            if ticket.assignee.as_deref() != Some(want) {
                continue;
            }
        }
        if is_admin {
            visible.push(ticket.clone());
            continue;
        }
        let config = access::load(&state.data_root, ticket.project_id, state.backend.as_ref()).await;
        if access::is_allowed(&config, access::Need::View, email) {
            visible.push(ticket.clone());
        }
    }
    visible
}

/// `GET /api/tickets` — チケット一覧。各チケットは所属`project`への
/// `Need::View`権限がある場合のみ結果に含める(管理者は全件、
/// 未ログインは基本的に空配列——`RGit`と同じprivate既定の考え方)。
/// クエリパラメータ`status`(`open`/`in_progress`/`closed`)・
/// `project_id`(数値)・`tracker`(`bug`/`feature`/`support`/`task`)・
/// `assignee`(メールアドレス完全一致)で絞り込み可能(Redmine機能ギャップ
/// 対応、2026-07-23追加。`assignee`は2026-07-27にチケットへ担当者
/// フィールドを追加した際にあわせて対応)。
#[handler]
async fn list_tickets(req: &Request, state: Data<&AppState>) -> PoemResult<Response> {
    let email = session_email(req, &state);
    let is_admin = email.as_deref() == Some(state.admin_email.as_str());
    let query = parse_query_string(req);
    let status_filter = query.get("status").and_then(|s| parse_ticket_status(s));
    let project_filter: Option<u64> = query.get("project_id").and_then(|s| s.parse().ok());
    let tracker_filter = query.get("tracker").and_then(|s| parse_tracker(s));
    let assignee_filter: Option<&str> = query.get("assignee").map(|s| s.as_str());
    let visible = filter_visible_tickets(&state, email.as_deref(), is_admin, status_filter, project_filter, tracker_filter, assignee_filter).await;
    Ok(Response::builder().status(poem::http::StatusCode::OK).content_type("application/json").body(serde_json::to_vec(&visible).unwrap_or_default()))
}

#[handler]
async fn get_ticket(req: &Request, PathExtractor(id): PathExtractor<u64>, state: Data<&AppState>) -> PoemResult<Response> {
    let store = load_tickets(&state.data_root).await;
    match store.tickets.iter().find(|t| t.id == id) {
        Some(ticket) => {
            check_project_access(req, &state, ticket.project_id, access::Need::View).await?;
            Ok(Response::builder()
                .status(poem::http::StatusCode::OK)
                .content_type("application/json")
                .body(serde_json::to_vec(ticket).unwrap_or_default()))
        }
        None => Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("ticket not found")),
    }
}

#[derive(Deserialize)]
struct UpdateTicketRequest {
    title: Option<String>,
    description: Option<String>,
    status: Option<TicketStatus>,
    #[serde(default)]
    tracker: Option<Tracker>,
    #[serde(default)]
    priority: Option<Priority>,
    #[serde(default)]
    start_date: Option<String>,
    #[serde(default)]
    due_date: Option<String>,
    #[serde(default)]
    done_ratio: Option<u8>,
    #[serde(default)]
    assignee: Option<String>,
    /// カスタムフィールドの値を置き換える(指定した場合のみ、キーは
    /// 所属プロジェクトの`custom_field_defs`に含まれていなければ`400`)。
    #[serde(default)]
    custom_fields: Option<std::collections::HashMap<String, String>>,
}

/// `PUT /api/tickets/:id` — チケットのタイトル・説明・ステータスを更新する
/// (所属`project`への`Need::Edit`権限が必要、指定したフィールドのみ更新)。
#[handler]
async fn update_ticket(
    req: &Request,
    PathExtractor(id): PathExtractor<u64>,
    state: Data<&AppState>,
    body: poem::web::Json<UpdateTicketRequest>,
) -> PoemResult<Response> {
    let store_preview = load_tickets(&state.data_root).await;
    let Some(existing) = store_preview.tickets.iter().find(|t| t.id == id) else {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("ticket not found"));
    };
    check_project_access(req, &state, existing.project_id, access::Need::Edit).await?;
    if let Some(assignee) = &body.assignee {
        let accounts = accounts::load(&state.data_root, state.backend.as_ref()).await;
        if !assignee_email_is_valid(assignee, &accounts, &state.admin_email) {
            return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("assignee must be a registered account email"));
        }
    }
    if let Some(fields) = &body.custom_fields {
        let projects = project::load(&state.data_root, state.backend.as_ref()).await;
        let allowed = projects.find(existing.project_id).map(|p| p.custom_field_defs.clone()).unwrap_or_default();
        if !custom_fields_are_defined(fields, &allowed) {
            return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("custom_fields contains a key not defined on the project (see Project.custom_field_defs)"));
        }
    }
    let mut store = store_preview;
    let Some(ticket) = store.tickets.iter_mut().find(|t| t.id == id) else {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("ticket not found"));
    };
    if let Some(title) = &body.title {
        ticket.title = title.clone();
    }
    if let Some(description) = &body.description {
        ticket.description = description.clone();
    }
    if let Some(status) = &body.status {
        ticket.status = status.clone();
    }
    if let Some(tracker) = body.tracker {
        ticket.tracker = tracker;
    }
    if let Some(priority) = body.priority {
        ticket.priority = priority;
    }
    if let Some(done_ratio) = body.done_ratio {
        if done_ratio_out_of_range(done_ratio) {
            return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("done_ratio must be between 0 and 100"));
        }
        ticket.done_ratio = done_ratio;
    }
    if let Some(start_date) = &body.start_date {
        ticket.start_date = Some(start_date.clone());
    }
    if let Some(due_date) = &body.due_date {
        ticket.due_date = Some(due_date.clone());
    }
    if let Some(assignee) = &body.assignee {
        ticket.assignee = Some(assignee.clone());
    }
    if let Some(fields) = &body.custom_fields {
        ticket.custom_fields = fields.clone();
    }
    ticket.updated_at = project::now_rfc3339();
    let updated = ticket.clone();
    save_tickets(&state.data_root, &store)
        .await
        .map_err(|e| poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;
    Ok(Response::builder()
        .status(poem::http::StatusCode::OK)
        .content_type("application/json")
        .body(serde_json::to_vec(&updated).unwrap_or_default()))
}

#[derive(Deserialize)]
struct CreateCommentRequest {
    body: String,
}

/// `POST /api/tickets/:id/comments` — チケットへコメントを投稿する。
/// 対象チケットが所属するプロジェクトへの`Need::Edit`権限が必要
/// (既存の`update_ticket`と同じチェックを再利用、モデレーションキューは
/// 不要——投稿時点で権限確認済みのため)。投稿者は認証済みアカウントの
/// メールアドレス(未ログインは401)。
#[handler]
async fn create_comment(
    req: &Request,
    PathExtractor(ticket_id): PathExtractor<u64>,
    state: Data<&AppState>,
    body: poem::web::Json<CreateCommentRequest>,
) -> PoemResult<Response> {
    let tickets = load_tickets(&state.data_root).await;
    let Some(ticket) = tickets.tickets.iter().find(|t| t.id == ticket_id) else {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("ticket not found"));
    };
    check_project_access(req, &state, ticket.project_id, access::Need::Edit).await?;
    let Some(author_email) = session_email(req, &state) else {
        return Err(poem::Error::from_string("login required", poem::http::StatusCode::UNAUTHORIZED));
    };
    if body.body.trim().is_empty() {
        return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("body must not be empty"));
    }
    let mut store = comments::load(&state.data_root, state.backend.as_ref()).await;
    let id = store.next_id;
    store.next_id += 1;
    let comment = comments::Comment { id, ticket_id, author_email, body: body.body.clone(), created_at: project::now_rfc3339() };
    store.comments.push(comment.clone());
    comments::save(&state.data_root, &store, state.backend.as_ref())
        .await
        .map_err(|e| poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;
    Ok(Response::builder()
        .status(poem::http::StatusCode::CREATED)
        .content_type("application/json")
        .body(serde_json::to_vec(&comment).unwrap_or_default()))
}

/// `GET /api/tickets/:id/comments` — チケットへのコメント一覧。対象チケット
/// が所属するプロジェクトへの`Need::View`権限が必要(チケット自体の閲覧と
/// 同じチェック)。
#[handler]
async fn list_comments(req: &Request, PathExtractor(ticket_id): PathExtractor<u64>, state: Data<&AppState>) -> PoemResult<Response> {
    let tickets = load_tickets(&state.data_root).await;
    let Some(ticket) = tickets.tickets.iter().find(|t| t.id == ticket_id) else {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("ticket not found"));
    };
    check_project_access(req, &state, ticket.project_id, access::Need::View).await?;
    let store = comments::load(&state.data_root, state.backend.as_ref()).await;
    let visible: Vec<&comments::Comment> = store.for_ticket(ticket_id);
    Ok(Response::builder().status(poem::http::StatusCode::OK).content_type("application/json").body(serde_json::to_vec(&visible).unwrap_or_default()))
}

/// `DELETE /api/comments/:id` — コメントを削除する。管理者、またはコメントの
/// 投稿者本人のみ許可(`RGit`のオーナー/管理者どちらでも削除可、という
/// 前例パターンに準拠)。
#[handler]
async fn delete_comment(req: &Request, PathExtractor(id): PathExtractor<u64>, state: Data<&AppState>) -> PoemResult<Response> {
    let Some(email) = session_email(req, &state) else {
        return Err(poem::Error::from_string("login required", poem::http::StatusCode::UNAUTHORIZED));
    };
    let mut store = comments::load(&state.data_root, state.backend.as_ref()).await;
    let Some(comment) = store.find(id) else {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("comment not found"));
    };
    let is_admin = email == state.admin_email;
    let is_author = comment.author_email == email;
    if !is_admin && !is_author {
        return Err(poem::Error::from_string("only the comment author or an admin may delete this comment", poem::http::StatusCode::FORBIDDEN));
    }
    store.comments.retain(|c| c.id != id);
    comments::save(&state.data_root, &store, state.backend.as_ref())
        .await
        .map_err(|e| poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;
    Ok(Response::builder().status(poem::http::StatusCode::OK).body("deleted"))
}

/// `POST /api/tickets/:id/attachments` — チケットへファイルを添付する。
/// `multipart/form-data`で単一パート(フィールド名は問わない、最初に
/// ファイル名を持つパートを採用)を受け取る。アクセス制御は
/// `create_comment`と同じ(対象チケットが所属するプロジェクトへの
/// `Need::Edit`権限が必要)。
#[handler]
async fn create_attachment(
    req: &Request,
    PathExtractor(ticket_id): PathExtractor<u64>,
    state: Data<&AppState>,
    mut multipart: poem::web::Multipart,
) -> PoemResult<Response> {
    let tickets = load_tickets(&state.data_root).await;
    let Some(ticket) = tickets.tickets.iter().find(|t| t.id == ticket_id) else {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("ticket not found"));
    };
    check_project_access(req, &state, ticket.project_id, access::Need::Edit).await?;
    let Some(author_email) = session_email(req, &state) else {
        return Err(poem::Error::from_string("login required", poem::http::StatusCode::UNAUTHORIZED));
    };

    let mut field = None;
    while let Some(f) = multipart
        .next_field()
        .await
        .map_err(|e| poem::Error::from_string(format!("invalid multipart body: {e}"), poem::http::StatusCode::BAD_REQUEST))?
    {
        if f.file_name().is_some() {
            field = Some(f);
            break;
        }
    }
    let Some(field) = field else {
        return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("no file part found in multipart body"));
    };
    let file_name = field.file_name().unwrap_or("file").to_string();
    let content_type = field.content_type().unwrap_or("application/octet-stream").to_string();
    let bytes = field
        .bytes()
        .await
        .map_err(|e| poem::Error::from_string(format!("failed to read file body: {e}"), poem::http::StatusCode::BAD_REQUEST))?;

    let mut store = attachments::load(&state.data_root, state.backend.as_ref()).await;
    let id = store.next_id;
    store.next_id += 1;
    let attachment = attachments::Attachment {
        id,
        ticket_id,
        author_email,
        file_name: file_name.clone(),
        content_type,
        size_bytes: bytes.len() as u64,
        created_at: project::now_rfc3339(),
    };

    let blob_path = attachments::attachment_blob_path(&state.data_root, id, &file_name);
    state
        .backend
        .write(&blob_path.to_string_lossy(), &bytes)
        .await
        .map_err(|e| poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;

    store.attachments.push(attachment.clone());
    attachments::save(&state.data_root, &store, state.backend.as_ref())
        .await
        .map_err(|e| poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;

    Ok(Response::builder()
        .status(poem::http::StatusCode::CREATED)
        .content_type("application/json")
        .body(serde_json::to_vec(&attachment).unwrap_or_default()))
}

/// `GET /api/tickets/:id/attachments` — チケットへの添付ファイル一覧
/// (メタデータのみ、`Need::View`権限があれば閲覧可能)。
#[handler]
async fn list_attachments(req: &Request, PathExtractor(ticket_id): PathExtractor<u64>, state: Data<&AppState>) -> PoemResult<Response> {
    let tickets = load_tickets(&state.data_root).await;
    let Some(ticket) = tickets.tickets.iter().find(|t| t.id == ticket_id) else {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("ticket not found"));
    };
    check_project_access(req, &state, ticket.project_id, access::Need::View).await?;
    let store = attachments::load(&state.data_root, state.backend.as_ref()).await;
    let visible: Vec<&attachments::Attachment> = store.for_ticket(ticket_id);
    Ok(Response::builder().status(poem::http::StatusCode::OK).content_type("application/json").body(serde_json::to_vec(&visible).unwrap_or_default()))
}

/// `GET /api/attachments/:id/download` — 添付ファイルの実体をダウンロード
/// する。対象チケットが所属するプロジェクトへの`Need::View`権限が必要。
#[handler]
async fn download_attachment(req: &Request, PathExtractor(id): PathExtractor<u64>, state: Data<&AppState>) -> PoemResult<Response> {
    let store = attachments::load(&state.data_root, state.backend.as_ref()).await;
    let Some(attachment) = store.find(id) else {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("attachment not found"));
    };
    check_project_access(req, &state, {
        let tickets = load_tickets(&state.data_root).await;
        let Some(ticket) = tickets.tickets.iter().find(|t| t.id == attachment.ticket_id) else {
            return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("ticket not found"));
        };
        ticket.project_id
    }, access::Need::View).await?;

    let blob_path = attachments::attachment_blob_path(&state.data_root, attachment.id, &attachment.file_name);
    let bytes = state
        .backend
        .read(&blob_path.to_string_lossy())
        .await
        .map_err(|e| poem::Error::from_string(format!("failed to read attachment blob: {e}"), poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;

    Ok(Response::builder()
        .status(poem::http::StatusCode::OK)
        .content_type(attachment.content_type.clone())
        .header(
            "content-disposition",
            format!("attachment; filename=\"{}\"", attachment.file_name.replace('"', "'")),
        )
        .body(bytes))
}

/// `DELETE /api/attachments/:id` — 添付ファイルのメタデータを削除する。
/// 管理者、またはアップロードした本人のみ許可(`delete_comment`と同じ
/// 方針)。**正直な開示**: `StorageBackend`にはまだ削除APIが無いため、
/// このパスではメタデータ(一覧・ダウンロード可否)のみ削除し、
/// 保存先(local/sftp/gdrive)上の実ファイルは残り続ける(v1の既知の制約、
/// ディスク容量が問題になった場合に対応する)。
#[handler]
async fn delete_attachment(req: &Request, PathExtractor(id): PathExtractor<u64>, state: Data<&AppState>) -> PoemResult<Response> {
    let Some(email) = session_email(req, &state) else {
        return Err(poem::Error::from_string("login required", poem::http::StatusCode::UNAUTHORIZED));
    };
    let mut store = attachments::load(&state.data_root, state.backend.as_ref()).await;
    let Some(attachment) = store.find(id) else {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("attachment not found"));
    };
    let is_admin = email == state.admin_email;
    let is_author = attachment.author_email == email;
    if !is_admin && !is_author {
        return Err(poem::Error::from_string("only the uploader or an admin may delete this attachment", poem::http::StatusCode::FORBIDDEN));
    }
    store.attachments.retain(|a| a.id != id);
    attachments::save(&state.data_root, &store, state.backend.as_ref())
        .await
        .map_err(|e| poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;
    Ok(Response::builder().status(poem::http::StatusCode::OK).body("deleted"))
}

#[derive(Deserialize)]
struct CreateRelationRequest {
    to_ticket_id: u64,
    kind: relations::RelationKind,
}

/// `POST /api/tickets/:id/relations` — チケット間の関連(ブロック/重複/
/// 先行)を作成する。`from`側チケットが所属するプロジェクトへの
/// `Need::Edit`権限が必要(コメント投稿と同じ権限モデル)。`to_ticket_id`が
/// 実在しない場合・自分自身を指した場合・同じ`(from, to, kind)`の関連が
/// 既に存在する場合はいずれも`400`で拒否する。
#[handler]
async fn create_relation(
    req: &Request,
    PathExtractor(from_id): PathExtractor<u64>,
    state: Data<&AppState>,
    body: poem::web::Json<CreateRelationRequest>,
) -> PoemResult<Response> {
    let tickets = load_tickets(&state.data_root).await;
    let Some(from_ticket) = tickets.tickets.iter().find(|t| t.id == from_id) else {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("ticket not found"));
    };
    check_project_access(req, &state, from_ticket.project_id, access::Need::Edit).await?;
    if body.to_ticket_id == from_id {
        return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("a ticket cannot relate to itself"));
    }
    if !tickets.tickets.iter().any(|t| t.id == body.to_ticket_id) {
        return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("to_ticket_id does not refer to an existing ticket"));
    }
    let mut store = relations::load(&state.data_root, state.backend.as_ref()).await;
    if store.duplicate_exists(from_id, body.to_ticket_id, body.kind) {
        return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("this relation already exists"));
    }
    let id = store.next_id;
    store.next_id += 1;
    let relation = relations::IssueRelation {
        id,
        from_ticket_id: from_id,
        to_ticket_id: body.to_ticket_id,
        kind: body.kind,
        created_at: project::now_rfc3339(),
    };
    store.relations.push(relation.clone());
    relations::save(&state.data_root, &store, state.backend.as_ref())
        .await
        .map_err(|e| poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;
    Ok(Response::builder()
        .status(poem::http::StatusCode::CREATED)
        .content_type("application/json")
        .body(serde_json::to_vec(&relation).unwrap_or_default()))
}

/// `GET /api/tickets/:id/relations` — チケットが関わる関連の一覧
/// (`from`/`to`いずれの立場でも含む)。閲覧には対象チケットが所属する
/// プロジェクトへの`Need::View`権限が必要。
#[handler]
async fn list_relations(req: &Request, PathExtractor(ticket_id): PathExtractor<u64>, state: Data<&AppState>) -> PoemResult<Response> {
    let tickets = load_tickets(&state.data_root).await;
    let Some(ticket) = tickets.tickets.iter().find(|t| t.id == ticket_id) else {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("ticket not found"));
    };
    check_project_access(req, &state, ticket.project_id, access::Need::View).await?;
    let store = relations::load(&state.data_root, state.backend.as_ref()).await;
    let visible: Vec<&relations::IssueRelation> = store.for_ticket(ticket_id);
    Ok(Response::builder().status(poem::http::StatusCode::OK).content_type("application/json").body(serde_json::to_vec(&visible).unwrap_or_default()))
}

/// `DELETE /api/relations/:id` — 関連を削除する。`from`側チケットが所属する
/// プロジェクトへの`Need::Edit`権限が必要(片方向の関係だが、`from`側で
/// 権限判定すれば実用上十分という判断)。
#[handler]
async fn delete_relation(req: &Request, PathExtractor(id): PathExtractor<u64>, state: Data<&AppState>) -> PoemResult<Response> {
    let mut store = relations::load(&state.data_root, state.backend.as_ref()).await;
    let Some(relation) = store.find(id) else {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("relation not found"));
    };
    let tickets = load_tickets(&state.data_root).await;
    let Some(from_ticket) = tickets.tickets.iter().find(|t| t.id == relation.from_ticket_id) else {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("ticket not found"));
    };
    check_project_access(req, &state, from_ticket.project_id, access::Need::Edit).await?;
    store.relations.retain(|r| r.id != id);
    relations::save(&state.data_root, &store, state.backend.as_ref())
        .await
        .map_err(|e| poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;
    Ok(Response::builder().status(poem::http::StatusCode::OK).body("deleted"))
}

#[derive(Deserialize)]
struct CreateTimeEntryRequest {
    hours: f64,
    activity: String,
    #[serde(default)]
    comments: String,
    spent_on: String,
}

/// `POST /api/tickets/:id/time_entries` — チケットへ作業時間を記録する。
/// 対象チケットが所属するプロジェクトへの`Need::Edit`権限が必要
/// (コメント投稿と同じ権限モデル)。`hours`は`0`より大きく`24`以下
/// (1日の作業記録として非現実的な値を弾く実用上の妥当性チェック)。
#[handler]
async fn create_time_entry(
    req: &Request,
    PathExtractor(ticket_id): PathExtractor<u64>,
    state: Data<&AppState>,
    body: poem::web::Json<CreateTimeEntryRequest>,
) -> PoemResult<Response> {
    let Some(email) = session_email(req, &state) else {
        return Err(poem::Error::from_string("login required", poem::http::StatusCode::UNAUTHORIZED));
    };
    let tickets = load_tickets(&state.data_root).await;
    let Some(ticket) = tickets.tickets.iter().find(|t| t.id == ticket_id) else {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("ticket not found"));
    };
    check_project_access(req, &state, ticket.project_id, access::Need::Edit).await?;
    if !(body.hours > 0.0 && body.hours <= 24.0) {
        return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("hours must be greater than 0 and at most 24"));
    }
    if body.activity.trim().is_empty() {
        return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("activity must not be empty"));
    }
    let mut store = time_entries::load(&state.data_root, state.backend.as_ref()).await;
    let id = store.next_id;
    store.next_id += 1;
    let entry = time_entries::TimeEntry {
        id,
        ticket_id,
        author_email: email,
        hours: body.hours,
        activity: body.activity.clone(),
        comments: body.comments.clone(),
        spent_on: body.spent_on.clone(),
        created_at: project::now_rfc3339(),
    };
    store.entries.push(entry.clone());
    time_entries::save(&state.data_root, &store, state.backend.as_ref())
        .await
        .map_err(|e| poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;
    Ok(Response::builder()
        .status(poem::http::StatusCode::CREATED)
        .content_type("application/json")
        .body(serde_json::to_vec(&entry).unwrap_or_default()))
}

/// `GET /api/tickets/:id/time_entries` — チケットの作業時間記録一覧。
/// 対象チケットが所属するプロジェクトへの`Need::View`権限が必要。
#[handler]
async fn list_time_entries(req: &Request, PathExtractor(ticket_id): PathExtractor<u64>, state: Data<&AppState>) -> PoemResult<Response> {
    let tickets = load_tickets(&state.data_root).await;
    let Some(ticket) = tickets.tickets.iter().find(|t| t.id == ticket_id) else {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("ticket not found"));
    };
    check_project_access(req, &state, ticket.project_id, access::Need::View).await?;
    let store = time_entries::load(&state.data_root, state.backend.as_ref()).await;
    let visible: Vec<&time_entries::TimeEntry> = store.for_ticket(ticket_id);
    Ok(Response::builder().status(poem::http::StatusCode::OK).content_type("application/json").body(serde_json::to_vec(&visible).unwrap_or_default()))
}

/// `DELETE /api/time_entries/:id` — 作業時間記録を削除する。管理者、または
/// 記録の投稿者本人のみ許可(コメント削除と同じ権限パターン)。
#[handler]
async fn delete_time_entry(req: &Request, PathExtractor(id): PathExtractor<u64>, state: Data<&AppState>) -> PoemResult<Response> {
    let Some(email) = session_email(req, &state) else {
        return Err(poem::Error::from_string("login required", poem::http::StatusCode::UNAUTHORIZED));
    };
    let mut store = time_entries::load(&state.data_root, state.backend.as_ref()).await;
    let Some(entry) = store.find(id) else {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("time entry not found"));
    };
    let is_admin = email == state.admin_email;
    let is_author = entry.author_email == email;
    if !is_admin && !is_author {
        return Err(poem::Error::from_string("only the time entry author or an admin may delete this time entry", poem::http::StatusCode::FORBIDDEN));
    }
    store.entries.retain(|e| e.id != id);
    time_entries::save(&state.data_root, &store, state.backend.as_ref())
        .await
        .map_err(|e| poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;
    Ok(Response::builder().status(poem::http::StatusCode::OK).body("deleted"))
}

#[derive(Deserialize)]
struct CreateWikiPageRequest {
    slug: String,
    title: String,
    body: String,
}

/// `POST /api/projects/:id/wiki` — プロジェクト配下に新規Wikiページを
/// 作成する。対象プロジェクトへの`Need::Edit`権限が必要
/// (`create_comment`と同じ権限モデル)。`slug`はプロジェクト内で一意。
#[handler]
async fn create_wiki_page(
    req: &Request,
    PathExtractor(project_id): PathExtractor<u64>,
    state: Data<&AppState>,
    body: poem::web::Json<CreateWikiPageRequest>,
) -> PoemResult<Response> {
    let projects = project::load(&state.data_root, state.backend.as_ref()).await;
    if !projects.exists(project_id) {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("project not found"));
    }
    check_project_access(req, &state, project_id, access::Need::Edit).await?;
    let Some(author_email) = session_email(req, &state) else {
        return Err(poem::Error::from_string("login required", poem::http::StatusCode::UNAUTHORIZED));
    };
    if body.slug.trim().is_empty() || body.title.trim().is_empty() || body.body.trim().is_empty() {
        return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("slug, title and body must not be empty"));
    }
    let mut store = wiki::load(&state.data_root, state.backend.as_ref()).await;
    if store.slug_taken(project_id, &body.slug) {
        return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("slug already in use for this project"));
    }
    let id = store.next_id;
    store.next_id += 1;
    let page = wiki::WikiPage {
        id,
        project_id,
        slug: body.slug.clone(),
        title: body.title.clone(),
        revisions: vec![wiki::WikiRevision { body: body.body.clone(), author_email, created_at: project::now_rfc3339() }],
    };
    store.pages.push(page.clone());
    wiki::save(&state.data_root, &store, state.backend.as_ref())
        .await
        .map_err(|e| poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;
    Ok(Response::builder()
        .status(poem::http::StatusCode::CREATED)
        .content_type("application/json")
        .body(serde_json::to_vec(&page).unwrap_or_default()))
}

/// `GET /api/projects/:id/wiki` — プロジェクト配下のWikiページ一覧
/// (対象プロジェクトへの`Need::View`権限が必要)。
#[handler]
async fn list_wiki_pages(req: &Request, PathExtractor(project_id): PathExtractor<u64>, state: Data<&AppState>) -> PoemResult<Response> {
    let projects = project::load(&state.data_root, state.backend.as_ref()).await;
    if !projects.exists(project_id) {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("project not found"));
    }
    check_project_access(req, &state, project_id, access::Need::View).await?;
    let store = wiki::load(&state.data_root, state.backend.as_ref()).await;
    let pages = store.for_project(project_id);
    Ok(Response::builder().status(poem::http::StatusCode::OK).content_type("application/json").body(serde_json::to_vec(&pages).unwrap_or_default()))
}

/// `GET /api/wiki/:id` — Wikiページの最新版を取得する(所属プロジェクトへの
/// `Need::View`権限が必要、`get_ticket`と同じ「まず存在確認してから権限
/// チェック」の順序)。
#[handler]
async fn get_wiki_page(req: &Request, PathExtractor(id): PathExtractor<u64>, state: Data<&AppState>) -> PoemResult<Response> {
    let store = wiki::load(&state.data_root, state.backend.as_ref()).await;
    let Some(page) = store.find(id) else {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("wiki page not found"));
    };
    check_project_access(req, &state, page.project_id, access::Need::View).await?;
    Ok(Response::builder().status(poem::http::StatusCode::OK).content_type("application/json").body(serde_json::to_vec(page).unwrap_or_default()))
}

#[derive(Deserialize)]
struct UpdateWikiPageRequest {
    body: String,
    #[serde(default)]
    title: Option<String>,
}

/// `PUT /api/wiki/:id` — 新しいリビジョンを追記する(旧内容は`revisions`に
/// 残したまま、履歴を保持する)。所属プロジェクトへの`Need::Edit`権限が
/// 必要。
#[handler]
async fn update_wiki_page(
    req: &Request,
    PathExtractor(id): PathExtractor<u64>,
    state: Data<&AppState>,
    body: poem::web::Json<UpdateWikiPageRequest>,
) -> PoemResult<Response> {
    let mut store = wiki::load(&state.data_root, state.backend.as_ref()).await;
    let Some(project_id) = store.find(id).map(|p| p.project_id) else {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("wiki page not found"));
    };
    check_project_access(req, &state, project_id, access::Need::Edit).await?;
    let Some(author_email) = session_email(req, &state) else {
        return Err(poem::Error::from_string("login required", poem::http::StatusCode::UNAUTHORIZED));
    };
    if body.body.trim().is_empty() {
        return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("body must not be empty"));
    }
    let page = store.find_mut(id).expect("id was just confirmed to exist in the same store");
    if let Some(title) = &body.title {
        page.title = title.clone();
    }
    page.revisions.push(wiki::WikiRevision { body: body.body.clone(), author_email, created_at: project::now_rfc3339() });
    let updated = page.clone();
    wiki::save(&state.data_root, &store, state.backend.as_ref())
        .await
        .map_err(|e| poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;
    Ok(Response::builder().status(poem::http::StatusCode::OK).content_type("application/json").body(serde_json::to_vec(&updated).unwrap_or_default()))
}

/// `DELETE /api/wiki/:id` — Wikiページを削除する(管理者のみ、
/// `create_project`等と同じ「構造を作れる/壊せるのは管理者のみ」方針)。
#[handler]
async fn delete_wiki_page(req: &Request, PathExtractor(id): PathExtractor<u64>, state: Data<&AppState>) -> PoemResult<Response> {
    require_admin_session(req, &state)?;
    let mut store = wiki::load(&state.data_root, state.backend.as_ref()).await;
    if store.find(id).is_none() {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("wiki page not found"));
    }
    store.pages.retain(|p| p.id != id);
    wiki::save(&state.data_root, &store, state.backend.as_ref())
        .await
        .map_err(|e| poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;
    Ok(Response::builder().status(poem::http::StatusCode::OK).body("deleted"))
}

/// トップページ(`GET /`)のHTMLランディングページ。
/// ブラウザで実インスタンスへアクセスしたユーザーへ、アプリの概要・
/// 実装済みAPI一覧・未実装機能の正直な開示・ダウンロードリンクを示す
/// (JSON APIのみで何も表示されないUXバグの修正、`RGit`の
/// `static/index.html`と同じ趣旨)。
const INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="ja">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>RS-Chiketto</title>
<style>
  body { font-family: system-ui, sans-serif; max-width: 780px; margin: 2rem auto; padding: 0 1rem; line-height: 1.6; color: #222; }
  h1 { margin-bottom: 0; }
  .tagline { color: #666; margin-top: 0.2rem; }
  code { background: #f2f2f2; padding: 0.1rem 0.35rem; border-radius: 3px; }
  table { border-collapse: collapse; width: 100%; margin: 1rem 0; }
  th, td { text-align: left; padding: 0.4rem 0.6rem; border-bottom: 1px solid #ddd; font-size: 0.92rem; }
  .warn { background: #fff8e1; border: 1px solid #ffe08a; border-radius: 6px; padding: 0.8rem 1rem; }
  .btn { display: inline-block; background: #2d6cdf; color: #fff; padding: 0.5rem 1rem; border-radius: 6px; text-decoration: none; margin-right: 0.5rem; }
  footer { color: #888; font-size: 0.85rem; margin-top: 2rem; }
</style>
</head>
<body>
<h1>RS-Chiketto</h1>
<p class="tagline">Redmine相当のチケット(Issue)トラッカー — Rust + poem(RPoem)製、高速・高セキュリティ・省メモリ志向。v0.1.0。</p>

<h2>これは何?</h2>
<p>
  <a href="https://redmine.org/">Redmine</a>のRust版を目指すプロジェクトです。
  v0.1.0時点ではチケット管理とOTPログイン・アクセス制御のみを実装しています。
</p>

<h2>使い方: 現在はJSON APIのみ(ブラウザUIはまだありません)</h2>
<p>このページ以外はすべてJSON APIです。以下のエンドポイントに対して<code>curl</code>や外部クライアントからアクセスしてください。</p>
<table>
<tr><th>メソッド / パス</th><th>説明</th></tr>
<tr><td><code>GET /healthz</code></td><td>ヘルスチェック</td></tr>
<tr><td><code>POST /api/auth/request-otp</code></td><td>ログイン用ワンタイムパスワードをメール送信</td></tr>
<tr><td><code>POST /api/auth/verify-otp</code></td><td>OTPを検証してセッショントークンを発行</td></tr>
<tr><td><code>POST /api/auth/logout</code></td><td>ログアウト(トークン失効)</td></tr>
<tr><td><code>GET /api/accounts</code> / <code>POST /api/accounts</code></td><td>登録アカウント一覧取得 / 追加(管理者のみ)</td></tr>
<tr><td><code>POST /api/accounts/request</code></td><td>アカウント利用の自己申請(認証不要)</td></tr>
<tr><td><code>GET /api/accounts/requests</code></td><td>保留中の自己申請一覧(管理者のみ)</td></tr>
<tr><td><code>POST /api/accounts/requests/:id/decide</code></td><td>自己申請の承認/却下・プロジェクトへの閲覧/編集権限付与(管理者のみ)</td></tr>
<tr><td><code>GET /api/projects</code> / <code>POST /api/projects</code></td><td>プロジェクト一覧取得 / 新規作成(管理者のみ、<code>parent_id</code>でサブプロジェクト化可能)</td></tr>
<tr><td><code>GET /api/projects/:id</code> / <code>PUT /api/projects/:id</code> / <code>DELETE /api/projects/:id</code></td><td>プロジェクト詳細取得 / 更新・削除(管理者のみ、<code>parent_id</code>変更は循環参照を拒否)</td></tr>
<tr><td><code>GET /api/projects/:id/children</code></td><td>直接の子プロジェクト一覧</td></tr>
<tr><td><code>GET /api/tickets</code> / <code>POST /api/tickets</code></td><td>チケット一覧取得(アクセス権のあるプロジェクトのみ) / 新規作成(実在する<code>project_id</code>が必要)</td></tr>
<tr><td><code>GET /api/tickets/:id</code> / <code>PUT /api/tickets/:id</code></td><td>チケット詳細取得 / 更新(ステータス変更含む)</td></tr>
<tr><td><code>GET /api/tickets/:id/comments</code> / <code>POST /api/tickets/:id/comments</code></td><td>コメント一覧取得(閲覧権限が必要) / 投稿(編集権限が必要)</td></tr>
<tr><td><code>DELETE /api/comments/:id</code></td><td>コメント削除(管理者または投稿者本人のみ)</td></tr>
</table>

<div class="warn">
<strong>正直な開示: まだ実装していない機能</strong>
<ul>
<li>ガントチャート・カレンダー</li>
<li>Wiki・フォーラム</li>
<li>リポジトリ連携(SCM閲覧、<a href="https://github.com/aon-co-jp/RGit">RGit</a>との連携は将来検討)</li>
<li>カスタムフィールド・ワークフロー</li>
<li><code>aruaru-db</code>/PostgreSQLへの移行(現状はJSONファイル永続化)</li>
</ul>
</div>

<h2>ダウンロード / インストール</h2>
<p>
  <a class="btn" href="https://github.com/aon-co-jp/RS-Chiketto/releases/latest">最新リリースをダウンロード</a>
  <a class="btn" href="https://github.com/aon-co-jp/RS-Chiketto">GitHubでソースを見る</a>
</p>
<p>Linux(静的リンクmuslバイナリ)・Windows向けにインストーラー付きビルド済みバイナリを配布しています。詳細は<a href="https://github.com/aon-co-jp/RS-Chiketto#readme">README</a>参照。</p>

<footer>RS-Chiketto v0.1.0 &mdash; <a href="https://github.com/aon-co-jp/RS-Chiketto">aon-co-jp/RS-Chiketto</a></footer>
</body>
</html>
"#;

/// `GET /` — ブラウザGUI(`web/index.html`)を配信する。GUIビルド成果物
/// (`web/index.html`+`web/pkg/`)が存在しない環境では、後方互換として
/// 旧来の`INDEX_HTML`(API概要ページ)へフォールバックする(ユーザー
/// 指示「チケット管理を行なうWEBアプリなのでGUIは基本」、2026-07-23)。
#[handler]
async fn index() -> Response {
    match tokio::fs::read_to_string(web_root().join("index.html")).await {
        Ok(html) => Response::builder().status(poem::http::StatusCode::OK).content_type("text/html; charset=utf-8").body(html),
        Err(_) => Response::builder().status(poem::http::StatusCode::OK).content_type("text/html; charset=utf-8").body(INDEX_HTML),
    }
}

fn web_root() -> PathBuf {
    std::env::var("RSCHIKETTO_WEB_DIR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("./web"))
}

/// `GET /pkg/:file` — WASMビルド成果物(`rs_red_web.js`/`rs_red_web_bg.wasm`)
/// を配信する。パストラバーサル対策として、ファイル名に`/`や`..`を
/// 含む場合は拒否する(`open-web-server`の`static_files.rs`と同じ方針)。
#[handler]
async fn serve_pkg(PathExtractor(file): PathExtractor<String>) -> Response {
    if file.contains("..") || file.contains('/') || file.contains('\\') {
        return Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("invalid path");
    }
    let path = web_root().join("pkg").join(&file);
    let content_type = if file.ends_with(".wasm") {
        "application/wasm"
    } else if file.ends_with(".js") {
        "application/javascript"
    } else {
        "application/octet-stream"
    };
    match tokio::fs::read(&path).await {
        Ok(bytes) => Response::builder().status(poem::http::StatusCode::OK).content_type(content_type).body(bytes),
        Err(_) => Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("not found"),
    }
}

#[handler]
async fn healthz() -> &'static str {
    "ok"
}

#[handler]
async fn request_otp(state: Data<&AppState>, body: poem::web::Json<serde_json::Value>) -> PoemResult<Response> {
    let email = body.get("email").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if email != state.admin_email {
        let registered = accounts::load(&state.data_root, state.backend.as_ref()).await;
        if !registered.emails.contains(&email) {
            return Ok(Response::builder().status(poem::http::StatusCode::FORBIDDEN).body("email not registered"));
        }
    }
    // 2026-07-27追記(使いやすさ改善): SMTP未設定の開発・検証環境では
    // 従来503を返すのみで、GUIを一切使い始められなかった(実SMTPサーバーが
    // 無いと結合テストすら組めない、という外部監査で指摘された最重要の
    // セットアップ障壁)。`RSCHIKETTO_DEV_LOG_OTP=true`を明示的に設定した
    // 場合に限り、OTPをメール送信せず`tracing::warn!`でサーバーログへ
    // 出力する開発用バイパスを用意する(既定は従来通りoff、本番誤爆防止)。
    let dev_log_otp = std::env::var("RSCHIKETTO_DEV_LOG_OTP")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);
    if state.smtp.is_none() && !dev_log_otp {
        return Ok(Response::builder().status(poem::http::StatusCode::SERVICE_UNAVAILABLE).body("SMTP not configured"));
    }
    let auth::RequestOtpOutcome::Issued(code) = state.auth.request_otp(&email);
    if state.smtp.is_none() && dev_log_otp {
        tracing::warn!(
            "RSCHIKETTO_DEV_LOG_OTP: SMTP not configured, printing OTP for {email} to server log \
             instead of sending mail (DEV ONLY, never enable this in production): {code}"
        );
        return Ok(Response::builder().status(poem::http::StatusCode::OK).body("otp sent (dev mode: logged to server console, not emailed)"));
    }
    let smtp = state.smtp.clone().expect("checked above");
    match mail::send_otp(smtp, email, code).await {
        Ok(()) => Ok(Response::builder().status(poem::http::StatusCode::OK).body("otp sent")),
        Err(e) => {
            tracing::warn!("failed to send OTP mail: {e}");
            Ok(Response::builder().status(poem::http::StatusCode::BAD_GATEWAY).body("failed to send mail"))
        }
    }
}

#[derive(Deserialize)]
struct VerifyOtpRequest {
    email: String,
    code: String,
}

#[handler]
async fn verify_otp(state: Data<&AppState>, body: poem::web::Json<VerifyOtpRequest>) -> PoemResult<Response> {
    match state.auth.consume_otp(&body.email, &body.code) {
        Ok(()) => {
            let token = state.auth.create_session(&body.email);
            Ok(Response::builder()
                .status(poem::http::StatusCode::OK)
                .content_type("application/json")
                .body(serde_json::to_vec(&serde_json::json!({ "token": token })).unwrap_or_default()))
        }
        Err(e) => Ok(Response::builder().status(poem::http::StatusCode::FORBIDDEN).body(e.message())),
    }
}

/// `POST /api/auth/logout` — セッショントークンを失効させる。
#[handler]
async fn logout(req: &Request, state: Data<&AppState>) -> PoemResult<Response> {
    let header = req.header(poem::http::header::AUTHORIZATION).unwrap_or("");
    if let Some(token) = header.strip_prefix("Bearer ") {
        state.auth.logout(token);
    }
    Ok(Response::builder().status(poem::http::StatusCode::OK).body("logged out"))
}

#[derive(Deserialize)]
struct AddAccountRequest {
    email: String,
}

/// `POST /api/accounts` — ログイン可能なメールアドレスを1件登録する
/// (管理者のみ)。`accounts_locked`中は管理者メール以外を拒否する。
#[handler]
async fn add_account(req: &Request, state: Data<&AppState>, body: poem::web::Json<AddAccountRequest>) -> PoemResult<Response> {
    require_admin_session(req, &state)?;
    let email = body.email.trim().to_string();
    if !email.contains('@') {
        return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("invalid email"));
    }
    if state.accounts_locked && email != state.admin_email {
        return Ok(Response::builder()
            .status(poem::http::StatusCode::FORBIDDEN)
            .body("account registration is currently restricted to the administrator email only"));
    }
    let mut store = accounts::load(&state.data_root, state.backend.as_ref()).await;
    store.emails.insert(email);
    accounts::save(&state.data_root, &store, state.backend.as_ref())
        .await
        .map_err(|e| poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;
    Ok(Response::builder().status(poem::http::StatusCode::CREATED).body("ok"))
}

/// `GET /api/accounts` — 登録済みメールアドレス一覧(管理者のみ)。
#[handler]
async fn list_accounts(req: &Request, state: Data<&AppState>) -> PoemResult<Response> {
    require_admin_session(req, &state)?;
    let store = accounts::load(&state.data_root, state.backend.as_ref()).await;
    let mut emails: Vec<&String> = store.emails.iter().collect();
    emails.sort();
    Ok(Response::builder().status(poem::http::StatusCode::OK).content_type("application/json").body(serde_json::to_vec(&emails).unwrap_or_default()))
}

#[derive(Deserialize)]
struct AccessRequestPayload {
    email: String,
    #[serde(default)]
    message: Option<String>,
}

/// `POST /api/accounts/request` — **認証不要、誰でも申請可能**。
/// ログイン許可を求める申請を保留リストへ追加する
/// (管理者が[`decide_access_request`]で許可するまでは無効)。
#[handler]
async fn request_access(state: Data<&AppState>, body: poem::web::Json<AccessRequestPayload>) -> PoemResult<Response> {
    let email = body.email.trim().to_string();
    if !email.contains('@') {
        return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("invalid email"));
    }
    let mut store = accounts::load(&state.data_root, state.backend.as_ref()).await;
    let id = accounts::generate_request_id();
    store.pending_requests.push(accounts::AccessRequest { id, email: email.clone(), message: body.message.clone() });
    accounts::save(&state.data_root, &store, state.backend.as_ref())
        .await
        .map_err(|e| poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;
    if let Some(smtp) = state.smtp.clone() {
        if let Err(e) = mail::send_access_request_notice(smtp, state.admin_email.clone(), email, body.message.clone()).await {
            tracing::warn!("failed to notify admin of access request: {e}");
        }
    }
    Ok(Response::builder().status(poem::http::StatusCode::CREATED).body("request submitted"))
}

/// `GET /api/accounts/requests` — 保留中の申請一覧(管理者のみ)。
#[handler]
async fn list_access_requests(req: &Request, state: Data<&AppState>) -> PoemResult<Response> {
    require_admin_session(req, &state)?;
    let store = accounts::load(&state.data_root, state.backend.as_ref()).await;
    Ok(Response::builder()
        .status(poem::http::StatusCode::OK)
        .content_type("application/json")
        .body(serde_json::to_vec(&store.pending_requests).unwrap_or_default()))
}

#[derive(Deserialize)]
struct DecideAccessRequestPayload {
    approve: bool,
    #[serde(default)]
    allow_view: bool,
    #[serde(default)]
    allow_edit: bool,
    /// このプロジェクトのメンバー管理権限も付与するか(2026-07-27追加)。
    /// グローバル管理者以外(プロジェクトマネージャー)が審査する場合、
    /// このフィールドが`true`だと権限昇格になるため`403`で拒否する
    /// (`decide_access_request`参照)。
    #[serde(default)]
    allow_manage_members: bool,
    #[serde(default)]
    project_id: Option<u64>,
    /// 名前付きロールプリセット(`"manager"`/`"developer"`/`"reporter"`、
    /// 2026-07-31追加)。指定された場合、上記の生フラグ
    /// (`allow_view`/`allow_edit`/`allow_manage_members`)より優先される
    /// (プリセットが実際に展開する権限をそのまま使う)。未知のロール名は
    /// `400`で拒否する。
    #[serde(default)]
    role: Option<String>,
}

/// `POST /api/accounts/requests/:id/decide` — 申請を審査する。
/// グローバル管理者に加え、`project_id`が指定されている場合はその
/// プロジェクトの`allow_manage_members`権限を持つアカウント(プロジェクト
/// マネージャー)も審査できる(ロール権限管理の細分化、2026-07-27追加、
/// Redmine本家の「プロジェクトマネージャーロール」相当)。
/// 承認時、`project_id`が指定されていればそのプロジェクトの
/// `access::AccessConfig::accounts`に閲覧/編集許可を書き込む
/// (プロジェクト指定が無い申請はアカウント登録のみ行う、この場合は
/// スコープを判定できないため引き続き管理者のみ)。
/// **権限昇格の防止**: プロジェクトマネージャー(グローバル管理者では
/// ない審査者)は`allow_manage_members: true`を新規に付与することは
/// できない(`403`)——メンバー管理権限自体の付与はグローバル管理者のみに
/// 限定する。
/// `accounts_locked`中は管理者メール以外の承認を拒否する
/// (`RGit`の`RGIT_ACCOUNTS_LOCKED`と同じ方針)。
#[handler]
async fn decide_access_request(
    req: &Request,
    PathExtractor(id): PathExtractor<String>,
    state: Data<&AppState>,
    body: poem::web::Json<DecideAccessRequestPayload>,
) -> PoemResult<Response> {
    let acting_email = require_admin_or_project_manager(req, &state, body.project_id).await?;
    let acting_is_global_admin = acting_email == state.admin_email;

    // `role`が指定されていれば生フラグより優先し、プリセットが展開する
    // 権限をそのまま使う(未知のロール名は`400`)。
    let granted_permission = if let Some(role) = &body.role {
        let Some(preset) = access::RolePreset::parse(role) else {
            return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("unknown role (expected \"manager\", \"developer\", or \"reporter\")"));
        };
        preset.permissions()
    } else {
        access::AccountPermission { allow_view: body.allow_view, allow_edit: body.allow_edit, allow_manage_members: body.allow_manage_members }
    };
    if body.approve && granted_permission.allow_manage_members && !acting_is_global_admin {
        return Ok(Response::builder()
            .status(poem::http::StatusCode::FORBIDDEN)
            .body("only the administrator can grant member-management permission"));
    }
    let mut store = accounts::load(&state.data_root, state.backend.as_ref()).await;
    let Some(pos) = store.pending_requests.iter().position(|r| r.id == id) else {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("request not found"));
    };
    let request = store.pending_requests.remove(pos);

    if body.approve && state.accounts_locked && request.email != state.admin_email {
        accounts::save(&state.data_root, &store, state.backend.as_ref())
            .await
            .map_err(|e| poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;
        return Ok(Response::builder()
            .status(poem::http::StatusCode::FORBIDDEN)
            .body("account registration is currently restricted to the administrator email only"));
    }

    if body.approve {
        store.emails.insert(request.email.clone());
    }
    accounts::save(&state.data_root, &store, state.backend.as_ref())
        .await
        .map_err(|e| poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;

    if body.approve {
        if let Some(pid) = body.project_id {
            let mut config = access::load(&state.data_root, pid, state.backend.as_ref()).await;
            config.accounts.insert(request.email.clone(), granted_permission);
            access::save(&state.data_root, pid, &config, state.backend.as_ref())
                .await
                .map_err(|e| poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;
        }
    }

    if let Some(smtp) = state.smtp.clone() {
        if let Err(e) = mail::send_access_decision(smtp, request.email.clone(), body.approve).await {
            tracing::warn!("failed to notify requester of decision: {e}");
        }
    }
    Ok(Response::builder().status(poem::http::StatusCode::OK).body(if body.approve { "approved" } else { "denied" }))
}

#[derive(Deserialize)]
struct CreateSavedQueryRequest {
    name: String,
    #[serde(default)]
    project_id: Option<u64>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    tracker: Option<String>,
    #[serde(default)]
    assignee: Option<String>,
}

/// `POST /api/saved_queries` — `GET /api/tickets`と同じ絞り込み条件を
/// 名前付きで保存する(ログイン必須、所有者は自分のみ)。
#[handler]
async fn create_saved_query(req: &Request, state: Data<&AppState>, body: poem::web::Json<CreateSavedQueryRequest>) -> PoemResult<Response> {
    let Some(email) = session_email(req, &state) else {
        return Err(poem::Error::from_string("login required", poem::http::StatusCode::UNAUTHORIZED));
    };
    if body.name.trim().is_empty() {
        return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("name must not be empty"));
    }
    if let Some(s) = &body.status {
        if parse_ticket_status(s).is_none() {
            return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("status must be one of open, in_progress, closed"));
        }
    }
    if let Some(t) = &body.tracker {
        if parse_tracker(t).is_none() {
            return Ok(Response::builder().status(poem::http::StatusCode::BAD_REQUEST).body("tracker must be one of bug, feature, support, task"));
        }
    }
    let mut store = saved_queries::load(&state.data_root, state.backend.as_ref()).await;
    let id = store.next_id;
    store.next_id += 1;
    let query = saved_queries::SavedQuery {
        id,
        owner_email: email,
        name: body.name.clone(),
        project_id: body.project_id,
        status: body.status.clone(),
        tracker: body.tracker.clone(),
        assignee: body.assignee.clone(),
        created_at: project::now_rfc3339(),
    };
    store.queries.push(query.clone());
    saved_queries::save(&state.data_root, &store, state.backend.as_ref())
        .await
        .map_err(|e| poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;
    Ok(Response::builder().status(poem::http::StatusCode::CREATED).content_type("application/json").body(serde_json::to_vec(&query).unwrap_or_default()))
}

/// `GET /api/saved_queries` — 自分が作成した保存済みクエリの一覧
/// (ログイン必須、他人のクエリは見えない)。
#[handler]
async fn list_saved_queries(req: &Request, state: Data<&AppState>) -> PoemResult<Response> {
    let Some(email) = session_email(req, &state) else {
        return Err(poem::Error::from_string("login required", poem::http::StatusCode::UNAUTHORIZED));
    };
    let store = saved_queries::load(&state.data_root, state.backend.as_ref()).await;
    let mine = store.owned_by(&email);
    Ok(Response::builder().status(poem::http::StatusCode::OK).content_type("application/json").body(serde_json::to_vec(&mine).unwrap_or_default()))
}

/// `DELETE /api/saved_queries/:id` — 保存済みクエリを削除する
/// (管理者または作成者本人のみ、コメント/作業時間記録の削除と同じ
/// 権限モデル)。
#[handler]
async fn delete_saved_query(req: &Request, PathExtractor(id): PathExtractor<u64>, state: Data<&AppState>) -> PoemResult<Response> {
    let Some(email) = session_email(req, &state) else {
        return Err(poem::Error::from_string("login required", poem::http::StatusCode::UNAUTHORIZED));
    };
    let mut store = saved_queries::load(&state.data_root, state.backend.as_ref()).await;
    let Some(query) = store.find(id) else {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("saved query not found"));
    };
    if query.owner_email != email && email != state.admin_email {
        return Ok(Response::builder().status(poem::http::StatusCode::FORBIDDEN).body("only the owner or an administrator can delete this saved query"));
    }
    store.queries.retain(|q| q.id != id);
    saved_queries::save(&state.data_root, &store, state.backend.as_ref())
        .await
        .map_err(|e| poem::Error::from_string(e.to_string(), poem::http::StatusCode::INTERNAL_SERVER_ERROR))?;
    Ok(Response::builder().status(poem::http::StatusCode::OK).body("deleted"))
}

/// `GET /api/saved_queries/:id/run` — 保存済みクエリを実行し、
/// `GET /api/tickets`と同じアクセス制御を適用した結果を返す(所有者または
/// 管理者のみ実行可能)。
#[handler]
async fn run_saved_query(req: &Request, PathExtractor(id): PathExtractor<u64>, state: Data<&AppState>) -> PoemResult<Response> {
    let Some(email) = session_email(req, &state) else {
        return Err(poem::Error::from_string("login required", poem::http::StatusCode::UNAUTHORIZED));
    };
    let store = saved_queries::load(&state.data_root, state.backend.as_ref()).await;
    let Some(query) = store.find(id) else {
        return Ok(Response::builder().status(poem::http::StatusCode::NOT_FOUND).body("saved query not found"));
    };
    let is_admin_caller = email == state.admin_email;
    if query.owner_email != email && !is_admin_caller {
        return Ok(Response::builder().status(poem::http::StatusCode::FORBIDDEN).body("only the owner or an administrator can run this saved query"));
    }
    let status_filter = query.status.as_deref().and_then(parse_ticket_status);
    let tracker_filter = query.tracker.as_deref().and_then(parse_tracker);
    let visible =
        filter_visible_tickets(&state, Some(email.as_str()), is_admin_caller, status_filter, query.project_id, tracker_filter, query.assignee.as_deref()).await;
    Ok(Response::builder().status(poem::http::StatusCode::OK).content_type("application/json").body(serde_json::to_vec(&visible).unwrap_or_default()))
}

fn env_data_dir() -> PathBuf {
    std::env::var("RSCHIKETTO_DATA_DIR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("./data"))
}

/// ルーティング定義を`main()`とテスト(`poem::test::TestClient`)の両方から
/// 再利用できるように切り出したもの。
fn build_routes(state: AppState) -> impl poem::Endpoint {
    Route::new()
        .at("/", get(index))
        .at("/pkg/:file", get(serve_pkg))
        .at("/healthz", get(healthz))
        .at("/api/auth/request-otp", post(request_otp))
        .at("/api/auth/verify-otp", post(verify_otp))
        .at("/api/auth/logout", post(logout))
        .at("/api/accounts", get(list_accounts).post(add_account))
        .at("/api/accounts/request", post(request_access))
        .at("/api/accounts/requests", get(list_access_requests))
        .at("/api/accounts/requests/:id/decide", post(decide_access_request))
        .at("/api/projects", get(list_projects).post(create_project))
        .at("/api/projects/:id", get(get_project).put(update_project).delete(delete_project))
        .at("/api/projects/:id/children", get(list_project_children))
        .at("/api/tickets", get(list_tickets).post(create_ticket))
        .at("/api/tickets/:id", get(get_ticket).put(update_ticket))
        .at("/api/tickets/:id/comments", get(list_comments).post(create_comment))
        .at("/api/comments/:id", delete(delete_comment))
        .at("/api/tickets/:id/attachments", get(list_attachments).post(create_attachment))
        .at("/api/attachments/:id/download", get(download_attachment))
        .at("/api/attachments/:id", delete(delete_attachment))
        .at("/api/tickets/:id/relations", get(list_relations).post(create_relation))
        .at("/api/relations/:id", delete(delete_relation))
        .at("/api/tickets/:id/time_entries", get(list_time_entries).post(create_time_entry))
        .at("/api/time_entries/:id", delete(delete_time_entry))
        .at("/api/projects/:id/wiki", get(list_wiki_pages).post(create_wiki_page))
        .at("/api/wiki/:id", get(get_wiki_page).put(update_wiki_page).delete(delete_wiki_page))
        .at("/api/saved_queries", get(list_saved_queries).post(create_saved_query))
        .at("/api/saved_queries/:id", delete(delete_saved_query))
        .at("/api/saved_queries/:id/run", get(run_saved_query))
        .data(state)
        .with(Tracing)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let data_root = env_data_dir();
    tokio::fs::create_dir_all(&data_root).await?;
    tracing::info!("rs-chiketto v0.1.0 starting, data_root={:?}", data_root);

    let admin_email = std::env::var("RSCHIKETTO_ADMIN_EMAIL").unwrap_or_else(|_| "admin@example.com".to_string());
    let smtp = mail::SmtpConfig::from_env();
    if smtp.is_none() {
        tracing::warn!("RSCHIKETTO_SMTP_* not fully configured; /api/auth/request-otp will return 503");
    }
    let accounts_locked = std::env::var("RSCHIKETTO_ACCOUNTS_LOCKED").map(|v| v != "false" && v != "0").unwrap_or(true);
    if accounts_locked {
        tracing::info!("account registration is locked to the admin email only (RSCHIKETTO_ACCOUNTS_LOCKED=false to lift)");
    }
    let backend = storage::backend_from_env();
    let state = AppState { data_root, auth: Arc::new(auth::AuthStore::default()), admin_email, smtp, accounts_locked, backend };

    tracing::info!("storage backend: {} (RSCHIKETTO_STORAGE_BACKEND)", storage::selected_backend_name());
    ddns::spawn_if_configured();

    let app = build_routes(state);

    let port = std::env::var("RSCHIKETTO_PORT").unwrap_or_else(|_| "8100".to_string());
    let addr = format!("0.0.0.0:{port}");
    tracing::info!("listening on {addr}");
    Server::new(TcpListener::bind(addr)).run(app).await?;
    Ok(())
}

#[cfg(test)]
mod handler_tests {
    //! `poem::test::TestClient`を使ったハンドラレベルの統合テスト
    //! (2026-07-21追記、HANDOFF記載の宿題への対応)。
    //! `cargo test`実行時にテストごとに独立した一時ディレクトリを
    //! `RSCHIKETTO_DATA_DIR`として使うため、`AppState.data_root`は各テスト
    //! ごとに直接構築する(実プロセスの環境変数には依存しない)。

    use super::*;
    use poem::test::TestClient;

    const ADMIN_EMAIL: &str = "admin@example.com";

    fn temp_dir(label: &str) -> PathBuf {
        let unique = accounts::generate_request_id();
        std::env::temp_dir().join(format!("rschiketto-handler-test-{label}-{unique}"))
    }

    /// `accounts_locked`を指定してテスト用の`AppState`を構築する
    /// (環境変数に依存しないテストローカル構築、SMTP未設定)。
    async fn make_state(label: &str, accounts_locked: bool) -> AppState {
        let data_root = temp_dir(label);
        tokio::fs::create_dir_all(&data_root).await.unwrap();
        AppState { data_root, auth: Arc::new(auth::AuthStore::default()), admin_email: ADMIN_EMAIL.to_string(), smtp: None, accounts_locked, backend: std::sync::Arc::new(storage::LocalFsBackend) }
    }

    /// 管理者としてログイン済みのセッショントークンを、OTPフローを経由
    /// せず直接`AuthStore`に発行させて得る(SMTP無し環境でもテスト可能)。
    fn admin_token(state: &AppState) -> String {
        state.auth.create_session(ADMIN_EMAIL)
    }

    /// `RSCHIKETTO_DEV_LOG_OTP`はプロセス全体のグローバル環境変数のため、
    /// 並列実行される他のテストと競合しないようこのMutexで直列化する。
    fn dev_log_otp_env_lock() -> &'static std::sync::Mutex<()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
    }

    #[tokio::test]
    async fn request_otp_without_smtp_returns_503_by_default() {
        // 2026-07-27追記の回帰確認: 開発バイパスを明示していない場合、
        // 従来通りSMTP未設定なら503を返す(本番での意図しないバイパスを
        // 防ぐデフォルト動作)。
        let _guard = dev_log_otp_env_lock().lock().unwrap();
        std::env::remove_var("RSCHIKETTO_DEV_LOG_OTP");

        let state = make_state("otp-503-default", true).await;
        let app = build_routes(state);
        let client = TestClient::new(app);

        let resp = client
            .post("/api/auth/request-otp")
            .body_json(&serde_json::json!({ "email": ADMIN_EMAIL }))
            .send()
            .await;
        resp.assert_status(poem::http::StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn request_otp_with_dev_log_otp_bypasses_smtp_requirement() {
        // 2026-07-27追記: SMTP未設定でも`RSCHIKETTO_DEV_LOG_OTP=true`を
        // 明示すれば200を返し、GUIをすぐ使い始められる(外部監査で指摘
        // された最重要のセットアップ障壁の解消)。
        let _guard = dev_log_otp_env_lock().lock().unwrap();
        std::env::set_var("RSCHIKETTO_DEV_LOG_OTP", "true");

        let state = make_state("otp-dev-bypass", true).await;
        let app = build_routes(state);
        let client = TestClient::new(app);

        let resp = client
            .post("/api/auth/request-otp")
            .body_json(&serde_json::json!({ "email": ADMIN_EMAIL }))
            .send()
            .await;
        resp.assert_status_is_ok();

        std::env::remove_var("RSCHIKETTO_DEV_LOG_OTP");
    }

    #[tokio::test]
    async fn unauthenticated_list_tickets_returns_200_with_empty_array() {
        // HANDOFFに明記された設計: 未ログインの`GET /api/tickets`は
        // 401ではなく200・空配列(project単位のフィルタリングにより
        // 可視チケットが0件になるため)。
        let state = make_state("list-empty", true).await;
        let app = build_routes(state);
        let client = TestClient::new(app);

        let resp = client.get("/api/tickets").send().await;
        resp.assert_status_is_ok();
        resp.assert_text("[]").await;
    }

    #[tokio::test]
    async fn root_returns_landing_page_with_key_markers() {
        // UXバグ修正の検証: JSON APIオンリーで何も表示されなかった`GET /`が
        // 何らかのHTMLを返すこと。2026-07-23、`web/index.html`(ブラウザGUI)
        // が存在する場合はそちらを優先して返すよう変更されたため
        // (ユーザー指示「チケット管理を行なうWEBアプリなのでGUIは基本」)、
        // `cargo test`実行時のカレントディレクトリ(リポジトリルート)には
        // 実際に`web/index.html`が存在し、GUIシェルが返る。
        let state = make_state("landing-page", true).await;
        let app = build_routes(state);
        let client = TestClient::new(app);

        let resp = client.get("/").send().await;
        resp.assert_status_is_ok();
        let body = resp.0.into_body().into_string().await.unwrap();
        assert!(body.contains("<title>open-redmine</title>"));
        assert!(body.contains("request-otp-btn"));
    }

    // 正直な開示: `web/index.html`が存在しない場合のフォールバック
    // (`INDEX_HTML`)自体は`index()`のコードレビューで明らかに正しい
    // 単純なmatch分岐だが、専用テストは追加していない——
    // `RSCHIKETTO_WEB_DIR`環境変数はプロセス全体で共有されるため、
    // `cargo test`のデフォルトの並行実行下で他のテスト(同じ`GET /`を
    // 叩く`root_returns_landing_page_with_key_markers`)と競合し
    // フレーキーになるリスクを避けた。

    #[tokio::test]
    async fn self_service_account_request_returns_201_and_creates_pending_request() {
        let state = make_state("self-service-request", true).await;
        let data_root = state.data_root.clone();
        let app = build_routes(state);
        let client = TestClient::new(app);

        let resp = client
            .post("/api/accounts/request")
            .body_json(&serde_json::json!({ "email": "newcomer@example.com", "message": "please let me in" }))
            .send()
            .await;
        resp.assert_status(poem::http::StatusCode::CREATED);

        let store = accounts::load(&data_root, &storage::LocalFsBackend).await;
        assert_eq!(store.pending_requests.len(), 1);
        assert_eq!(store.pending_requests[0].email, "newcomer@example.com");
    }

    #[tokio::test]
    async fn admin_approving_a_request_grants_the_expected_access_config_entry() {
        let state = make_state("approve-grants-access", false).await;
        let data_root = state.data_root.clone();
        let token = admin_token(&state);
        let app = build_routes(state);
        let client = TestClient::new(app);

        // まず自己申請を作成。
        client
            .post("/api/accounts/request")
            .body_json(&serde_json::json!({ "email": "member@example.com" }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::CREATED);

        let store = accounts::load(&data_root, &storage::LocalFsBackend).await;
        let request_id = store.pending_requests[0].id.clone();

        // 管理者セッションで承認、project_id=42へview権限を付与。
        let resp = client
            .post(format!("/api/accounts/requests/{request_id}/decide"))
            .header("Authorization", format!("Bearer {token}"))
            .body_json(&serde_json::json!({
                "approve": true,
                "allow_view": true,
                "allow_edit": false,
                "project_id": 42
            }))
            .send()
            .await;
        resp.assert_status_is_ok();

        // 承認によりaccounts一覧へ追加されていること。
        let updated_store = accounts::load(&data_root, &storage::LocalFsBackend).await;
        assert!(updated_store.emails.contains("member@example.com"));
        assert!(updated_store.pending_requests.is_empty());

        // access::AccessConfigへ期待した許可が書き込まれていること。
        let config = access::load(&data_root, 42, &storage::LocalFsBackend).await;
        let perm = config.accounts.get("member@example.com").expect("member should have an access grant");
        assert!(perm.allow_view);
        assert!(!perm.allow_edit);
    }

    /// ロール権限管理の細分化(2026-07-27追加): グローバル管理者ではない
    /// 「プロジェクトマネージャー」(`allow_manage_members: true`を持つ
    /// アカウント)が、自分の管理するプロジェクト宛の申請を審査できる
    /// こと、他プロジェクト宛の申請は審査できないこと(403)、
    /// `allow_manage_members: true`自体を新規に付与しようとすると
    /// 権限昇格として拒否される(403)ことを確認する。
    #[tokio::test]
    async fn project_manager_can_decide_requests_scoped_to_their_own_project_but_not_others_or_grant_manage_members() {
        let state = make_state("project-manager-decide", false).await;
        let data_root = state.data_root.clone();
        let admin = admin_token(&state);
        // マネージャー役のセッショントークンも、OTPフローを経由せず
        // `AuthStore`へ直接発行させる(`admin_token`と同じテスト用手段)。
        let manager_token = state.auth.create_session("manager@example.com");
        let app = build_routes(state);
        let client = TestClient::new(app);

        // プロジェクトマネージャー自身のアカウントを登録し、project_id=1に
        // 対するallow_manage_membersを管理者が付与する(自己申請→承認)。
        client.post("/api/accounts/request").body_json(&serde_json::json!({ "email": "manager@example.com" })).send().await.assert_status(poem::http::StatusCode::CREATED);
        let manager_request_id = accounts::load(&data_root, &storage::LocalFsBackend).await.pending_requests[0].id.clone();
        client
            .post(format!("/api/accounts/requests/{manager_request_id}/decide"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "approve": true, "allow_view": true, "allow_edit": true, "allow_manage_members": true, "project_id": 1 }))
            .send()
            .await
            .assert_status_is_ok();

        // 一般ユーザーからの自己申請(project_id=1宛)。
        client.post("/api/accounts/request").body_json(&serde_json::json!({ "email": "newcomer@example.com" })).send().await.assert_status(poem::http::StatusCode::CREATED);
        let newcomer_request_id = accounts::load(&data_root, &storage::LocalFsBackend).await.pending_requests[0].id.clone();

        // プロジェクトマネージャーが自分の管理するproject_id=1宛の申請を
        // 審査できること(管理者トークンなしで成功)。
        let decide_resp = client
            .post(format!("/api/accounts/requests/{newcomer_request_id}/decide"))
            .header("Authorization", format!("Bearer {manager_token}"))
            .body_json(&serde_json::json!({ "approve": true, "allow_view": true, "allow_edit": false, "project_id": 1 }))
            .send()
            .await;
        decide_resp.assert_status_is_ok();
        let config = access::load(&data_root, 1, &storage::LocalFsBackend).await;
        assert!(config.accounts.get("newcomer@example.com").expect("newcomer should have a grant").allow_view);

        // 別プロジェクト(project_id=2)宛の申請は、project_id=1のマネージャー
        // では審査できない(403)。
        client.post("/api/accounts/request").body_json(&serde_json::json!({ "email": "other-project-user@example.com" })).send().await.assert_status(poem::http::StatusCode::CREATED);
        let other_request_id = accounts::load(&data_root, &storage::LocalFsBackend).await.pending_requests[0].id.clone();
        client
            .post(format!("/api/accounts/requests/{other_request_id}/decide"))
            .header("Authorization", format!("Bearer {manager_token}"))
            .body_json(&serde_json::json!({ "approve": true, "allow_view": true, "project_id": 2 }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::FORBIDDEN);

        // プロジェクトマネージャーはallow_manage_members: trueを新規に
        // 付与できない(権限昇格の防止、403)。
        client.post("/api/accounts/request").body_json(&serde_json::json!({ "email": "wanna-be-manager@example.com" })).send().await.assert_status(poem::http::StatusCode::CREATED);
        let escalation_request_id = accounts::load(&data_root, &storage::LocalFsBackend).await.pending_requests[0].id.clone();
        client
            .post(format!("/api/accounts/requests/{escalation_request_id}/decide"))
            .header("Authorization", format!("Bearer {manager_token}"))
            .body_json(&serde_json::json!({ "approve": true, "allow_view": true, "allow_manage_members": true, "project_id": 1 }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn accounts_locked_rejects_non_admin_approval_with_403() {
        // このテストはローカル構築の`AppState`で`accounts_locked: true`を
        // 指定するのみで、プロセス環境変数`RSCHIKETTO_ACCOUNTS_LOCKED`は
        // 一切変更しない(他テストへの影響を避けるため)。
        let state = make_state("locked-rejects-approval", true).await;
        let data_root = state.data_root.clone();
        let token = admin_token(&state);
        let app = build_routes(state);
        let client = TestClient::new(app);

        client
            .post("/api/accounts/request")
            .body_json(&serde_json::json!({ "email": "outsider@example.com" }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::CREATED);

        let store = accounts::load(&data_root, &storage::LocalFsBackend).await;
        let request_id = store.pending_requests[0].id.clone();

        // 管理者セッションであっても、承認対象が管理者メール以外かつ
        // accounts_locked中は403で拒否される(main.rsのdecide_access_request参照)。
        let resp = client
            .post(format!("/api/accounts/requests/{request_id}/decide"))
            .header("Authorization", format!("Bearer {token}"))
            .body_json(&serde_json::json!({ "approve": true }))
            .send()
            .await;
        resp.assert_status(poem::http::StatusCode::FORBIDDEN);

        // 拒否されても申請自体はpendingリストから取り除かれ、emailsには
        // 追加されていないこと(main.rsの実装通り)。
        let after = accounts::load(&data_root, &storage::LocalFsBackend).await;
        assert!(!after.emails.contains("outsider@example.com"));
    }

    /// Project CRUD(HANDOFF「(3) Project自体のCRUD」対応の検証):
    /// 管理者が作成し、誰でも一覧・詳細取得でき、管理者のみ更新・削除
    /// できることを確認する。
    #[tokio::test]
    async fn project_crud_via_http() {
        let state = make_state("project-crud", true).await;
        let token = admin_token(&state);
        let app = build_routes(state);
        let client = TestClient::new(app);

        // 非管理者(未ログイン)は作成できない。
        client
            .post("/api/projects")
            .body_json(&serde_json::json!({ "name": "no-auth", "description": "" }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::UNAUTHORIZED);

        // 管理者は作成できる。
        let resp = client
            .post("/api/projects")
            .header("Authorization", format!("Bearer {token}"))
            .body_json(&serde_json::json!({ "name": "demo", "description": "a demo project" }))
            .send()
            .await;
        resp.assert_status(poem::http::StatusCode::CREATED);
        let body: serde_json::Value = resp.json().await.value().deserialize();
        let id = body["id"].as_u64().expect("created project should have an id");
        assert_eq!(body["name"], "demo");

        // 一覧取得(認証不要)。
        let list = client.get("/api/projects").send().await;
        list.assert_status_is_ok();
        let list_body: serde_json::Value = list.json().await.value().deserialize();
        assert_eq!(list_body.as_array().unwrap().len(), 1);

        // 詳細取得(認証不要)。
        client.get(format!("/api/projects/{id}")).send().await.assert_status_is_ok();

        // 非管理者は更新できない。
        client
            .put(format!("/api/projects/{id}"))
            .body_json(&serde_json::json!({ "name": "renamed" }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::UNAUTHORIZED);

        // 管理者は更新できる。
        let updated = client
            .put(format!("/api/projects/{id}"))
            .header("Authorization", format!("Bearer {token}"))
            .body_json(&serde_json::json!({ "name": "renamed" }))
            .send()
            .await;
        updated.assert_status_is_ok();
        let updated_body: serde_json::Value = updated.json().await.value().deserialize();
        assert_eq!(updated_body["name"], "renamed");

        // 存在しないIDへの操作は404。
        client.get("/api/projects/999999").send().await.assert_status(poem::http::StatusCode::NOT_FOUND);

        // 管理者は削除できる。
        client
            .delete(format!("/api/projects/{id}"))
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .assert_status_is_ok();
        client.get(format!("/api/projects/{id}")).send().await.assert_status(poem::http::StatusCode::NOT_FOUND);
    }

    /// チケット作成時に`project_id`が実在しないプロジェクトを指す場合、
    /// `400`で明確に拒否されることを確認する(HANDOFFタスク要件)。
    #[tokio::test]
    async fn create_ticket_against_nonexistent_project_fails_cleanly() {
        let state = make_state("ticket-nonexistent-project", true).await;
        let token = admin_token(&state);
        let app = build_routes(state);
        let client = TestClient::new(app);

        let resp = client
            .post("/api/tickets")
            .header("Authorization", format!("Bearer {token}"))
            .body_json(&serde_json::json!({ "title": "t", "description": "d", "project_id": 424242 }))
            .send()
            .await;
        resp.assert_status(poem::http::StatusCode::BAD_REQUEST);
    }

    /// アクセス制御が実在の`project_id`(ハッシュ経由ではなく連番ID)で
    /// 正しく効くことを確認する: private既定のプロジェクトへ、権限の
    /// 無いアカウントがチケット作成しようとすると403、権限が付与された
    /// アカウントは成功する。
    #[tokio::test]
    async fn access_control_gates_ticket_creation_by_real_project_id() {
        let state = make_state("access-control-real-project-id", true).await;
        let data_root = state.data_root.clone();
        let admin = admin_token(&state);
        let app = build_routes(state);
        let client = TestClient::new(app);

        // 管理者がプロジェクトを作成。
        let created = client
            .post("/api/projects")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "name": "private-proj", "description": "" }))
            .send()
            .await;
        created.assert_status(poem::http::StatusCode::CREATED);
        let created_body: serde_json::Value = created.json().await.value().deserialize();
        let project_id = created_body["id"].as_u64().unwrap();

        // 未ログインでの作成は401(private既定・admin以外拒否)。
        client
            .post("/api/tickets")
            .body_json(&serde_json::json!({ "title": "t", "description": "d", "project_id": project_id }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::UNAUTHORIZED);

        // access::AccessConfigへ直接member@example.comへのedit許可を書き込み、
        // 実際にAuthStoreでセッションを発行してから許可されることを確認する。
        let mut config = access::load(&data_root, project_id, &storage::LocalFsBackend).await;
        config.accounts.insert("member@example.com".to_string(), access::AccountPermission { allow_view: true, allow_edit: true, allow_manage_members: false });
        access::save(&data_root, project_id, &config, &storage::LocalFsBackend).await.unwrap();

        // 新しいAppStateを同じdata_rootで作り直し(auth::AuthStoreは
        // プロセスごとに新規になるため、このAppStateに対応する
        // TestClientでセッションを発行して検証する)。
        let state2 = AppState { data_root: data_root.clone(), auth: Arc::new(auth::AuthStore::default()), admin_email: ADMIN_EMAIL.to_string(), smtp: None, accounts_locked: true, backend: std::sync::Arc::new(storage::LocalFsBackend) };
        let member_session = state2.auth.create_session("member@example.com");
        let app2 = build_routes(state2);
        let client2 = TestClient::new(app2);

        let resp = client2
            .post("/api/tickets")
            .header("Authorization", format!("Bearer {member_session}"))
            .body_json(&serde_json::json!({ "title": "t", "description": "d", "project_id": project_id }))
            .send()
            .await;
        resp.assert_status(poem::http::StatusCode::CREATED);

        // 別の(許可されていない)一般ユーザーは403。
        let stranger_state = AppState { data_root: data_root.clone(), auth: Arc::new(auth::AuthStore::default()), admin_email: ADMIN_EMAIL.to_string(), smtp: None, accounts_locked: true, backend: std::sync::Arc::new(storage::LocalFsBackend) };
        let stranger_session = stranger_state.auth.create_session("stranger@example.com");
        let stranger_app = build_routes(stranger_state);
        let stranger_client = TestClient::new(stranger_app);
        stranger_client
            .post("/api/tickets")
            .header("Authorization", format!("Bearer {stranger_session}"))
            .body_json(&serde_json::json!({ "title": "t", "description": "d", "project_id": project_id }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::FORBIDDEN);
    }

    /// サブプロジェクト階層: 子プロジェクト作成、`GET /children`での一覧、
    /// および循環参照(親を自分の子孫に設定しようとする)が`400`で
    /// 拒否されることを確認する。
    #[tokio::test]
    async fn subproject_hierarchy_children_listing_and_cycle_rejection() {
        let state = make_state("subproject-hierarchy", true).await;
        let token = admin_token(&state);
        let app = build_routes(state);
        let client = TestClient::new(app);

        // ルートプロジェクトを作成。
        let root = client
            .post("/api/projects")
            .header("Authorization", format!("Bearer {token}"))
            .body_json(&serde_json::json!({ "name": "root" }))
            .send()
            .await;
        root.assert_status(poem::http::StatusCode::CREATED);
        let root_id = root.json().await.value().deserialize::<serde_json::Value>()["id"].as_u64().unwrap();

        // rootを親に持つ子プロジェクトを作成。
        let child = client
            .post("/api/projects")
            .header("Authorization", format!("Bearer {token}"))
            .body_json(&serde_json::json!({ "name": "child", "parent_id": root_id }))
            .send()
            .await;
        child.assert_status(poem::http::StatusCode::CREATED);
        let child_body: serde_json::Value = child.json().await.value().deserialize();
        let child_id = child_body["id"].as_u64().unwrap();
        assert_eq!(child_body["parent_id"], root_id);

        // GET /children はrootに対してchildのみを返す(直接の子のみ)。
        let children = client.get(format!("/api/projects/{root_id}/children")).send().await;
        children.assert_status_is_ok();
        let children_body: serde_json::Value = children.json().await.value().deserialize();
        let arr = children_body.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], child_id);

        // rootの親をchildに設定しようとすると循環参照で400拒否される。
        let cycle_attempt = client
            .put(format!("/api/projects/{root_id}"))
            .header("Authorization", format!("Bearer {token}"))
            .body_json(&serde_json::json!({ "parent_id": child_id }))
            .send()
            .await;
        cycle_attempt.assert_status(poem::http::StatusCode::BAD_REQUEST);

        // 自分自身を親に設定しようとしても400拒否される。
        let self_cycle = client
            .put(format!("/api/projects/{child_id}"))
            .header("Authorization", format!("Bearer {token}"))
            .body_json(&serde_json::json!({ "parent_id": child_id }))
            .send()
            .await;
        self_cycle.assert_status(poem::http::StatusCode::BAD_REQUEST);
    }

    /// チケットコメント: プロジェクトへの編集権限を持つアカウントのみが
    /// コメント投稿できること(権限が無ければ403、未ログインは401)を確認する。
    #[tokio::test]
    async fn comment_creation_is_gated_by_project_edit_access() {
        let state = make_state("comment-create-gated", true).await;
        let data_root = state.data_root.clone();
        let admin = admin_token(&state);
        let app = build_routes(state);
        let client = TestClient::new(app);

        let created = client
            .post("/api/projects")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "name": "comment-proj" }))
            .send()
            .await;
        created.assert_status(poem::http::StatusCode::CREATED);
        let project_id = created.json().await.value().deserialize::<serde_json::Value>()["id"].as_u64().unwrap();

        let ticket = client
            .post("/api/tickets")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "title": "t", "description": "d", "project_id": project_id }))
            .send()
            .await;
        ticket.assert_status(poem::http::StatusCode::CREATED);
        let ticket_id = ticket.json().await.value().deserialize::<serde_json::Value>()["id"].as_u64().unwrap();

        // 未ログインでのコメント投稿は401。
        client
            .post(format!("/api/tickets/{ticket_id}/comments"))
            .body_json(&serde_json::json!({ "body": "hi" }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::UNAUTHORIZED);

        // editが無い一般ユーザーは403。
        let stranger_state = AppState { data_root: data_root.clone(), auth: Arc::new(auth::AuthStore::default()), admin_email: ADMIN_EMAIL.to_string(), smtp: None, accounts_locked: true, backend: std::sync::Arc::new(storage::LocalFsBackend) };
        let stranger_session = stranger_state.auth.create_session("stranger@example.com");
        let stranger_app = build_routes(stranger_state);
        let stranger_client = TestClient::new(stranger_app);
        stranger_client
            .post(format!("/api/tickets/{ticket_id}/comments"))
            .header("Authorization", format!("Bearer {stranger_session}"))
            .body_json(&serde_json::json!({ "body": "hi" }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::FORBIDDEN);

        // editを付与されたメンバーは投稿できる。
        let mut config = access::load(&data_root, project_id, &storage::LocalFsBackend).await;
        config.accounts.insert("member@example.com".to_string(), access::AccountPermission { allow_view: true, allow_edit: true, allow_manage_members: false });
        access::save(&data_root, project_id, &config, &storage::LocalFsBackend).await.unwrap();
        let member_state = AppState { data_root: data_root.clone(), auth: Arc::new(auth::AuthStore::default()), admin_email: ADMIN_EMAIL.to_string(), smtp: None, accounts_locked: true, backend: std::sync::Arc::new(storage::LocalFsBackend) };
        let member_session = member_state.auth.create_session("member@example.com");
        let member_app = build_routes(member_state);
        let member_client = TestClient::new(member_app);
        let posted = member_client
            .post(format!("/api/tickets/{ticket_id}/comments"))
            .header("Authorization", format!("Bearer {member_session}"))
            .body_json(&serde_json::json!({ "body": "looks good" }))
            .send()
            .await;
        posted.assert_status(poem::http::StatusCode::CREATED);
        let posted_body: serde_json::Value = posted.json().await.value().deserialize();
        assert_eq!(posted_body["author_email"], "member@example.com");
        assert_eq!(posted_body["body"], "looks good");
    }

    /// コメント閲覧: プロジェクトへの閲覧権限を持たないアカウントは
    /// `GET /api/tickets/:id/comments`が403(未ログインは401)、権限があれば
    /// 200でコメント一覧が返ることを確認する。
    #[tokio::test]
    async fn comment_visibility_is_gated_by_project_view_access() {
        let state = make_state("comment-view-gated", true).await;
        let data_root = state.data_root.clone();
        let admin = admin_token(&state);
        let app = build_routes(state);
        let client = TestClient::new(app);

        let created = client
            .post("/api/projects")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "name": "comment-view-proj" }))
            .send()
            .await;
        let project_id = created.json().await.value().deserialize::<serde_json::Value>()["id"].as_u64().unwrap();

        let ticket = client
            .post("/api/tickets")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "title": "t", "description": "d", "project_id": project_id }))
            .send()
            .await;
        let ticket_id = ticket.json().await.value().deserialize::<serde_json::Value>()["id"].as_u64().unwrap();

        client
            .post(format!("/api/tickets/{ticket_id}/comments"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "body": "admin note" }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::CREATED);

        // 未ログインは401。
        client.get(format!("/api/tickets/{ticket_id}/comments")).send().await.assert_status(poem::http::StatusCode::UNAUTHORIZED);

        // 権限の無い一般ユーザーは403。
        let stranger_state = AppState { data_root: data_root.clone(), auth: Arc::new(auth::AuthStore::default()), admin_email: ADMIN_EMAIL.to_string(), smtp: None, accounts_locked: true, backend: std::sync::Arc::new(storage::LocalFsBackend) };
        let stranger_session = stranger_state.auth.create_session("stranger@example.com");
        let stranger_app = build_routes(stranger_state);
        let stranger_client = TestClient::new(stranger_app);
        stranger_client
            .get(format!("/api/tickets/{ticket_id}/comments"))
            .header("Authorization", format!("Bearer {stranger_session}"))
            .send()
            .await
            .assert_status(poem::http::StatusCode::FORBIDDEN);

        // view権限を付与されたメンバーは200でコメント一覧を取得できる。
        let mut config = access::load(&data_root, project_id, &storage::LocalFsBackend).await;
        config.accounts.insert("viewer@example.com".to_string(), access::AccountPermission { allow_view: true, allow_edit: false, allow_manage_members: false });
        access::save(&data_root, project_id, &config, &storage::LocalFsBackend).await.unwrap();
        let viewer_state = AppState { data_root: data_root.clone(), auth: Arc::new(auth::AuthStore::default()), admin_email: ADMIN_EMAIL.to_string(), smtp: None, accounts_locked: true, backend: std::sync::Arc::new(storage::LocalFsBackend) };
        let viewer_session = viewer_state.auth.create_session("viewer@example.com");
        let viewer_app = build_routes(viewer_state);
        let viewer_client = TestClient::new(viewer_app);
        let resp = viewer_client
            .get(format!("/api/tickets/{ticket_id}/comments"))
            .header("Authorization", format!("Bearer {viewer_session}"))
            .send()
            .await;
        resp.assert_status_is_ok();
        let body: serde_json::Value = resp.json().await.value().deserialize();
        let arr = body.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["body"], "admin note");
    }

    /// Wiki: 編集権限を持つアカウントのみがページ作成・改訂でき、閲覧権限が
    /// あれば履歴を保持したまま最新版を取得できることを確認する。
    /// (未ログイン401・無許可403・許可済み成功、既存のコメント/チケットと
    /// 同じ権限モデルの検証パターン)。
    #[tokio::test]
    async fn wiki_page_lifecycle_is_gated_by_project_access_and_keeps_revision_history() {
        let state = make_state("wiki-lifecycle", true).await;
        let data_root = state.data_root.clone();
        let admin = admin_token(&state);
        let app = build_routes(state);
        let client = TestClient::new(app);

        let created = client
            .post("/api/projects")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "name": "wiki-proj" }))
            .send()
            .await;
        created.assert_status(poem::http::StatusCode::CREATED);
        let project_id = created.json().await.value().deserialize::<serde_json::Value>()["id"].as_u64().unwrap();

        // 未ログインでのページ作成は401。
        client
            .post(format!("/api/projects/{project_id}/wiki"))
            .body_json(&serde_json::json!({ "slug": "start", "title": "Start", "body": "v1" }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::UNAUTHORIZED);

        // 管理者(常に編集可)でページを作成。
        let page = client
            .post(format!("/api/projects/{project_id}/wiki"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "slug": "start", "title": "Start", "body": "v1" }))
            .send()
            .await;
        page.assert_status(poem::http::StatusCode::CREATED);
        let page_id = page.json().await.value().deserialize::<serde_json::Value>()["id"].as_u64().unwrap();

        // 同じslugでの再作成は400(プロジェクト内で一意)。
        client
            .post(format!("/api/projects/{project_id}/wiki"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "slug": "start", "title": "dup", "body": "v1" }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::BAD_REQUEST);

        // editが無い一般ユーザーは改訂できない(403)。
        let stranger_state = AppState { data_root: data_root.clone(), auth: Arc::new(auth::AuthStore::default()), admin_email: ADMIN_EMAIL.to_string(), smtp: None, accounts_locked: true, backend: std::sync::Arc::new(storage::LocalFsBackend) };
        let stranger_session = stranger_state.auth.create_session("stranger@example.com");
        let stranger_app = build_routes(stranger_state);
        let stranger_client = TestClient::new(stranger_app);
        stranger_client
            .put(format!("/api/wiki/{page_id}"))
            .header("Authorization", format!("Bearer {stranger_session}"))
            .body_json(&serde_json::json!({ "body": "hacked" }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::FORBIDDEN);

        // 管理者が改訂すると履歴に旧版が残ったまま最新版が更新される。
        let updated = client
            .put(format!("/api/wiki/{page_id}"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "body": "v2" }))
            .send()
            .await;
        updated.assert_status_is_ok();
        let updated_body: serde_json::Value = updated.json().await.value().deserialize();
        let revisions = updated_body["revisions"].as_array().unwrap();
        assert_eq!(revisions.len(), 2);
        assert_eq!(revisions[0]["body"], "v1");
        assert_eq!(revisions[1]["body"], "v2");

        // GET /api/wiki/:id は最新の履歴を含めて取得できる。
        let fetched = client.get(format!("/api/wiki/{page_id}")).header("Authorization", format!("Bearer {admin}")).send().await;
        fetched.assert_status_is_ok();
        let fetched_body: serde_json::Value = fetched.json().await.value().deserialize();
        assert_eq!(fetched_body["revisions"].as_array().unwrap().len(), 2);

        // 管理者のみページを削除できる。
        client
            .delete(format!("/api/wiki/{page_id}"))
            .header("Authorization", format!("Bearer {stranger_session}"))
            .send()
            .await
            .assert_status(poem::http::StatusCode::UNAUTHORIZED);
        client.delete(format!("/api/wiki/{page_id}")).header("Authorization", format!("Bearer {admin}")).send().await.assert_status_is_ok();
        client
            .get(format!("/api/wiki/{page_id}"))
            .header("Authorization", format!("Bearer {admin}"))
            .send()
            .await
            .assert_status(poem::http::StatusCode::NOT_FOUND);
    }

    /// ガントチャート用フィールド(`start_date`/`due_date`/`done_ratio`)が
    /// 作成・更新の両方で保存され、`done_ratio`の範囲外の値(101)は
    /// `400`で拒否されることを確認する(Redmine機能ギャップ対応、
    /// 2026-07-23追加)。
    #[tokio::test]
    async fn ticket_gantt_fields_are_persisted_and_validated() {
        let state = make_state("ticket-gantt-fields", true).await;
        let admin = admin_token(&state);
        let app = build_routes(state);
        let client = TestClient::new(app);

        let created_project = client
            .post("/api/projects")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "name": "gantt-proj", "description": "" }))
            .send()
            .await;
        created_project.assert_status(poem::http::StatusCode::CREATED);
        let project_id = created_project.json().await.value().deserialize::<serde_json::Value>()["id"].as_u64().unwrap();

        // 作成時に start_date/due_date/done_ratio を指定できる。
        let created = client
            .post("/api/tickets")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({
                "title": "gantt ticket",
                "description": "",
                "project_id": project_id,
                "start_date": "2026-07-01",
                "due_date": "2026-07-31",
                "done_ratio": 40
            }))
            .send()
            .await;
        created.assert_status(poem::http::StatusCode::CREATED);
        let created_body: serde_json::Value = created.json().await.value().deserialize();
        assert_eq!(created_body["start_date"], "2026-07-01");
        assert_eq!(created_body["due_date"], "2026-07-31");
        assert_eq!(created_body["done_ratio"], 40);
        let ticket_id = created_body["id"].as_u64().unwrap();

        // 作成時に done_ratio が範囲外(101)だと400。
        client
            .post("/api/tickets")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "title": "bad", "description": "", "project_id": project_id, "done_ratio": 101 }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::BAD_REQUEST);

        // 更新でも同じフィールドを変更でき、範囲外は拒否される。
        let updated = client
            .put(format!("/api/tickets/{ticket_id}"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "done_ratio": 80, "due_date": "2026-08-15" }))
            .send()
            .await;
        updated.assert_status_is_ok();
        let updated_body: serde_json::Value = updated.json().await.value().deserialize();
        assert_eq!(updated_body["done_ratio"], 80);
        assert_eq!(updated_body["due_date"], "2026-08-15");
        assert_eq!(updated_body["start_date"], "2026-07-01", "unspecified fields must remain unchanged");

        client
            .put(format!("/api/tickets/{ticket_id}"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "done_ratio": 255 }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::BAD_REQUEST);
    }

    /// `GET /api/tickets`が`status`・`project_id`クエリパラメータで
    /// 絞り込めることを確認する(Redmine機能ギャップ対応、フィルタ・
    /// 検索機能、2026-07-23追加)。
    #[tokio::test]
    async fn list_tickets_supports_status_and_project_id_filters() {
        let state = make_state("ticket-list-filters", true).await;
        let admin = admin_token(&state);
        let app = build_routes(state);
        let client = TestClient::new(app);

        let mut project_ids = Vec::new();
        for name in ["proj-a", "proj-b"] {
            let created = client
                .post("/api/projects")
                .header("Authorization", format!("Bearer {admin}"))
                .body_json(&serde_json::json!({ "name": name, "description": "" }))
                .send()
                .await;
            created.assert_status(poem::http::StatusCode::CREATED);
            project_ids.push(created.json().await.value().deserialize::<serde_json::Value>()["id"].as_u64().unwrap());
        }
        let (proj_a, proj_b) = (project_ids[0], project_ids[1]);

        // proj_a に2件(open/closed)、proj_b に1件(open)を作成する。
        let make_ticket = |title: &'static str, project_id: u64| {
            let client = &client;
            let admin = admin.clone();
            async move {
                let resp = client
                    .post("/api/tickets")
                    .header("Authorization", format!("Bearer {admin}"))
                    .body_json(&serde_json::json!({ "title": title, "description": "", "project_id": project_id }))
                    .send()
                    .await;
                resp.assert_status(poem::http::StatusCode::CREATED);
                resp.json().await.value().deserialize::<serde_json::Value>()["id"].as_u64().unwrap()
            }
        };
        let a1 = make_ticket("a-open", proj_a).await;
        let a2 = make_ticket("a-to-close", proj_a).await;
        let _b1 = make_ticket("b-open", proj_b).await;

        client
            .put(format!("/api/tickets/{a2}"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "status": "closed" }))
            .send()
            .await
            .assert_status_is_ok();

        // project_id での絞り込み: proj_a のみで2件。
        let by_project =
            client.get(format!("/api/tickets?project_id={proj_a}")).header("Authorization", format!("Bearer {admin}")).send().await;
        by_project.assert_status_is_ok();
        let by_project_body: serde_json::Value = by_project.json().await.value().deserialize();
        assert_eq!(by_project_body.as_array().unwrap().len(), 2);

        // status=open での絞り込み: a-open と b-open の2件。
        let by_status = client.get("/api/tickets?status=open").header("Authorization", format!("Bearer {admin}")).send().await;
        by_status.assert_status_is_ok();
        let by_status_body: serde_json::Value = by_status.json().await.value().deserialize();
        assert_eq!(by_status_body.as_array().unwrap().len(), 2);

        // status=closed & project_id=proj_a の組み合わせ: a-to-close の1件のみ。
        let combined = client
            .get(format!("/api/tickets?status=closed&project_id={proj_a}"))
            .header("Authorization", format!("Bearer {admin}"))
            .send()
            .await;
        combined.assert_status_is_ok();
        let combined_body: serde_json::Value = combined.json().await.value().deserialize();
        let combined_array = combined_body.as_array().unwrap();
        assert_eq!(combined_array.len(), 1);
        assert_eq!(combined_array[0]["id"].as_u64().unwrap(), a2);
        let _ = a1;
    }

    /// トラッカー種別(Redmine機能ギャップ対応、2026-07-26追加)。作成時に
    /// `tracker`を指定できること・省略時は`bug`既定であること・
    /// `GET /api/tickets?tracker=...`での絞り込み・`PUT`での変更を確認する。
    #[tokio::test]
    async fn ticket_tracker_defaults_to_bug_is_filterable_and_updatable() {
        let state = make_state("ticket-tracker", true).await;
        let admin = admin_token(&state);
        let app = build_routes(state);
        let client = TestClient::new(app);

        let project = client
            .post("/api/projects")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "name": "tracker-proj" }))
            .send()
            .await;
        project.assert_status(poem::http::StatusCode::CREATED);
        let project_id = project.json().await.value().deserialize::<serde_json::Value>()["id"].as_u64().unwrap();

        // trackerを省略すると既定でbug。
        let default_ticket = client
            .post("/api/tickets")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "title": "no tracker specified", "description": "d", "project_id": project_id }))
            .send()
            .await;
        default_ticket.assert_status(poem::http::StatusCode::CREATED);
        let default_body: serde_json::Value = default_ticket.json().await.value().deserialize();
        assert_eq!(default_body["tracker"].as_str().unwrap(), "bug");
        let default_id = default_body["id"].as_u64().unwrap();

        // 明示的にfeatureを指定して作成。
        let feature_ticket = client
            .post("/api/tickets")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "title": "a feature", "description": "d", "project_id": project_id, "tracker": "feature" }))
            .send()
            .await;
        feature_ticket.assert_status(poem::http::StatusCode::CREATED);
        let feature_body: serde_json::Value = feature_ticket.json().await.value().deserialize();
        assert_eq!(feature_body["tracker"].as_str().unwrap(), "feature");
        let feature_id = feature_body["id"].as_u64().unwrap();

        // tracker=featureで絞り込むと1件のみ。
        let filtered = client.get("/api/tickets?tracker=feature").header("Authorization", format!("Bearer {admin}")).send().await;
        filtered.assert_status_is_ok();
        let filtered_body: serde_json::Value = filtered.json().await.value().deserialize();
        let filtered_array = filtered_body.as_array().unwrap();
        assert_eq!(filtered_array.len(), 1);
        assert_eq!(filtered_array[0]["id"].as_u64().unwrap(), feature_id);

        // PUTでtrackerをsupportへ変更できる。
        let updated = client
            .put(format!("/api/tickets/{default_id}"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "tracker": "support" }))
            .send()
            .await;
        updated.assert_status_is_ok();
        let updated_body: serde_json::Value = updated.json().await.value().deserialize();
        assert_eq!(updated_body["tracker"].as_str().unwrap(), "support");
    }

    /// 担当者(assignee)フィールドのライフサイクル: 未登録メールアドレス
    /// を指定した作成・更新はいずれも400、登録済みアカウント
    /// (管理者以外)を指定した作成は成功、`assignee`クエリでの絞り込み、
    /// PUTでの担当者変更、を一気通貫で確認する(2026-07-27追加)。
    #[tokio::test]
    async fn ticket_assignee_must_be_a_registered_account_and_is_filterable() {
        let state = make_state("ticket-assignee", false).await;
        let admin = admin_token(&state);
        let app = build_routes(state);
        let client = TestClient::new(app);

        let project = client
            .post("/api/projects")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "name": "assignee-proj" }))
            .send()
            .await;
        project.assert_status(poem::http::StatusCode::CREATED);
        let project_id = project.json().await.value().deserialize::<serde_json::Value>()["id"].as_u64().unwrap();

        // 未登録メールアドレスをassigneeに指定した作成は400。
        let rejected = client
            .post("/api/tickets")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "title": "t", "description": "d", "project_id": project_id, "assignee": "nobody@example.com" }))
            .send()
            .await;
        rejected.assert_status(poem::http::StatusCode::BAD_REQUEST);

        // 管理者メールアドレスは常に有効な担当者として指定できる。
        let admin_assigned = client
            .post("/api/tickets")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "title": "assigned to admin", "description": "d", "project_id": project_id, "assignee": ADMIN_EMAIL }))
            .send()
            .await;
        admin_assigned.assert_status(poem::http::StatusCode::CREATED);
        let admin_assigned_body: serde_json::Value = admin_assigned.json().await.value().deserialize();
        assert_eq!(admin_assigned_body["assignee"].as_str().unwrap(), ADMIN_EMAIL);

        // 一般アカウントを登録した上で、そのメールアドレスを担当者に
        // 指定した作成が成功すること。
        let dev_email = "dev@example.com";
        let register = client
            .post("/api/accounts")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "email": dev_email }))
            .send()
            .await;
        register.assert_status(poem::http::StatusCode::CREATED);

        let dev_assigned = client
            .post("/api/tickets")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "title": "assigned to dev", "description": "d", "project_id": project_id, "assignee": dev_email }))
            .send()
            .await;
        dev_assigned.assert_status(poem::http::StatusCode::CREATED);
        let dev_assigned_body: serde_json::Value = dev_assigned.json().await.value().deserialize();
        let dev_ticket_id = dev_assigned_body["id"].as_u64().unwrap();
        assert_eq!(dev_assigned_body["assignee"].as_str().unwrap(), dev_email);

        // assignee=dev@example.comで絞り込むと1件のみ。
        let filtered = client.get(format!("/api/tickets?assignee={dev_email}")).header("Authorization", format!("Bearer {admin}")).send().await;
        filtered.assert_status_is_ok();
        let filtered_body: serde_json::Value = filtered.json().await.value().deserialize();
        let filtered_array = filtered_body.as_array().unwrap();
        assert_eq!(filtered_array.len(), 1);
        assert_eq!(filtered_array[0]["id"].as_u64().unwrap(), dev_ticket_id);

        // PUTで未登録メールアドレスへの担当者変更を試みると400。
        let reject_update = client
            .put(format!("/api/tickets/{dev_ticket_id}"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "assignee": "still-nobody@example.com" }))
            .send()
            .await;
        reject_update.assert_status(poem::http::StatusCode::BAD_REQUEST);

        // PUTで管理者へ担当者を付け替えると成功する。
        let update = client
            .put(format!("/api/tickets/{dev_ticket_id}"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "assignee": ADMIN_EMAIL }))
            .send()
            .await;
        update.assert_status_is_ok();
        let update_body: serde_json::Value = update.json().await.value().deserialize();
        assert_eq!(update_body["assignee"].as_str().unwrap(), ADMIN_EMAIL);
    }

    /// 課題関連(`blocks`/`duplicates`/`precedes`)のライフサイクル: 作成・
    /// 一覧(from/to双方の立場で見えること)・自己参照拒否・存在しない
    /// `to_ticket_id`拒否・重複関連拒否・削除・権限ゲート(未ログイン401、
    /// 無許可403)を一気通貫で確認する。
    #[tokio::test]
    async fn issue_relation_lifecycle_and_access_gating() {
        let state = make_state("relation-lifecycle", true).await;
        let data_root = state.data_root.clone();
        let admin = admin_token(&state);
        let app = build_routes(state);
        let client = TestClient::new(app);

        let project = client
            .post("/api/projects")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "name": "relation-proj" }))
            .send()
            .await;
        project.assert_status(poem::http::StatusCode::CREATED);
        let project_id = project.json().await.value().deserialize::<serde_json::Value>()["id"].as_u64().unwrap();

        let mk_ticket = |title: &'static str| {
            let client = &client;
            let admin = &admin;
            async move {
                let created = client
                    .post("/api/tickets")
                    .header("Authorization", format!("Bearer {admin}"))
                    .body_json(&serde_json::json!({ "title": title, "description": "d", "project_id": project_id }))
                    .send()
                    .await;
                created.assert_status(poem::http::StatusCode::CREATED);
                created.json().await.value().deserialize::<serde_json::Value>()["id"].as_u64().unwrap()
            }
        };
        let blocker_id = mk_ticket("blocker").await;
        let blocked_id = mk_ticket("blocked").await;

        // 未ログインでの関連作成は401。
        client
            .post(format!("/api/tickets/{blocker_id}/relations"))
            .body_json(&serde_json::json!({ "to_ticket_id": blocked_id, "kind": "blocks" }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::UNAUTHORIZED);

        // editが無い一般ユーザーは403。
        let stranger_state = AppState { data_root: data_root.clone(), auth: Arc::new(auth::AuthStore::default()), admin_email: ADMIN_EMAIL.to_string(), smtp: None, accounts_locked: true, backend: std::sync::Arc::new(storage::LocalFsBackend) };
        let stranger_session = stranger_state.auth.create_session("stranger@example.com");
        let stranger_app = build_routes(stranger_state);
        let stranger_client = TestClient::new(stranger_app);
        stranger_client
            .post(format!("/api/tickets/{blocker_id}/relations"))
            .header("Authorization", format!("Bearer {stranger_session}"))
            .body_json(&serde_json::json!({ "to_ticket_id": blocked_id, "kind": "blocks" }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::FORBIDDEN);

        // 自己参照は400。
        client
            .post(format!("/api/tickets/{blocker_id}/relations"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "to_ticket_id": blocker_id, "kind": "blocks" }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::BAD_REQUEST);

        // 存在しないto_ticket_idは400。
        client
            .post(format!("/api/tickets/{blocker_id}/relations"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "to_ticket_id": 999999, "kind": "blocks" }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::BAD_REQUEST);

        // 正常な関連作成。
        let created = client
            .post(format!("/api/tickets/{blocker_id}/relations"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "to_ticket_id": blocked_id, "kind": "blocks" }))
            .send()
            .await;
        created.assert_status(poem::http::StatusCode::CREATED);
        let relation_id = created.json().await.value().deserialize::<serde_json::Value>()["id"].as_u64().unwrap();

        // 同じ組み合わせの再作成は重複として400。
        client
            .post(format!("/api/tickets/{blocker_id}/relations"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "to_ticket_id": blocked_id, "kind": "blocks" }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::BAD_REQUEST);

        // from側・to側どちらのチケットから見ても一覧に現れる。
        let from_side = client.get(format!("/api/tickets/{blocker_id}/relations")).header("Authorization", format!("Bearer {admin}")).send().await;
        from_side.assert_status_is_ok();
        let from_side_body: serde_json::Value = from_side.json().await.value().deserialize();
        assert_eq!(from_side_body.as_array().unwrap().len(), 1);

        let to_side = client.get(format!("/api/tickets/{blocked_id}/relations")).header("Authorization", format!("Bearer {admin}")).send().await;
        to_side.assert_status_is_ok();
        let to_side_body: serde_json::Value = to_side.json().await.value().deserialize();
        assert_eq!(to_side_body.as_array().unwrap().len(), 1);

        // 削除。
        client.delete(format!("/api/relations/{relation_id}")).header("Authorization", format!("Bearer {admin}")).send().await.assert_status_is_ok();
        let after_delete = client.get(format!("/api/tickets/{blocker_id}/relations")).header("Authorization", format!("Bearer {admin}")).send().await;
        after_delete.assert_status_is_ok();
        let after_delete_body: serde_json::Value = after_delete.json().await.value().deserialize();
        assert_eq!(after_delete_body.as_array().unwrap().len(), 0);
    }

    /// 作業時間記録のライフサイクル: 投稿・一覧・`hours`の範囲外拒否
    /// (0以下・24超)・投稿者本人または管理者のみ削除可・権限ゲート
    /// (未ログイン401、無許可403)を確認する。
    #[tokio::test]
    async fn time_entry_lifecycle_validates_hours_and_gates_deletion() {
        let state = make_state("time-entry-lifecycle", true).await;
        let data_root = state.data_root.clone();
        let admin = admin_token(&state);
        let app = build_routes(state);
        let client = TestClient::new(app);

        let project = client
            .post("/api/projects")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "name": "time-proj" }))
            .send()
            .await;
        project.assert_status(poem::http::StatusCode::CREATED);
        let project_id = project.json().await.value().deserialize::<serde_json::Value>()["id"].as_u64().unwrap();

        let ticket = client
            .post("/api/tickets")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "title": "t", "description": "d", "project_id": project_id }))
            .send()
            .await;
        ticket.assert_status(poem::http::StatusCode::CREATED);
        let ticket_id = ticket.json().await.value().deserialize::<serde_json::Value>()["id"].as_u64().unwrap();

        // 未ログインでの記録は401。
        client
            .post(format!("/api/tickets/{ticket_id}/time_entries"))
            .body_json(&serde_json::json!({ "hours": 1.0, "activity": "Development", "spent_on": "2026-07-26" }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::UNAUTHORIZED);

        // editが無い一般ユーザーは403。
        let stranger_state = AppState { data_root: data_root.clone(), auth: Arc::new(auth::AuthStore::default()), admin_email: ADMIN_EMAIL.to_string(), smtp: None, accounts_locked: true, backend: std::sync::Arc::new(storage::LocalFsBackend) };
        let stranger_session = stranger_state.auth.create_session("stranger@example.com");
        let stranger_app = build_routes(stranger_state);
        let stranger_client = TestClient::new(stranger_app);
        stranger_client
            .post(format!("/api/tickets/{ticket_id}/time_entries"))
            .header("Authorization", format!("Bearer {stranger_session}"))
            .body_json(&serde_json::json!({ "hours": 1.0, "activity": "Development", "spent_on": "2026-07-26" }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::FORBIDDEN);

        // hours=0は400。
        client
            .post(format!("/api/tickets/{ticket_id}/time_entries"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "hours": 0.0, "activity": "Development", "spent_on": "2026-07-26" }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::BAD_REQUEST);

        // hours=25は400(24超過)。
        client
            .post(format!("/api/tickets/{ticket_id}/time_entries"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "hours": 25.0, "activity": "Development", "spent_on": "2026-07-26" }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::BAD_REQUEST);

        // 正常な記録作成(管理者本人として)。
        let created = client
            .post(format!("/api/tickets/{ticket_id}/time_entries"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "hours": 2.5, "activity": "Development", "comments": "fixed it", "spent_on": "2026-07-26" }))
            .send()
            .await;
        created.assert_status(poem::http::StatusCode::CREATED);
        let entry_id = created.json().await.value().deserialize::<serde_json::Value>()["id"].as_u64().unwrap();

        let listed = client.get(format!("/api/tickets/{ticket_id}/time_entries")).header("Authorization", format!("Bearer {admin}")).send().await;
        listed.assert_status_is_ok();
        let listed_body: serde_json::Value = listed.json().await.value().deserialize();
        assert_eq!(listed_body.as_array().unwrap().len(), 1);

        // メンバー(editを持つ)による他人の記録削除は403(投稿者でも管理者でもない)。
        let mut config = access::load(&data_root, project_id, &storage::LocalFsBackend).await;
        config.accounts.insert("member@example.com".to_string(), access::AccountPermission { allow_view: true, allow_edit: true, allow_manage_members: false });
        access::save(&data_root, project_id, &config, &storage::LocalFsBackend).await.unwrap();
        let member_state = AppState { data_root: data_root.clone(), auth: Arc::new(auth::AuthStore::default()), admin_email: ADMIN_EMAIL.to_string(), smtp: None, accounts_locked: true, backend: std::sync::Arc::new(storage::LocalFsBackend) };
        let member_session = member_state.auth.create_session("member@example.com");
        let member_app = build_routes(member_state);
        let member_client = TestClient::new(member_app);
        member_client
            .delete(format!("/api/time_entries/{entry_id}"))
            .header("Authorization", format!("Bearer {member_session}"))
            .send()
            .await
            .assert_status(poem::http::StatusCode::FORBIDDEN);

        // 管理者による削除は成功。
        client.delete(format!("/api/time_entries/{entry_id}")).header("Authorization", format!("Bearer {admin}")).send().await.assert_status_is_ok();
        let after_delete = client.get(format!("/api/tickets/{ticket_id}/time_entries")).header("Authorization", format!("Bearer {admin}")).send().await;
        after_delete.assert_status_is_ok();
        let after_delete_body: serde_json::Value = after_delete.json().await.value().deserialize();
        assert_eq!(after_delete_body.as_array().unwrap().len(), 0);
    }

    /// カスタムフィールド(2026-07-31追加): プロジェクトの`custom_field_defs`に
    /// 定義したキーのみチケットに設定できること、未定義のキーは作成・更新
    /// いずれも`400`で拒否されること、定義済みキーの値は往復して読めることを
    /// 実HTTPリクエストで確認する。
    #[tokio::test]
    async fn ticket_custom_fields_must_be_defined_on_the_project() {
        let state = make_state("custom-fields", false).await;
        let admin = admin_token(&state);
        let app = build_routes(state);
        let client = TestClient::new(app);

        let project = client
            .post("/api/projects")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "name": "cf-proj", "custom_field_defs": ["severity", "customer"] }))
            .send()
            .await;
        project.assert_status(poem::http::StatusCode::CREATED);
        let project_id = project.json().await.value().deserialize::<serde_json::Value>()["id"].as_u64().unwrap();

        // 未定義のキーを含む作成は400。
        let rejected = client
            .post("/api/tickets")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "title": "t", "description": "d", "project_id": project_id, "custom_fields": {"unknown_field": "x"} }))
            .send()
            .await;
        rejected.assert_status(poem::http::StatusCode::BAD_REQUEST);

        // 定義済みキーのみの作成は成功し、値が往復する。
        let created = client
            .post("/api/tickets")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "title": "t", "description": "d", "project_id": project_id, "custom_fields": {"severity": "high"} }))
            .send()
            .await;
        created.assert_status(poem::http::StatusCode::CREATED);
        let created_body: serde_json::Value = created.json().await.value().deserialize();
        let ticket_id = created_body["id"].as_u64().unwrap();
        assert_eq!(created_body["custom_fields"]["severity"].as_str().unwrap(), "high");

        // 更新時に未定義キーを渡すと400、既存のticketは変更されない。
        let update_rejected = client
            .put(format!("/api/tickets/{ticket_id}"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "custom_fields": {"not_defined": "y"} }))
            .send()
            .await;
        update_rejected.assert_status(poem::http::StatusCode::BAD_REQUEST);

        // 定義済みキーでの更新は成功する。
        let updated = client
            .put(format!("/api/tickets/{ticket_id}"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "custom_fields": {"severity": "low", "customer": "acme"} }))
            .send()
            .await;
        updated.assert_status_is_ok();
        let updated_body: serde_json::Value = updated.json().await.value().deserialize();
        assert_eq!(updated_body["custom_fields"]["severity"].as_str().unwrap(), "low");
        assert_eq!(updated_body["custom_fields"]["customer"].as_str().unwrap(), "acme");
    }

    /// 名前付きロールプリセット(2026-07-31追加): `role: "developer"`を
    /// 指定した承認が`allow_view`/`allow_edit`を実際に付与すること、
    /// `role: "manager"`が`allow_manage_members`まで付与すること
    /// (これはグローバル管理者による承認でのみ許可される既存の権限昇格
    /// 防止ロジックと組み合わさる)、未知のロール名が`400`になることを
    /// 実HTTPリクエストで確認する。
    #[tokio::test]
    async fn role_preset_expands_to_the_expected_permission_flags() {
        let state = make_state("role-preset", false).await;
        let data_root = state.data_root.clone();
        let admin = admin_token(&state);
        let app = build_routes(state);
        let client = TestClient::new(app);

        let project = client.post("/api/projects").header("Authorization", format!("Bearer {admin}")).body_json(&serde_json::json!({ "name": "role-proj" })).send().await;
        project.assert_status(poem::http::StatusCode::CREATED);
        let project_id = project.json().await.value().deserialize::<serde_json::Value>()["id"].as_u64().unwrap();

        // 自己申請のレスポンス自体はプレーンテキスト("request submitted")
        // なので、発行された`request_id`は既存テストと同じく
        // `accounts::AccountStore::pending_requests`を直接読んで取得する。
        async fn latest_pending_request_id(data_root: &std::path::Path) -> String {
            let store = accounts::load(data_root, &storage::LocalFsBackend).await;
            store.pending_requests.last().expect("a pending request must exist").id.clone()
        }

        // developerロールでの承認申請 → allow_view/allow_edit=true、
        // allow_manage_members=falseが実際に付与されることを、承認後の
        // AccessConfigを直接読んで確認する(HTTPレスポンス自体は
        // "approved"という文字列のみを返す設計のため)。
        client
            .post("/api/accounts/request")
            .body_json(&serde_json::json!({ "email": "dev@example.com", "project_id": project_id }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::CREATED);
        let request_id = latest_pending_request_id(&data_root).await;

        client
            .post(format!("/api/accounts/requests/{request_id}/decide"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "approve": true, "project_id": project_id, "role": "developer" }))
            .send()
            .await
            .assert_status_is_ok();

        let config = access::load(&data_root, project_id, &storage::LocalFsBackend).await;
        let perm = config.accounts.get("dev@example.com").expect("developer role must have inserted an AccountPermission entry");
        assert!(perm.allow_view, "developer role must grant allow_view");
        assert!(perm.allow_edit, "developer role must grant allow_edit");
        assert!(!perm.allow_manage_members, "developer role must NOT grant allow_manage_members");

        // managerロールでの承認申請 → allow_manage_membersまで付与される
        // (グローバル管理者による承認のため権限昇格チェックは通る)。
        client
            .post("/api/accounts/request")
            .body_json(&serde_json::json!({ "email": "boss@example.com", "project_id": project_id }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::CREATED);
        let manager_request_id = latest_pending_request_id(&data_root).await;
        client
            .post(format!("/api/accounts/requests/{manager_request_id}/decide"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "approve": true, "project_id": project_id, "role": "manager" }))
            .send()
            .await
            .assert_status_is_ok();
        let config2 = access::load(&data_root, project_id, &storage::LocalFsBackend).await;
        let manager_perm = config2.accounts.get("boss@example.com").expect("manager role must have inserted an AccountPermission entry");
        assert!(manager_perm.allow_view && manager_perm.allow_edit && manager_perm.allow_manage_members, "manager role must grant all three flags");

        // 未知のロール名は400。
        client
            .post("/api/accounts/request")
            .body_json(&serde_json::json!({ "email": "someone@example.com", "project_id": project_id }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::CREATED);
        let request2_id = latest_pending_request_id(&data_root).await;
        let bad_role = client
            .post(format!("/api/accounts/requests/{request2_id}/decide"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "approve": true, "project_id": project_id, "role": "superuser" }))
            .send()
            .await;
        bad_role.assert_status(poem::http::StatusCode::BAD_REQUEST);
    }

    /// 保存済みクエリ(2026-07-31追加): 作成→一覧(自分のもののみ)→実行
    /// (`GET /api/tickets`と同じ絞り込み条件が適用される)→削除、を
    /// 実HTTPリクエストで一気通貫に確認する。
    #[tokio::test]
    async fn saved_query_lifecycle_creates_lists_runs_and_deletes() {
        let state = make_state("saved-queries", false).await;
        let admin = admin_token(&state);
        let stranger_token = state.auth.create_session("stranger@example.com");
        let app = build_routes(state);
        let client = TestClient::new(app);

        let project = client.post("/api/projects").header("Authorization", format!("Bearer {admin}")).body_json(&serde_json::json!({ "name": "sq-proj" })).send().await;
        project.assert_status(poem::http::StatusCode::CREATED);
        let project_id = project.json().await.value().deserialize::<serde_json::Value>()["id"].as_u64().unwrap();

        // 未ログインでの保存済みクエリ作成は401。
        client.post("/api/saved_queries").body_json(&serde_json::json!({ "name": "x" })).send().await.assert_status(poem::http::StatusCode::UNAUTHORIZED);

        // open状態のチケットを1件・closed状態のチケットを1件作成。
        let open_ticket = client
            .post("/api/tickets")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "title": "open one", "description": "d", "project_id": project_id }))
            .send()
            .await;
        open_ticket.assert_status(poem::http::StatusCode::CREATED);

        let closed_ticket = client
            .post("/api/tickets")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "title": "closed one", "description": "d", "project_id": project_id }))
            .send()
            .await;
        closed_ticket.assert_status(poem::http::StatusCode::CREATED);
        let closed_id = closed_ticket.json().await.value().deserialize::<serde_json::Value>()["id"].as_u64().unwrap();
        client
            .put(format!("/api/tickets/{closed_id}"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "status": "closed" }))
            .send()
            .await
            .assert_status_is_ok();

        // 「このプロジェクトのopenチケットのみ」を保存済みクエリとして保存。
        let created = client
            .post("/api/saved_queries")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "name": "my open tickets", "project_id": project_id, "status": "open" }))
            .send()
            .await;
        created.assert_status(poem::http::StatusCode::CREATED);
        let created_body: serde_json::Value = created.json().await.value().deserialize();
        let query_id = created_body["id"].as_u64().unwrap();

        // 一覧に1件反映される。
        let listed = client.get("/api/saved_queries").header("Authorization", format!("Bearer {admin}")).send().await;
        listed.assert_status_is_ok();
        let listed_body: serde_json::Value = listed.json().await.value().deserialize();
        assert_eq!(listed_body.as_array().unwrap().len(), 1);

        // 実行するとopenチケットのみが返る(closedチケットは含まれない)。
        let ran = client.get(format!("/api/saved_queries/{query_id}/run")).header("Authorization", format!("Bearer {admin}")).send().await;
        ran.assert_status_is_ok();
        let ran_body: serde_json::Value = ran.json().await.value().deserialize();
        let titles: Vec<&str> = ran_body.as_array().unwrap().iter().map(|t| t["title"].as_str().unwrap()).collect();
        assert_eq!(titles, vec!["open one"]);

        // 他人(strangerセッション)はこのクエリを一覧・実行できない
        // (自分のクエリのみ見える設計)——一覧は空、実行は403。
        let stranger_listed = client.get("/api/saved_queries").header("Authorization", format!("Bearer {stranger_token}")).send().await;
        stranger_listed.assert_status_is_ok();
        let stranger_listed_body: serde_json::Value = stranger_listed.json().await.value().deserialize();
        assert_eq!(stranger_listed_body.as_array().unwrap().len(), 0, "a stranger must not see another user's saved queries");
        client
            .get(format!("/api/saved_queries/{query_id}/run"))
            .header("Authorization", format!("Bearer {stranger_token}"))
            .send()
            .await
            .assert_status(poem::http::StatusCode::FORBIDDEN);

        // 削除後は一覧から消え、実行も404になる。
        client.delete(format!("/api/saved_queries/{query_id}")).header("Authorization", format!("Bearer {admin}")).send().await.assert_status_is_ok();
        let after_delete = client.get("/api/saved_queries").header("Authorization", format!("Bearer {admin}")).send().await;
        after_delete.assert_status_is_ok();
        let after_delete_body: serde_json::Value = after_delete.json().await.value().deserialize();
        assert_eq!(after_delete_body.as_array().unwrap().len(), 0);
        client.get(format!("/api/saved_queries/{query_id}/run")).header("Authorization", format!("Bearer {admin}")).send().await.assert_status(poem::http::StatusCode::NOT_FOUND);
    }

    /// チケットが担当者へ割り振られ、`resolved`(解決・報告者確認待ち)
    /// を経て`closed`(完了)へ遷移できることを確認する(Redmine本家の
    /// ワークフロー〈New→In Progress→Resolved→Closed〉相当、2026-07-31追加)。
    #[tokio::test]
    async fn ticket_can_be_assigned_and_progress_through_resolved_to_closed() {
        let state = make_state("ticket-assign-resolve", false).await;
        let admin = admin_token(&state);
        let data_root = state.data_root.clone();
        let app = build_routes(state);
        let client = poem::test::TestClient::new(app);

        client
            .post("/api/accounts")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "email": "dev@example.com" }))
            .send()
            .await
            .assert_status(poem::http::StatusCode::CREATED);

        let project = client
            .post("/api/projects")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "name": "assign-resolve" }))
            .send()
            .await;
        project.assert_status(poem::http::StatusCode::CREATED);
        let project_id = project.json().await.value().deserialize::<serde_json::Value>()["id"].as_u64().unwrap();

        // 作成時点で担当者を割り振れる。
        let created = client
            .post("/api/tickets")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "title": "fix bug", "description": "d", "project_id": project_id, "assignee": "dev@example.com" }))
            .send()
            .await;
        created.assert_status(poem::http::StatusCode::CREATED);
        let ticket: serde_json::Value = created.json().await.value().deserialize();
        assert_eq!(ticket["assignee"].as_str(), Some("dev@example.com"));
        let ticket_id = ticket["id"].as_u64().unwrap();

        // resolvedへ更新できる。
        let resolved = client
            .put(format!("/api/tickets/{ticket_id}"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "status": "resolved" }))
            .send()
            .await;
        resolved.assert_status_is_ok();
        let resolved_ticket: serde_json::Value = resolved.json().await.value().deserialize();
        assert_eq!(resolved_ticket["status"].as_str(), Some("resolved"));

        // status=resolvedで絞り込める。
        let filtered = client
            .get(format!("/api/tickets?status=resolved&project_id={project_id}"))
            .header("Authorization", format!("Bearer {admin}"))
            .send()
            .await;
        filtered.assert_status_is_ok();
        let filtered_body: serde_json::Value = filtered.json().await.value().deserialize();
        assert_eq!(filtered_body.as_array().unwrap().len(), 1);

        // 報告者確認後、closedへ最終遷移できる。
        let closed = client
            .put(format!("/api/tickets/{ticket_id}"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "status": "closed" }))
            .send()
            .await;
        closed.assert_status_is_ok();
        let closed_ticket: serde_json::Value = closed.json().await.value().deserialize();
        assert_eq!(closed_ticket["status"].as_str(), Some("closed"));

        // 担当者の付け替えも引き続き可能(既存機能の回帰確認)。
        let reassigned = client
            .put(format!("/api/tickets/{ticket_id}"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "assignee": "admin@example.com" }))
            .send()
            .await;
        reassigned.assert_status_is_ok();
        let reassigned_ticket: serde_json::Value = reassigned.json().await.value().deserialize();
        assert_eq!(reassigned_ticket["assignee"].as_str(), Some("admin@example.com"));

        let _ = data_root;
    }

    /// 優先度(Redmine本家と同じ5段階、2026-07-31追加)が既定値`normal`で
    /// 作成され、作成時の明示指定・更新時の変更がいずれも反映されることを
    /// 確認する。
    #[tokio::test]
    async fn ticket_priority_defaults_to_normal_and_is_settable_on_create_and_update() {
        let state = make_state("ticket-priority", true).await;
        let admin = admin_token(&state);
        let app = build_routes(state);
        let client = poem::test::TestClient::new(app);

        let project = client
            .post("/api/projects")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "name": "priority-proj" }))
            .send()
            .await;
        project.assert_status(poem::http::StatusCode::CREATED);
        let project_id = project.json().await.value().deserialize::<serde_json::Value>()["id"].as_u64().unwrap();

        // priority未指定時はnormalが既定値。
        let default_created = client
            .post("/api/tickets")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "title": "default priority", "description": "d", "project_id": project_id }))
            .send()
            .await;
        default_created.assert_status(poem::http::StatusCode::CREATED);
        let default_body: serde_json::Value = default_created.json().await.value().deserialize();
        assert_eq!(default_body["priority"].as_str(), Some("normal"));

        // 作成時にpriorityを明示指定できる。
        let urgent_created = client
            .post("/api/tickets")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "title": "urgent one", "description": "d", "project_id": project_id, "priority": "urgent" }))
            .send()
            .await;
        urgent_created.assert_status(poem::http::StatusCode::CREATED);
        let urgent_body: serde_json::Value = urgent_created.json().await.value().deserialize();
        assert_eq!(urgent_body["priority"].as_str(), Some("urgent"));
        let ticket_id = urgent_body["id"].as_u64().unwrap();

        // 更新でpriorityを変更できる。
        let updated = client
            .put(format!("/api/tickets/{ticket_id}"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "priority": "immediate" }))
            .send()
            .await;
        updated.assert_status_is_ok();
        let updated_body: serde_json::Value = updated.json().await.value().deserialize();
        assert_eq!(updated_body["priority"].as_str(), Some("immediate"));
    }

    /// チケットの`created_at`/`updated_at`(Redmine本家の一覧「更新日」列
    /// 相当、2026-07-31追加)が作成時に設定され、更新のたびに
    /// `updated_at`だけが変わり`created_at`は不変であることを確認する。
    #[tokio::test]
    async fn ticket_created_at_and_updated_at_are_tracked() {
        let state = make_state("ticket-timestamps", true).await;
        let admin = admin_token(&state);
        let app = build_routes(state);
        let client = poem::test::TestClient::new(app);

        let project = client
            .post("/api/projects")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "name": "timestamps-proj" }))
            .send()
            .await;
        project.assert_status(poem::http::StatusCode::CREATED);
        let project_id = project.json().await.value().deserialize::<serde_json::Value>()["id"].as_u64().unwrap();

        let created = client
            .post("/api/tickets")
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "title": "t", "description": "d", "project_id": project_id }))
            .send()
            .await;
        created.assert_status(poem::http::StatusCode::CREATED);
        let created_body: serde_json::Value = created.json().await.value().deserialize();
        let created_at = created_body["created_at"].as_str().unwrap().to_string();
        let first_updated_at = created_body["updated_at"].as_str().unwrap().to_string();
        assert!(!created_at.is_empty());
        assert_eq!(created_at, first_updated_at, "creation sets both timestamps to the same value");
        let ticket_id = created_body["id"].as_u64().unwrap();

        let updated = client
            .put(format!("/api/tickets/{ticket_id}"))
            .header("Authorization", format!("Bearer {admin}"))
            .body_json(&serde_json::json!({ "done_ratio": 50 }))
            .send()
            .await;
        updated.assert_status_is_ok();
        let updated_body: serde_json::Value = updated.json().await.value().deserialize();
        assert_eq!(updated_body["created_at"].as_str(), Some(created_at.as_str()), "created_at must not change on update");
        assert!(updated_body["updated_at"].as_str().is_some());
    }
}
