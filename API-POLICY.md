# API方針 (open-redmine)

正本は aon-co-jp/runo の `API-POLICY.md`。REST全廃・Connect/gRPC方針(2026-11-04)。

## 状況: API: REST全廃 → Connect (2026-11-07 移行済み)

公開APIは **Connect の単純呼び出しのみ**: `POST /open_redmine.v1.<Service>/<Method>`(`content-type: application/json`、本文はJSONオブジェクト、認証は従来どおり `Authorization: Bearer <token>`)。成功は常にHTTP 200、失敗は `{"code","message"}`(401/403/404/400/415/413 など)。リクエストJSONの `id` は対象リソースのid(例: `Comments/List` はチケットid、`Comments/Delete` はコメントid)、残りが本文/絞り込み条件。本文は重複キー・深さ32・256KB(添付のみ16MB)の上限つきで読む。**旧 `/api/...` は削除済み(404)**。

| 旧REST | Connect (`/open_redmine.v1.` 以下) |
|---|---|
| `GET /api/auth/config`, `POST /api/auth/{request-otp,verify-otp,logout}` | `Auth/{Config,RequestOtp,VerifyOtp,Logout}` |
| `GET·POST /api/accounts`, `POST /api/accounts/request`, `GET /api/accounts/requests`, `POST .../requests/:id/decide` | `Accounts/{List,Add,RequestAccess,ListRequests,DecideRequest}` |
| `/api/projects`, `/projects/:id`, `/children`, `/github/commits` | `Projects/{List,Create,Get,Update,Delete,ListChildren,ListGithubCommits}` |
| `/api/tickets`(絞り込みは本文の `status`/`project_id`/`tracker`/`assignee`), `/tickets/:id` | `Tickets/{List,Create,Get,Update}` |
| コメント | `Comments/{List,Create,Delete}` |
| 添付(multipart廃止→`file_name`/`content_type`/`data_base64`、ダウンロードも同形のJSON) | `Attachments/{List,Create,Download,Delete}` |
| 関連 / 作業時間 | `Relations/{List,Create,Delete}` / `TimeEntries/{List,Create,Delete}` |
| Wiki | `Wiki/{List,Create,Get,Update,Delete}` |
| 保存済みクエリ | `SavedQueries/{List,Create,Delete,Run}` |

- **移した: 42本**(上表)。**例外として残した: `POST /api/github/webhook`**(GitHubの規格・`X-Hub-Signature-256`署名検証)、`/healthz`、`/`・`/pkg/*`(画面・静的ファイル)。例外は `version-control` の台帳へ(理由: 第三者Webhook/ヘルスチェック/静的ファイル)。
- 実装: `src/connect.rs`(公開面)。認可・検証・永続化を持つ既存ハンドラは内部ルーター(`build_internal`)に残し、外へは出さない。呼び出し元(`web/` のWASM画面、文書)は同時に更新。Androidアプリ(`android/`)はWebView/疎通確認のみでAPI呼び出し無し。他リポジトリからの呼び出しは確認できず(`F:unoepository` を検索)。
- テスト: `cargo test` 96件成功(`connect_tests`: 旧REST404・401・415・400・重複キー400・正常200・添付のbase64往復)。実サーバー+ブラウザ(Browser pane)で プロジェクト/チケット/コメント/工数/添付(作成・ダウンロード)/Wiki を操作し、全て200を確認。
- **未了/正直な注意**: 本番(VPS)への反映は未実施(WSLリリースビルド→glibc確認→入れ替えが必要。前面のopen-web-serverのパスプレフィックス転送は `/open-redmine/open_redmine.v1.*` がそのまま通る想定だが未確認)。以下の旧REST表記の表・`RUST-API-GUIDE*.md` の `/api/...` は**廃止前の記述**で、上の対応表で読み替えること。

