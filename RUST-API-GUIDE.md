# open-redmine Rust APIクライアント利用マニュアル

open-redmineは素のJSON over HTTP APIとして実装されており、専用のRustクレート
(SDK)は提供していません——`reqwest`等の一般的なHTTPクライアントから直接
呼び出せます。本マニュアルは、Rustアプリケーションから実際にこのAPIを
呼び出す際のパターンと具体的なコード例をまとめたものです。

**正直な開示**: 本マニュアルはAPIの呼び出し方(認証・エンドポイント一覧・
リクエスト/レスポンス形状)を説明するものであり、Redmine本家との機能網羅性
(カスタムフィールド・保存済みクエリ・ガントチャート/カレンダーGUI・
フォーラム/ニュース・SCM連携は未実装)を保証するものではありません。
実装済み機能の範囲は`CLAUDE.md`のHANDOFF・下記「対応していない機能」節を
参照してください。

## 目次

1. [認証(OTPログイン)](#1-認証otpログイン)
2. [共通のリクエストパターン](#2-共通のリクエストパターン)
3. [プロジェクト](#3-プロジェクト)
4. [チケット](#4-チケット)
5. [コメント](#5-コメント)
6. [チケットの関連(リレーション)](#6-チケットの関連リレーション)
7. [作業時間記録](#7-作業時間記録)
8. [添付ファイル](#8-添付ファイル)
9. [Wiki](#9-wiki)
10. [アカウント・アクセス申請](#10-アカウントアクセス申請)
11. [対応していない機能(正直な開示)](#11-対応していない機能正直な開示)

## 依存クレート(利用側のCargo.toml)

```toml
[dependencies]
reqwest = { version = "0.12", features = ["json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
```

## 1. 認証(OTPログイン)

open-redmineは固定パスワードを持たず、メールワンタイムパスワード(OTP)
方式のみでログインします。

```
POST /api/auth/request-otp   {"email": "you@example.com"}   -> 200 (OTPをメール送信)
POST /api/auth/verify-otp    {"email": "...", "code": "123456"} -> 200 {"token": "..."}
POST /api/auth/logout        (Authorization: Bearer <token>)  -> 200
```

以後の全リクエストに`Authorization: Bearer <token>`ヘッダを付ける
(認証不要のエンドポイントもあり、詳細は各節を参照)。

```rust
use serde::{Deserialize, Serialize};

const BASE_URL: &str = "http://127.0.0.1:8100"; // 本番: https://easy-web.tokyo/open-redmine

#[derive(Serialize)]
struct RequestOtp<'a> {
    email: &'a str,
}

#[derive(Serialize)]
struct VerifyOtp<'a> {
    email: &'a str,
    code: &'a str,
}

#[derive(Deserialize)]
struct VerifyOtpResponse {
    token: String,
}

/// OTPコードをメール送信させる(コード自体はメール本文にのみ含まれる。
/// 開発・検証用には`RSCHIKETTO_DEV_LOG_OTP=true`でサーバー起動すると
/// SMTP未設定でもサーバーログへOTPが出力される——本番では絶対に
/// 有効化しないこと、CLAUDE.md参照)。
async fn request_otp(client: &reqwest::Client, email: &str) -> anyhow::Result<()> {
    client
        .post(format!("{BASE_URL}/api/auth/request-otp"))
        .json(&RequestOtp { email })
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

/// メールで受け取ったOTPコードをセッショントークンへ交換する。
async fn verify_otp(client: &reqwest::Client, email: &str, code: &str) -> anyhow::Result<String> {
    let resp: VerifyOtpResponse = client
        .post(format!("{BASE_URL}/api/auth/verify-otp"))
        .json(&VerifyOtp { email, code })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(resp.token)
}
```

## 2. 共通のリクエストパターン

- **ベースURL**: 本番は`https://easy-web.tokyo/open-redmine`(デモ環境
  `https://easy-web.tokyo/open-redmine/demo`——現状は本番と同一バックエンド
  のエイリアス、独立データセットではない)。ローカル開発は
  `http://127.0.0.1:<RSCHIKETTO_PORT>`(既定`8100`)。
- **認証ヘッダ**: `Authorization: Bearer <token>`。トークンは
  `verify_otp`のレスポンスで取得。
- **ステータスコードの意味**: `401`=未ログイン、`403`=ログイン済みだが
  権限不足、`404`=対象が存在しない(先に存在確認、後に権限確認という順序
  のハンドラが多い——`404`と`403`のどちらが先に返るかはエンドポイントに
  よって異なる、詳細は`CLAUDE.md`の各HANDOFFエントリに個別記載あり)、
  `400`=バリデーション違反(不正なメールアドレス・範囲外の`done_ratio`・
  存在しない`project_id`等)。
- **アクセス制御**: 各プロジェクトへの閲覧(`View`)/編集(`Edit`)権限は
  プロジェクト単位で個別付与される(`access.rs`)。管理者は常に全操作が
  許可される。

共通のクライアントラッパー(以降の全例で`ApiClient`として再利用):

```rust
use reqwest::Client;
use serde::de::DeserializeOwned;

struct ApiClient {
    http: Client,
    base_url: String,
    token: String,
}

impl ApiClient {
    fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self { http: Client::new(), base_url: base_url.into(), token: token.into() }
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        Ok(self
            .http
            .get(format!("{}{}", self.base_url, path))
            .bearer_auth(&self.token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    async fn post_json<B: serde::Serialize, T: DeserializeOwned>(&self, path: &str, body: &B) -> anyhow::Result<T> {
        Ok(self
            .http
            .post(format!("{}{}", self.base_url, path))
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    async fn put_json<B: serde::Serialize, T: DeserializeOwned>(&self, path: &str, body: &B) -> anyhow::Result<T> {
        Ok(self
            .http
            .put(format!("{}{}", self.base_url, path))
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    async fn delete(&self, path: &str) -> anyhow::Result<()> {
        self.http.delete(format!("{}{}", self.base_url, path)).bearer_auth(&self.token).send().await?.error_for_status()?;
        Ok(())
    }
}
```

## 3. プロジェクト

```
POST   /api/projects              {"name","description","parent_id"(任意)} -> 201 Project  [管理者のみ]
GET    /api/projects                                                      -> 200 [Project]  [認証不要]
GET    /api/projects/:id                                                  -> 200 Project    [認証不要]
PUT    /api/projects/:id          (省略可フィールドのみ更新)               -> 200 Project    [管理者のみ]
DELETE /api/projects/:id                                                  -> 200             [管理者のみ]
GET    /api/projects/:id/children                                        -> 200 [Project]  [認証不要]
```

```rust
#[derive(serde::Deserialize, Debug)]
struct Project {
    id: u64,
    name: String,
    description: String,
    parent_id: Option<u64>,
    created_at: String,
    updated_at: String,
}

#[derive(serde::Serialize)]
struct CreateProjectReq<'a> {
    name: &'a str,
    description: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_id: Option<u64>,
}

async fn create_project(api: &ApiClient, name: &str, description: &str) -> anyhow::Result<Project> {
    api.post_json("/api/projects", &CreateProjectReq { name, description, parent_id: None }).await
}

async fn list_projects(api: &ApiClient) -> anyhow::Result<Vec<Project>> {
    api.get("/api/projects").await
}
```

## 4. チケット

```
POST /api/tickets            {"title","description","project_id","tracker"?,"start_date"?,"due_date"?,"done_ratio"?,"assignee"?} -> 201 Ticket
GET  /api/tickets             ?status=open|in_progress|closed  ?project_id=  ?tracker=  ?assignee=  -> 200 [Ticket]
GET  /api/tickets/:id                                                        -> 200 Ticket
PUT  /api/tickets/:id         (省略可フィールドのみ更新、statusも変更可)      -> 200 Ticket
```

`tracker`は`"bug"`/`"feature"`/`"support"`/`"task"`のいずれか(既定`bug`)。
`assignee`は管理者メールアドレス、または`accounts`に登録済みのメール
アドレスのいずれかでなければならず、それ以外は`400`。`done_ratio`は
0〜100の範囲外だと`400`。

```rust
#[derive(serde::Deserialize, Debug)]
struct Ticket {
    id: u64,
    project_id: u64,
    title: String,
    description: String,
    status: String, // "open" | "in_progress" | "closed"
    tracker: String, // "bug" | "feature" | "support" | "task"
    start_date: Option<String>,
    due_date: Option<String>,
    done_ratio: u8,
    assignee: Option<String>,
}

#[derive(serde::Serialize)]
struct CreateTicketReq<'a> {
    title: &'a str,
    description: &'a str,
    project_id: u64,
}

async fn create_ticket(api: &ApiClient, project_id: u64, title: &str, description: &str) -> anyhow::Result<Ticket> {
    api.post_json("/api/tickets", &CreateTicketReq { title, description, project_id }).await
}

async fn list_open_tickets(api: &ApiClient, project_id: u64) -> anyhow::Result<Vec<Ticket>> {
    api.get(&format!("/api/tickets?project_id={project_id}&status=open")).await
}

#[derive(serde::Serialize)]
struct UpdateTicketStatusReq<'a> {
    status: &'a str,
}

async fn close_ticket(api: &ApiClient, id: u64) -> anyhow::Result<Ticket> {
    api.put_json(&format!("/api/tickets/{id}"), &UpdateTicketStatusReq { status: "closed" }).await
}
```

## 5. コメント

```
POST /api/tickets/:id/comments  {"body": "..."}  -> 201 Comment
GET  /api/tickets/:id/comments                    -> 200 [Comment]
DELETE /api/comments/:id                           -> 200  [管理者または投稿者本人のみ]
```

## 6. チケットの関連(リレーション)

```
POST /api/tickets/:id/relations  {"to_ticket_id": u64, "kind": "blocks"|"duplicates"|"precedes"} -> 201 IssueRelation
GET  /api/tickets/:id/relations                                                                   -> 200 [IssueRelation]  (from/to双方の立場を含む)
DELETE /api/relations/:id                                                                          -> 200
```

自己参照・存在しない`to_ticket_id`・重複する`(from, to, kind)`はいずれも`400`。

## 7. 作業時間記録

```
POST /api/tickets/:id/time_entries  {"hours": f64, "activity": "...", "comments"?: "...", "spent_on": "YYYY-MM-DD"} -> 201 TimeEntry
GET  /api/tickets/:id/time_entries                                                                                   -> 200 [TimeEntry]
DELETE /api/time_entries/:id                                                                                          -> 200  [管理者または記録した本人のみ]
```

`hours`は0より大きく24以下でなければ`400`。

## 8. 添付ファイル

添付ファイルの作成のみ`multipart/form-data`(それ以外は全てJSON)。
`reqwest`の`multipart::Form`を使う。

```
POST /api/tickets/:id/attachments   (multipart/form-data、1パートのみ、フィールド名は任意) -> 201 Attachment
GET  /api/tickets/:id/attachments                                                          -> 200 [Attachment]
GET  /api/attachments/:id/download                                                          -> 200 (生バイナリ)
DELETE /api/attachments/:id                                                                  -> 200
```

```rust
async fn upload_attachment(api: &ApiClient, ticket_id: u64, filename: &str, bytes: Vec<u8>) -> anyhow::Result<serde_json::Value> {
    let part = reqwest::multipart::Part::bytes(bytes).file_name(filename.to_string());
    let form = reqwest::multipart::Form::new().part("file", part);
    let resp = api
        .http
        .post(format!("{}/api/tickets/{ticket_id}/attachments", api.base_url))
        .bearer_auth(&api.token)
        .multipart(form)
        .send()
        .await?
        .error_for_status()?;
    Ok(resp.json().await?)
}

async fn download_attachment(api: &ApiClient, attachment_id: u64) -> anyhow::Result<Vec<u8>> {
    let resp = api
        .http
        .get(format!("{}/api/attachments/{attachment_id}/download", api.base_url))
        .bearer_auth(&api.token)
        .send()
        .await?
        .error_for_status()?;
    Ok(resp.bytes().await?.to_vec())
}
```

## 9. Wiki

```
POST /api/projects/:id/wiki  {"slug","title","body"} -> 201 WikiPage  (project内でslugは一意)
GET  /api/projects/:id/wiki                            -> 200 [WikiPage]
GET  /api/wiki/:id                                      -> 200 WikiPage (最新リビジョン込み)
PUT  /api/wiki/:id           {"body","title"?}          -> 200 WikiPage (新規リビジョンを追記、旧内容は保持)
DELETE /api/wiki/:id                                    -> 200  [管理者のみ]
```

## 10. アカウント・アクセス申請

```
POST /api/accounts              {"email"}                     -> 201  [管理者のみ、ログイン可能アドレスの登録]
GET  /api/accounts                                             -> 200 [String]  [管理者のみ]
POST /api/accounts/request      {"email","project_id"?, ...}   -> 201  [認証不要、自己申請]
GET  /api/accounts/requests                                    -> 200 [AccessRequest]  [管理者のみ]
POST /api/accounts/requests/:id/decide  {"approve": bool, "project_id"?, "allow_view"?, "allow_edit"?, "allow_manage_members"?} -> 200  [管理者、または対象project_idのManageMembers権限を持つアカウント]
```

`allow_manage_members`の新規付与自体は、審査者がグローバル管理者でない
限り`403`で拒否される(権限昇格の防止、詳細は`CLAUDE.md`HANDOFF
2026-07-27参照)。

## 11. 対応していない機能(正直な開示)

以下はRedmine本家には存在するが、本APIには実装されていません
(呼び出しても存在しないエンドポイントとして`404`になります)。将来の
実装状況は`CLAUDE.md`の「次にすべきこと」を参照してください。

- カスタムフィールド
- 保存済みカスタムクエリ/フィルタ
- ガントチャート・カレンダーの集計・描画API(チケットの`start_date`/
  `due_date`/`done_ratio`フィールド自体は存在し、クライアント側で
  独自に描画することは可能)
- 名前付きロールプリセット(Manager/Developer/Reporter等、現状は
  `allow_view`/`allow_edit`/`allow_manage_members`の個別フラグのみ)
- フォーラム・ニュース・ドキュメントモジュール
- SCM(リポジトリ)連携
- ウォッチャー機能
- 通知メール(チケット更新時の自動メール送信)
