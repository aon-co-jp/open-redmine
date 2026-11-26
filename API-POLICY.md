# API方針 (open-redmine)

正本は aon-co-jp/runo の `API-POLICY.md`。REST全廃・Connect/gRPC方針(2026-11-04)。

## 状況: API: REST全廃 → Connect (2026-11-07 移行済み)

公開APIは **Connect の単純呼び出しのみ**: `POST /open_redmine.<Service>/<Method>`(`content-type: application/json`、本文はJSONオブジェクト、認証は従来どおり `Authorization: Bearer <token>`)。成功は常にHTTP 200、失敗は `{"code","message"}`(401/403/404/400/415/413 など)。リクエストJSONの `id` は対象リソースのid(例: `Comments/List` はチケットid、`Comments/Delete` はコメントid)、残りが本文/絞り込み条件。本文は重複キー・深さ32・256KB(添付のみ16MB)の上限つきで読む。**旧 `/api/...` は削除済み(404)**。

| 旧REST | Connect (`/open_redmine.` 以下) |
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
- **未了/正直な注意**: 本番(VPS)への反映は未実施(WSLリリースビルド→glibc確認→入れ替えが必要。前面のopen-web-serverのパスプレフィックス転送は `/open-redmine/open_redmine.*` がそのまま通る想定だが未確認)。以下の旧REST表記の表・`RUST-API-GUIDE*.md` の `/api/...` は**廃止前の記述**で、上の対応表で読み替えること。



## 2026-11-26 Versionless化: 版つき(.v1)Connect経路を即時削除 / `.v1` removed, versionless only
**日本語**: 所有者の指示(2026-11-26)で、Connect のパッケージ名を `open_redmine.v1` から **`open_redmine`** に統一し、旧 `.v1` 経路は猶予なしで削除した(`POST /open_redmine.<Service>/<Method>`、42本、名前の変更のみで意味は同じ)。UI(`web/src/lib.rs`、WASM の `web/pkg` を再生成)・試験・文書を同時に直した。`cargo test` 96件成功(旧 `.v1` が404になる試験を追加)。本番(VPS、`open-redmine` 8100・`open-redmine-demo` 8113)へ WSL `Ubuntu-24.04`+`check-glibc.sh`(glibc 2.38)でビルドして入れ替え済み。バックアップ: `/root/open-redmine/target/release/rs-chiketto.before-v1del-20261126`、`/root/pkg.before-v1del-20261126`。実測: 新 `Projects/Create`(認証なし)401、旧 `.v1` 404、旧 `/api/projects` 404、`/healthz` 200、画面・`pkg/rs_red_web.js` 200、エラーログ0。最初の確認で `{}` 本文が400(検証が認証より先)だったため自動ロールバックが働いたが、確認条件が厳しすぎただけで、本文を直して再入れ替えし成功。デモ(8113)は認証なしで書けるのが従来どおりで、確認用に作った1件は削除済み。**未了**: ログイン後の通し操作の本番確認、ブラウザでの再確認(白画面が無いか。本番の `/` は200)。
**再開用メッセージ(別アカウントで貼り付け)**: `aon-co-jp/open-redmine` の続き。Connect は版なし `POST /open_redmine.<Service>/<Method>` に統一済み(`.v1` は削除・404)、`main` と本番に反映済み。新規clone して README・CLAUDE・PORTING を読み、未了(ログイン後の通し・ブラウザ確認)を確認なしで進める。決まり: 新しいRESTは作らない、秘密は書かない、VPSでビルドしない(WSL `Ubuntu-24.04` で `F:unops-build.sh`)、実機で確かめてから完了と書く、返答は日本語のみ。

## 2026-11-26 Versionless: the versioned (`.v1`) Connect routes were removed immediately
**English**: On the owner's instruction (2026-11-26) the Connect package was renamed from `open_redmine.v1` to **`open_redmine`** and the old `.v1` routes were deleted with no grace period (`POST /open_redmine.<Service>/<Method>`, 42 routes; names changed, meaning unchanged). The UI (`web/src/lib.rs`, with the WASM `web/pkg` regenerated), tests and docs were fixed together. `cargo test` passes 96 tests (a test that old `.v1` returns 404 was added). Production (VPS, `open-redmine` 8100 and `open-redmine-demo` 8113) was rebuilt in WSL `Ubuntu-24.04` with `check-glibc.sh` (needs glibc 2.38) and swapped in. Backups: `/root/open-redmine/target/release/rs-chiketto.before-v1del-20261126` and `/root/pkg.before-v1del-20261126`. Measured: new `Projects/Create` without auth 401, old `.v1` 404, old `/api/projects` 404, `/healthz` 200, UI and `pkg/rs_red_web.js` 200, error log 0. The first check saw 400 for a `{}` body (validation runs before auth), so the automatic rollback fired; the check was simply too strict, and after fixing it the swap was repeated successfully. The demo (8113) accepts writes without auth as before; the one test record I created was deleted. **Open**: authenticated end-to-end flows in production, and a browser re-check (no white screen; production `/` returns 200).
**Resume message (paste on another account)**: Continue `aon-co-jp/open-redmine`. Connect is unified on versionless `POST /open_redmine.<Service>/<Method>` (`.v1` removed, now 404), on `main` and in production. Clone fresh, read README, CLAUDE and PORTING, and carry on with the open items (authenticated flows, browser check) without asking. Rules: no new REST, never write secrets, never build on the VPS (WSL `Ubuntu-24.04` with `F:unops-build.sh`), verify on real systems before saying done, reply in Japanese only.
