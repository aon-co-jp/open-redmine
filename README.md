> Latest translations / 翻訳の最新版: [0-LATEST-TRANSLATIONS.md](0-LATEST-TRANSLATIONS.md)

# open-redmine

(旧名`RS-Chiketto`→2026-07-22に`RS-Red`→2026-07-27に`open-redmine`へ改名)

## Deployment / 運用先

- **Production/admin (本番/管理者用)**: <https://easy-web.tokyo/open-redmine/>
- **Demo (デモ用)**: <https://easy-web.tokyo/open-redmine/demo> (honest
  disclosure: currently an alias to the same backend as production, not an
  isolated demo dataset / 正直な開示: 現状は本番と同一バックエンドを指す
  エイリアスで、独立したデモ専用データはまだ無い)
- The `runo.tokyo/open-redmine` tenant entry was removed 2026-07-28,
  consolidating on easy-web.tokyo (same backend process, unaffected) /
  `runo.tokyo/open-redmine`のテナント登録は2026-07-28に削除し、
  easy-web.tokyoへ一本化した(バックエンドプロセス自体は同一・無停止)。

**開発開始日: 2026-07-21**(このリポジトリのGitHub作成日)

[Redmine](https://redmine.org/)のハイスピード・ハイセキュリティ・省メモリな
Rust+[poem](https://github.com/poem-web/poem)(RPoem)版。運用時はVPSレンタル
サーバー費用を安く抑えられる予定です。

> ⚠️ v0.1.0時点ではチケット(Issue)・プロジェクトのCRUD・Wiki・コメントに加え、
> トラッカー種別(Bug/Feature/Support/Task)・課題関連(blocks/duplicates/
> precedes)・作業時間記録(time tracking)まで(2026-07-26追加)。
> **2026-07-27追加**: チケットへの担当者(assignee)フィールド、
> プロジェクトマネージャーロール(グローバル管理者以外もプロジェクト単位
> でメンバー管理を行える権限、権限昇格は防止)。
> **2026-07-31追加**: カスタムフィールド・保存済みクエリ・ガントチャート/
> カレンダーGUI・名前付きロールプリセット(Manager/Developer/Reporter)・
> 優先度(Priority、5段階)・チケットステータス`resolved`・作成日時/
> 更新日時・カテゴリ(Category)。チケット一覧はテーブル形式(#/トラッカー/
> 優先度/カテゴリ/ステータス/件名/担当者/進捗%/期限/更新日)へ刷新し、
> ステータス・優先度・トラッカーの色分け、進捗バー、期限超過の赤字表示に
> 対応した。Redmine全体の機能網羅率は4〜5割程度まで前進(残る主な
> ギャップ: フォーラム/ニュース/ドキュメント/ファイルモジュール、SCM連携、
> ウォッチャー、通知メール、列クリックソート、カテゴリ/カスタムフィールド
> 定義のweb側管理UI)。詳細は`CLAUDE.md`参照。

## ブラウザGUI(`web/`、Rust→WebAssembly)

チケット管理を行うWEBアプリである以上GUIは基本機能——という方針で、
オンライン専用のブラウザフロントエンドを同梱している(Tauri・Node.js・
TypeScriptは不使用)。`cargo run`でサーバーを起動すると`GET /`で
自動的に配信される(`web/index.html`+`web/pkg/`が存在しない場合は
旧来のAPI概要ページへ自動フォールバック)。

- OTPログイン、プロジェクト一覧・作成、チケット一覧・作成・詳細表示・
  ステータス変更・コメント投稿、Wiki一覧・作成・閲覧に対応。
- **トラッカー種別・課題関連・作業時間記録のUI(2026-07-26追加)**:
  チケット作成フォームにトラッカー(Bug/Feature/Support/Task)選択、
  一覧・詳細にトラッカーバッジ表示。チケット詳細にはRelations
  (関連チケット、blocks/duplicates/precedes、追加・削除)・
  Time entries(作業時間記録、時間・作業分類・コメント・投稿者表示、
  追加・削除〈削除ボタンは投稿者本人にのみ表示、実際の許可判定は
  引き続きサーバー側〉)のセクションを追加。
- ピンチズームはブラウザ標準機能(`viewport`メタタグ)のため、
  Android/iOSのモバイルブラウザで特別な実装なしにそのまま動作する。
- **スマホ縦画面レスポンシブ対応・英語(日本語)ハイブリッド表示
  (2026-07-24追加)**: `@media (max-width: 600px)`でスマホ幅向けの
  余白調整、全ボタン・入力欄はタップ操作向けサイズ(44px以上)。
  UIラベルは「英語表記の直後に(日本語)」形式(例: `Login (ログイン)`)
  で常時両方表示。

### GUIのビルド方法

```bash
cd web
rustup target add wasm32-unknown-unknown  # 初回のみ
cargo install wasm-bindgen-cli             # 初回のみ、Cargo.lockのバージョンに合わせる
cargo build --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir pkg target/wasm32-unknown-unknown/debug/rs_red_web.wasm
```

## 再開用メッセージ(別アカウントでの再開 / 2026-11-15)

**日本語**
- **到達点**: REST全廃(Connect化)は完了し、`aon-co-jp/open-redmine` の `main` に反映済み。公開APIは `POST /open_redmine.<Service>/<Method>` の42本のみ(旧 `/api/...` は404)。例外は GitHub Webhook(`/api/github/webhook`、署名検証)・`/healthz`・画面と静的ファイル。実装は `src/connect.rs`(公開面)+ 内部ルーター `build_internal`(既存ハンドラ)、画面は `web/`(WASM、`rpc()`でConnect呼び出し、`web/pkg` は再生成して同梱)。
- **本番(VPS)**: 2026-11-08 に `6612f6b` を反映済み(`open-redmine` 8100・`open-redmine-demo` 8113、バックアップ `/root/rd-bak-20261108-065255`)。その後の依存更新(`13773b7` 脆弱性修正、`3551712` rand/hmac/sha2/reqwest/base64/thiserror 更新、いずれも `cargo test` 96件成功)の**VPS反映は未確認**。2026-11-15 の Android 実機確認では、新規ビルドの open-redmine が起動し `/healthz` 200。
- **未了**: (1) ログイン後の認証つき通し操作(画面→Connect)の本番確認、(2) 依存更新版の本番反映の有無の確認(VPSのバイナリ日時・`git log` と比較。ビルドはWSL、**VPSではビルドしない**)、(3) Android の open-redmine のブラウザ側確認、(4) `version-control` の例外台帳(`exceptions.toml`)へ上記3件の例外の追加依頼、(5) `RUST-API-GUIDE*.md` の `/api/...` 記述が旧REST表記のまま(冒頭に注記のみ。本文の書き換えは未了)。
- **注意**: `F:\runo\repository\open-redmine` の手元cloneには未コミット変更(README・install系)があるので、直接触らず新しくcloneして作業する。`git pull` で履歴を戻さない(`git fetch` + 新規clone)。`cargo fmt --all` 禁止。Rustのビルドは WSL(`CARGO_TARGET_DIR` はWSLのホーム)か `%USERPROFILE%\.cargo\bin` を前置きしたWindows。WASMは `web/` で `cargo build --release --target wasm32-unknown-unknown` → `wasm-bindgen --target web --out-dir pkg`(wasm-bindgen 0.2.126)。
- **再開するリポジトリ**: `aon-co-jp/open-redmine`(新規clone)。必要なら `F:\runo`(メタ索引。HANDOFFと `API-REST-REMAINING-20261106.md`)。

**貼り付け用メッセージ(日本語)**
> `aon-co-jp/open-redmine` の続き。REST全廃(Connect化、`POST /open_redmine.<Service>/<Method>`)は完了して main に反映済み、本番は 2026-11-08 に `6612f6b` を反映済み。まず `PORTING.md` と `CLAUDE.md` の「再開用メッセージ」を読むこと。やること: (1) 依存更新版(`13773b7`・`3551712`)が本番に入っているか確認し、入っていなければ WSL でリリースビルド→glibc 2.39 以下を確認→scp→バックアップ→入れ替え→`open-redmine`(8100)・`open-redmine-demo`(8113)を再起動→確認(失敗時ロールバック)、(2) ログイン後の通し操作をブラウザで確認、(3) `exceptions.toml` への例外登録を version-control のセッションへ依頼。`F:\runo\repository\open-redmine` の手元の未コミット変更は触らず、新規cloneで作業する。VPSでビルドしない。秘密は画面・コミットに出さない。チャットは日本語のみ。

**English**
- **Where things stand**: the REST abolition (Connect migration) is finished and on `main` of `aon-co-jp/open-redmine`. The only public API is 42 calls `POST /open_redmine.<Service>/<Method>` (old `/api/...` returns 404). Exceptions: the GitHub webhook (`/api/github/webhook`, signature-verified), `/healthz`, and the UI/static files. Code: `src/connect.rs` (public surface) + the internal router `build_internal` (existing handlers); the UI is `web/` (WASM calling Connect through `rpc()`; `web/pkg` is regenerated and committed).
- **Production (VPS)**: `6612f6b` was deployed on 2026-11-08 (`open-redmine` 8100, `open-redmine-demo` 8113, backup `/root/rd-bak-20261108-065255`). Whether the later dependency updates (`13773b7` security fixes, `3551712` rand/hmac/sha2/reqwest/base64/thiserror bumps; `cargo test` 96 passing) are deployed is **unverified**. On 2026-11-15 a freshly built open-redmine started on an Android device with `/healthz` 200.
- **Open**: (1) authenticated end-to-end use (login, UI, Connect) in production, (2) check whether the dependency-update build is live (compare the VPS binary time / `git log`; build in WSL, **never on the VPS**), (3) browser-side check of open-redmine on Android, (4) ask the version-control session to add the three exceptions to its ledger (`exceptions.toml`), (5) `RUST-API-GUIDE*.md` bodies still describe the old `/api/...` (only a banner was added).
- **Cautions**: the local clone `F:\runo\repository\open-redmine` has uncommitted changes (README, install scripts) - do not touch it, work in a fresh clone. Do not `git pull` over rewritten history (fetch + fresh clone). Never `cargo fmt --all`. Build Rust in WSL (`CARGO_TARGET_DIR` in the WSL home) or on Windows with `%USERPROFILE%\.cargo\bin` first in PATH. For WASM: in `web/`, `cargo build --release --target wasm32-unknown-unknown` then `wasm-bindgen --target web --out-dir pkg` (wasm-bindgen 0.2.126).
- **Repository to resume**: `aon-co-jp/open-redmine` (fresh clone). If needed, `F:\runo` (meta index: HANDOFF and `API-REST-REMAINING-20261106.md`).

**Paste-ready message (English)**
> Continue `aon-co-jp/open-redmine`. The REST abolition (Connect, `POST /open_redmine.<Service>/<Method>`) is done and on main; production got `6612f6b` on 2026-11-08. First read the resume message in `CLAUDE.md` and `PORTING.md`. To do: (1) check whether the dependency-update commits (`13773b7`, `3551712`) are live in production; if not, build in WSL, verify glibc <= 2.39, scp, back up, swap, restart `open-redmine` (8100) and `open-redmine-demo` (8113), verify (roll back on failure); (2) exercise login-then-use flows in a browser; (3) ask the version-control session to register the exceptions in `exceptions.toml`. Do not touch the uncommitted changes in `F:\runo\repository\open-redmine`; use a fresh clone. Never build on the VPS. Keep secrets out of screens and commits. Chat in Japanese only.

## API: REST全廃 → Connect (2026-11-07 移行済み)

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
- **本番反映**: 2026-11-08 に VPS へ反映済み(詳細は `PORTING.md` 末尾の記録)。以下の旧REST表記の表・`RUST-API-GUIDE*.md` の `/api/...` は**廃止前の記述**で、上の対応表で読み替えること。

## API エンドポイント

Rustから実際に呼び出す具体例(reqwestベースのコード付き)は
[RUST-API-GUIDE.md](RUST-API-GUIDE.md)([English](RUST-API-GUIDE-English.md))
を参照してください。

| メソッド / パス | 説明 |
| --- | --- |
| `GET /healthz` | ヘルスチェック |
| `POST /api/auth/request-otp` | ログイン用ワンタイムパスワードをメール送信 |
| `POST /api/auth/verify-otp` | OTPを検証してセッショントークンを発行 |
| `POST /api/auth/logout` | ログアウト(トークン失効) |
| `GET /api/accounts` / `POST /api/accounts` | 登録アカウント一覧取得 / 追加(管理者のみ) |
| `POST /api/accounts/request` | アカウント利用の自己申請(認証不要) |
| `GET /api/accounts/requests` | 保留中の自己申請一覧(管理者のみ) |
| `POST /api/accounts/requests/:id/decide` | 自己申請の承認/却下・プロジェクトへの閲覧/編集/メンバー管理権限付与(グローバル管理者、または対象プロジェクトの`allow_manage_members`を持つプロジェクトマネージャー。メンバー管理権限自体の新規付与はグローバル管理者のみ、2026-07-27追加) |
| `GET /api/projects` / `POST /api/projects` | プロジェクト一覧取得(認証不要) / 新規作成(管理者のみ、`parent_id`でサブプロジェクト化可能) |
| `GET /api/projects/:id` / `PUT /api/projects/:id` / `DELETE /api/projects/:id` | プロジェクト詳細取得(認証不要) / 更新・削除(管理者のみ、`parent_id`変更は循環参照を拒否) |
| `GET /api/projects/:id/children` | 直接の子プロジェクト一覧(認証不要) |
| `GET /api/tickets` / `POST /api/tickets` | チケット一覧取得(アクセス権のあるプロジェクトのみ、`status`/`project_id`/`tracker`/`assignee`クエリパラメータで絞り込み可能) / 新規作成(実在する`project_id`が必要、`tracker`〈`bug`/`feature`/`support`/`task`、省略時`bug`〉、`start_date`/`due_date`/`done_ratio`はガントチャート用の任意フィールド、`assignee`は登録済みアカウントのメールアドレスのみ指定可能) |
| `GET /api/tickets/:id` / `PUT /api/tickets/:id` | チケット詳細取得 / 更新(ステータス・`tracker`・`start_date`/`due_date`/`done_ratio`・`assignee`〈担当者〉の変更含む) |
| `GET /api/tickets/:id/comments` / `POST /api/tickets/:id/comments` | コメント一覧取得(閲覧権限が必要) / 投稿(編集権限が必要) |
| `DELETE /api/comments/:id` | コメント削除(管理者または投稿者本人のみ) |
| `GET /api/tickets/:id/relations` / `POST /api/tickets/:id/relations` | チケット間の関連(`blocks`/`duplicates`/`precedes`)一覧(from/to双方の立場で表示、閲覧権限が必要) / 新規作成(編集権限が必要、自己参照・存在しない相手・重複登録は`400`で拒否) |
| `DELETE /api/relations/:id` | 関連の削除(`from`側チケットが所属するプロジェクトへの編集権限が必要) |
| `GET /api/tickets/:id/time_entries` / `POST /api/tickets/:id/time_entries` | 作業時間記録一覧(閲覧権限が必要) / 新規作成(編集権限が必要、`hours`は0より大きく24以下、`activity`/`spent_on`必須) |
| `DELETE /api/time_entries/:id` | 作業時間記録の削除(管理者または記録した本人のみ) |
| `GET /api/projects/:id/wiki` / `POST /api/projects/:id/wiki` | プロジェクト配下のWikiページ一覧(閲覧権限が必要) / 新規作成(編集権限が必要、`slug`はプロジェクト内で一意) |
| `GET /api/wiki/:id` / `PUT /api/wiki/:id` | Wikiページ取得(改訂履歴含む、閲覧権限が必要) / 新しいリビジョンを追記(編集権限が必要、旧版は履歴に保持) |
| `DELETE /api/wiki/:id` | Wikiページ削除(管理者のみ) |

## DDNS運用(固定IPを持たないネット回線向け)

環境変数`RSCHIKETTO_DDNS_UPDATE_URL`に、現在のグローバルIPを埋め込みたい
箇所を`{ip}`と書いたURLを設定すると、5分ごとにグローバルIPを確認し、
変化していれば自動更新する(既定オフのオプトイン機能、固定IP環境では
不要)。例(DuckDNS):

```
RSCHIKETTO_DDNS_UPDATE_URL=https://www.duckdns.org/update?domains=myhost&token=xxxx&ip={ip}
```

Windows/Linuxのネイティブバイナリで動作する。**Android版でこの常駐更新が
実際に使えるのは、APK化(未着手)完了後**——現時点のAndroid版の位置づけ
について正直に記載しておく(詳細は`CLAUDE.md`のHANDOFF節)。

## データ/DB保存先の選択(`StorageBackend`)

環境変数`RSCHIKETTO_STORAGE_BACKEND`(`local`/`sftp`/`gdrive`、既定`local`)
で選択できる抽象化を`src/storage.rs`に実装し、既存の全`Store`
(`project.rs`/`comments.rs`/`wiki.rs`/`accounts.rs`/`access.rs`)の
`load`/`save`をこの`StorageBackend`トレイト経由に配線済み。

- **`local`(既定)**: `LocalFsBackend`、実ファイルI/Oでテスト済み。
- **`sftp`**: VPS/レンタルサーバー向け、`ssh2`crateベース(`sftp`
  feature有効時のみ)。`RSCHIKETTO_SFTP_HOST`/`RSCHIKETTO_SFTP_USER`等を
  設定すると実際にこのバックエンドへルーティングされる(2026-07-27に
  配線漏れを修正——以前は設定しても常に`LocalFsBackend`へ
  フォールバックしていた)。**この環境には実SFTPサーバーが無く、実
  ネットワーク越しの到達確認はまだ済んでいない**(到達不能ホストへの
  接続失敗を間接的な証拠として使うテストのみ、正直な開示)。
- **`gdrive`**: Googleドライブ向け、`reqwest`でREST APIを直叩き
  (`files.list`名前検索→`files.get`ダウンロード、アップロード)。
  `RSCHIKETTO_GDRIVE_ACCESS_TOKEN`を設定すると実際にこのバックエンドへ
  ルーティングされる(同じく2026-07-27に配線漏れを修正)。**実APIキーでの
  到達確認は未実施**(正直な開示)。

**`RSCHIKETTO_SFTP_HOST`/`RSCHIKETTO_GDRIVE_ACCESS_TOKEN`等の必須環境変数が
未設定、または`sftp`は`sftp` feature無効でビルドされている場合のみ、
安全側の判断として自動的に`LocalFsBackend`にフォールバックする**
(`storage.rs`の`backend_from_env()`参照)。Googleドライブ等クラウドAPIの
利用にはユーザー自身がOAuth2認証情報を取得する必要があり、このソフト
ウェア単体で完結する機能ではない。詳細・残作業は`CLAUDE.md`のHANDOFF節
および`PORTING.md`参照。

## インストール(ビルド済みバイナリ、インストーラー付き)

タグ付きリリース(`vX.Y.Z`)ごとに、GitHub Actions
(`.github/workflows/release.yml`)がLinux・Windows向けバイナリを
自動ビルドし、[GitHub Releases](https://github.com/aon-co-jp/RS-Chiketto/releases)へ添付する。

### Linux(AlmaLinux・Ubuntu・Debian・Fedora・RHEL等、systemdを使う主要ディストリ共通)

静的リンクされたmuslバイナリのため、ディストリ固有のライブラリ依存は無い。

```bash
curl -fsSL https://github.com/aon-co-jp/RS-Chiketto/releases/latest/download/rs-chiketto-linux-x86_64.tar.gz | tar xz
sudo ./install.sh
sudo systemctl edit rs-chiketto   # RSCHIKETTO_ADMIN_EMAIL等を設定
sudo systemctl enable --now rs-chiketto
```

### Windows / Windows Server

管理者権限のPowerShellで:

```powershell
Invoke-WebRequest -Uri "https://github.com/aon-co-jp/RS-Chiketto/releases/latest/download/rs-chiketto-windows-x86_64.zip" -OutFile rs-chiketto.zip
Expand-Archive rs-chiketto.zip -DestinationPath rs-chiketto
cd rs-chiketto
.\install.ps1
```

## ソースからビルド

```bash
cargo build --release
```

## 開発・検証時にSMTP無しでログインを試す(2026-07-27追記)

通常、`POST /api/auth/request-otp`はSMTP設定(`RSCHIKETTO_SMTP_*`)が
無いと503を返し、ログインフロー自体を試せない。実SMTPサーバーを
用意していない開発・検証・デモ環境では、環境変数
`RSCHIKETTO_DEV_LOG_OTP=true` を設定すると、OTPをメール送信する代わりに
サーバーの標準エラー出力(ログ)へ表示するようになり、すぐにログインを
試せる:

```bash
RSCHIKETTO_DEV_LOG_OTP=true cargo run --release
# ログに "RSCHIKETTO_DEV_LOG_OTP: ... printing OTP for ... : 123456" のように出力される
```

**本番運用ではこのフラグを絶対に設定しないこと**(OTPが誰でも読める
サーバーログに平文で出力されるため)。

## ライセンス

Apache-2.0

## API 方針: REST 全廃・gRPC / Connect へ (2026-11-04)

### 日本語
- **REST は全廃する。新しい API は REST で作らない。** ブラウザや `curl` から呼ぶ API は **Connect の単純呼び出し**(`POST /<パッケージ>.<Service>/<Method>`、`content-type: application/json`、本文は JSON)、サービス間は **gRPC / gRPC-web**(RPoem の管理API `open_runo.admin.v1.Admin` と同じ流儀)。ストリーミングは、必要になってから。
- **既存の REST は、触るときに順に移す。** 移したら旧 REST の経路は消す。移行が終わるまで残るが、**新規には追加しない**。
- **JSON は RS-JSON に統一**する(読み込みは `from_str_secure` / `parse_secure` でサイズ・深さ・重複キーの上限つき、書き出しは `rust_json::fast`)。ブラウザの WASM に `prost` は入れない(サイズが増えるため)。
- **例外(自分たちの API ではなく、相手の規格が決めているもの)**: ACME、OAuth/OIDC のコールバック、第三者(GitHub など)からの Webhook、ヘルスチェック、静的ファイル配信、OTLP など。これらは規格どおりの HTTP を受ける。
- 参考実装: `RPoem`(`open_runo.admin.v1.Admin`)、`open-easy-web`(`server/src/connect.rs`、`proto/open_easy_web/admin/v1/admin.proto`)。
- ※ このリポジトリのコードが既に方針どおりかどうかは、各リポジトリの現状による。この節は方針の記録であり、コードの変更を意味しない。

### English
- **REST is being abolished. New APIs are not built as REST.** APIs called from browsers or `curl` use **Connect unary calls** (`POST /<package>.<Service>/<Method>`, `content-type: application/json`, JSON body); service-to-service calls use **gRPC / gRPC-web** (the same style as RPoem's admin API `open_runo.admin.v1.Admin`). Streaming only when it is actually needed.
- **Existing REST endpoints are migrated one by one, when they are touched.** The old REST route is removed once migrated. Until migration is finished they remain, but **no new ones are added**.
- **JSON is unified on RS-JSON**: reading with `from_str_secure` / `parse_secure` (size, depth and duplicate-key limits), writing with `rust_json::fast`. `prost` is not put into browser WASM (it would grow the binary).
- **Exceptions (shapes dictated by someone else's standard, not our own API)**: ACME, OAuth/OIDC callbacks, webhooks from third parties (e.g. GitHub), health checks, static file serving, OTLP, and so on. These accept plain HTTP as the standard requires.
- Reference implementations: `RPoem` (`open_runo.admin.v1.Admin`) and `open-easy-web` (`server/src/connect.rs`, `proto/open_easy_web/admin/v1/admin.proto`).
- Note: whether the code in a given repository already follows this policy depends on that repository. This section records the policy; it does not mean the code has been changed.

### Deutsch (KI-Übersetzung, redaktionell überarbeitet; noch nicht von Muttersprachlern geprüft)
- **REST wird vollständig abgeschafft; neue APIs entstehen nicht mehr als REST.** APIs, die aus dem Browser oder per `curl` aufgerufen werden, verwenden **unäre Connect-Aufrufe** (`POST /<Paket>.<Service>/<Methode>`, `content-type: application/json`, JSON-Body). Zwischen Diensten kommen **gRPC bzw. gRPC-web** zum Einsatz, im selben Stil wie die Admin-API von RPoem (`open_runo.admin.v1.Admin`). Streaming wird erst ergänzt, wenn es tatsächlich gebraucht wird.
- **Bestehende REST-Endpunkte werden schrittweise umgestellt, und zwar immer dann, wenn man ohnehin an ihnen arbeitet.** Sobald ein Endpunkt umgestellt ist, wird die alte REST-Route entfernt. Bis zum Abschluss der Migration bleiben die übrigen bestehen, es werden aber **keine neuen mehr hinzugefügt**.
- **JSON wird einheitlich über RS-JSON verarbeitet**: Gelesen wird mit `from_str_secure` bzw. `parse_secure` (mit Obergrenzen für Größe, Verschachtelungstiefe und doppelte Schlüssel), geschrieben mit `rust_json::fast`. `prost` wird nicht in das Browser-WASM aufgenommen, da es die Datei vergrößern würde.
- **Ausnahmen (Schnittstellen, deren Form ein fremder Standard vorgibt und die nicht zu unserer eigenen API gehören)**: ACME, OAuth/OIDC-Callbacks, Webhooks von Drittanbietern (z. B. GitHub), Health-Checks, die Auslieferung statischer Dateien, OTLP und Ähnliches. Diese nehmen, wie vom jeweiligen Standard verlangt, gewöhnliches HTTP entgegen.
- Referenzimplementierungen: `RPoem` (`open_runo.admin.v1.Admin`) und `open-easy-web` (`server/src/connect.rs`, `proto/open_easy_web/admin/v1/admin.proto`).
- Hinweis: Ob der Code eines einzelnen Repositorys diese Richtlinie bereits erfüllt, hängt vom jeweiligen Repository ab. Dieser Abschnitt dokumentiert lediglich die Richtlinie; er besagt nicht, dass der Code schon angepasst wurde.

### Русский (перевод выполнен с помощью ИИ и отредактирован; носителями языка пока не проверен)
- **От REST мы полностью отказываемся; новые API больше не создаются в стиле REST.** API, к которым обращаются из браузера или через `curl`, используют **унарные вызовы Connect** (`POST /<пакет>.<Service>/<Method>`, `content-type: application/json`, тело запроса — JSON). Для взаимодействия между сервисами применяются **gRPC и gRPC-web** — в том же стиле, что и административный API RPoem (`open_runo.admin.v1.Admin`). Потоковая передача данных добавляется лишь тогда, когда в ней действительно возникает необходимость.
- **Существующие REST-эндпоинты переводятся поэтапно — по мере того, как с ними приходится работать.** После перевода старый REST-маршрут удаляется. Пока миграция не завершена, оставшиеся эндпоинты сохраняются, но **новые не добавляются**.
- **Работа с JSON унифицирована на основе RS-JSON**: чтение — через `from_str_secure` или `parse_secure` (с ограничениями на размер, глубину вложенности и повторяющиеся ключи), запись — через `rust_json::fast`. `prost` в браузерный WASM не включается, поскольку это увеличило бы размер файла.
- **Исключения (интерфейсы, форму которых определяет чужой стандарт, а не наш собственный API)**: ACME, обратные вызовы OAuth/OIDC, вебхуки сторонних сервисов (например, GitHub), проверки работоспособности, раздача статических файлов, OTLP и тому подобное. Такие интерфейсы принимают обычный HTTP, как того требует соответствующий стандарт.
- Эталонные реализации: `RPoem` (`open_runo.admin.v1.Admin`) и `open-easy-web` (`server/src/connect.rs`, `proto/open_easy_web/admin/v1/admin.proto`).
- Примечание: соответствует ли код конкретного репозитория этой политике, зависит от самого репозитория. Данный раздел лишь фиксирует политику и не означает, что код уже изменён.

## API 方針の補足: 用語の整理と使い分け (2026-11-04)

### 日本語
**1つに統一するものではなく、別の層の話です。** 「REST の代わり」になるのは Connect・gRPC・GraphQL だけで、WASM は画面を作る技術です。

| 用語 | 何か | REST の代わりか |
|---|---|---|
| REST | URL と HTTP メソッドで操作を表す API の作り方 | 全廃する側 |
| **Connect** | `POST /サービス名/メソッド名` に JSON を送る形式。`curl` でもブラウザの `fetch` でも呼べる | はい(**採用**) |
| **gRPC** | 同じ「サービス/メソッド」の考え方を、バイナリ(protobuf)と HTTP/2 で行う形式。速く、ストリーミングも得意 | はい(**サービス同士に採用**) |
| gRPC-web | gRPC のブラウザ向けの変形版 | はい(必要なら) |
| GraphQL | 欲しい項目を問い合わせる形式 | 一部だけ(**問い合わせ(読み取り)専用**) |
| WASM | Rust をブラウザの中で動かす仕組み | いいえ(画面を作る技術) |

- **ブラウザの画面や `curl` から呼ぶ API は Connect**(POST、JSON を RS-JSON で読み書き)。ブラウザの WASM は素の gRPC を話せず、Connect なら `fetch` のままで、`prost` を足さずに済む(画面のサイズも増えない)。
- **サービス同士の通信(サーバー間、重い処理、ストリーミング)は gRPC。** 多くは Connect で足りるので、ストリーミングが必要になった所だけ gRPC に上げる。
- **GraphQL は、まとめて問い合わせたいときの読み取り専用。** 書き込み(操作)には使わない。
- **WASM は画面を作る技術として、そのまま使う。** 画面から呼ぶ API が Connect になっただけ。
- 外部の規格が形を決めるもの(ACME、OAuth のコールバック、第三者の Webhook、ヘルスチェック、静的ファイル、OTLP)は、規格どおりの HTTP のまま。
- **迷ったら Connect。**

### English
**These are not alternatives to unify; they sit at different layers.** Only Connect, gRPC and GraphQL can replace REST; WASM is a way to build the UI.

| Term | What it is | Replaces REST? |
|---|---|---|
| REST | API style that expresses operations through URLs and HTTP methods | The thing being abolished |
| **Connect** | `POST /Service/Method` with a JSON body; callable from `curl` and from a browser `fetch` | Yes (**adopted**) |
| **gRPC** | The same service/method idea over binary (protobuf) and HTTP/2; fast, good at streaming | Yes (**for service-to-service**) |
| gRPC-web | A browser-friendly variant of gRPC | Yes (if needed) |
| GraphQL | A query language: ask for exactly the fields you want | Partly (**read-only queries**) |
| WASM | Runs Rust inside the browser | No (a UI technology) |

- **APIs called from browser screens or `curl` use Connect** (POST, JSON read and written with RS-JSON). Browser WASM cannot speak plain gRPC; with Connect it stays a `fetch`, and `prost` is not needed (the binary does not grow).
- **Service-to-service calls (server to server, heavy work, streaming) use gRPC.** Connect is enough in most places; move a call to gRPC only where streaming is actually needed.
- **GraphQL is read-only**, for queries that gather several pieces of data. It is not used for writes (operations).
- **WASM stays as the way to build the UI.** The API it calls simply became Connect.
- Interfaces whose shape an outside standard dictates (ACME, OAuth callbacks, third-party webhooks, health checks, static files, OTLP) stay plain HTTP.
- **When in doubt, use Connect.**

### Deutsch (KI-Übersetzung, sprachlich überarbeitet; noch nicht von Muttersprachlern geprüft)
**Es geht nicht darum, eine einzige Technik auszuwählen – die Begriffe liegen auf unterschiedlichen Ebenen.** Als Ersatz für REST kommen nur Connect, gRPC und GraphQL in Frage; WASM dagegen ist eine Technik, mit der die Benutzeroberfläche gebaut wird.

| Begriff | Was ist das? | Ersatz für REST? |
|---|---|---|
| REST | Ein API-Stil, bei dem Operationen über URLs und HTTP-Methoden ausgedrückt werden | wird abgeschafft |
| **Connect** | `POST /Dienst/Methode` mit JSON im Body; aufrufbar mit `curl` ebenso wie mit `fetch` im Browser | ja (**gewählt**) |
| **gRPC** | Dasselbe Prinzip aus Dienst und Methode, aber über binäres protobuf und HTTP/2; schnell und gut für Streaming geeignet | ja (**zwischen Diensten**) |
| gRPC-web | Eine für den Browser angepasste Variante von gRPC | ja (bei Bedarf) |
| GraphQL | Abfragesprache, mit der man genau die benötigten Felder anfordert | teilweise (**nur für lesende Abfragen**) |
| WASM | Führt Rust-Code direkt im Browser aus | nein (Technik für die Oberfläche) |

- **APIs, die von Browseroberflächen oder per `curl` aufgerufen werden, verwenden Connect** (POST; JSON wird mit RS-JSON gelesen und geschrieben). WASM im Browser kann kein reines gRPC sprechen. Mit Connect bleibt es bei einem gewöhnlichen `fetch`, und `prost` ist nicht nötig – die WASM-Datei wird dadurch nicht größer.
- **Für die Kommunikation zwischen Diensten (Server zu Server, rechenintensive Aufgaben, Streaming) wird gRPC eingesetzt.** In den meisten Fällen reicht Connect aus; auf gRPC wird nur dort umgestellt, wo Streaming tatsächlich gebraucht wird.
- **GraphQL dient ausschließlich lesenden Abfragen**, wenn mehrere Informationen gebündelt abgefragt werden sollen. Für Schreibzugriffe (Operationen) wird es nicht verwendet.
- **WASM bleibt die Technik für die Oberfläche.** Geändert hat sich lediglich, dass die aufgerufene API nun auf Connect beruht.
- Schnittstellen, deren Form durch einen externen Standard festgelegt ist (ACME, OAuth-Callbacks, Webhooks von Drittanbietern, Health-Checks, statische Dateien, OTLP), bleiben gewöhnliches HTTP.
- **Im Zweifel: Connect.**

### Русский (перевод выполнен с помощью ИИ и литературно отредактирован; носителями языка пока не проверен)
**Это не варианты, из которых нужно выбрать один, а технологии разных уровней.** Заменить REST могут только Connect, gRPC и GraphQL; WASM же — это способ создания пользовательского интерфейса.

| Термин | Что это | Заменяет REST? |
|---|---|---|
| REST | Стиль построения API, при котором операции выражаются через URL и HTTP-методы | отменяется |
| **Connect** | `POST /Сервис/Метод` с телом в формате JSON; вызывается как из `curl`, так и из браузерного `fetch` | да (**выбран**) |
| **gRPC** | Тот же принцип «сервис — метод», но поверх двоичного формата protobuf и HTTP/2; работает быстро и хорошо подходит для потоковой передачи | да (**для связи между сервисами**) |
| gRPC-web | Адаптированный для браузера вариант gRPC | да (при необходимости) |
| GraphQL | Язык запросов: клиент запрашивает ровно те поля, которые ему нужны | частично (**только запросы на чтение**) |
| WASM | Позволяет выполнять код на Rust прямо в браузере | нет (технология для интерфейса) |

- **API, которые вызываются из браузерных интерфейсов или через `curl`, строятся на Connect** (POST; JSON читается и записывается через RS-JSON). Браузерный WASM не умеет работать с «чистым» gRPC. С Connect достаточно обычного `fetch`, а `prost` не требуется — размер WASM-файла не увеличивается.
- **Для обмена между сервисами (сервер — сервер, ресурсоёмкие задачи, потоковая передача) применяется gRPC.** В большинстве случаев достаточно Connect; на gRPC переходят только там, где потоковая передача действительно необходима.
- **GraphQL используется только для чтения**, когда нужно получить сразу несколько связанных данных. Для записи (операций) он не применяется.
- **WASM по-прежнему остаётся технологией для интерфейса.** Изменилось лишь то, что вызываемый им API теперь построен на Connect.
- Интерфейсы, форма которых задана внешним стандартом (ACME, обратные вызовы OAuth, вебхуки сторонних сервисов, проверки работоспособности, статические файлы, OTLP), остаются обычным HTTP.
- **Если есть сомнения — выбирайте Connect.**


## 2026-11-26 Versionless化: 版つき(.v1)Connect経路を即時削除 / `.v1` removed, versionless only
**日本語**: 所有者の指示(2026-11-26)で、Connect のパッケージ名を `open_redmine.v1` から **`open_redmine`** に統一し、旧 `.v1` 経路は猶予なしで削除した(`POST /open_redmine.<Service>/<Method>`、42本、名前の変更のみで意味は同じ)。UI(`web/src/lib.rs`、WASM の `web/pkg` を再生成)・試験・文書を同時に直した。`cargo test` 96件成功(旧 `.v1` が404になる試験を追加)。本番(VPS、`open-redmine` 8100・`open-redmine-demo` 8113)へ WSL `Ubuntu-24.04`+`check-glibc.sh`(glibc 2.38)でビルドして入れ替え済み。バックアップ: `/root/open-redmine/target/release/rs-chiketto.before-v1del-20261126`、`/root/pkg.before-v1del-20261126`。実測: 新 `Projects/Create`(認証なし)401、旧 `.v1` 404、旧 `/api/projects` 404、`/healthz` 200、画面・`pkg/rs_red_web.js` 200、エラーログ0。最初の確認で `{}` 本文が400(検証が認証より先)だったため自動ロールバックが働いたが、確認条件が厳しすぎただけで、本文を直して再入れ替えし成功。デモ(8113)は認証なしで書けるのが従来どおりで、確認用に作った1件は削除済み。**未了**: ログイン後の通し操作の本番確認、ブラウザでの再確認(白画面が無いか。本番の `/` は200)。
**再開用メッセージ(別アカウントで貼り付け)**: `aon-co-jp/open-redmine` の続き。Connect は版なし `POST /open_redmine.<Service>/<Method>` に統一済み(`.v1` は削除・404)、`main` と本番に反映済み。新規clone して README・CLAUDE・PORTING を読み、未了(ログイン後の通し・ブラウザ確認)を確認なしで進める。決まり: 新しいRESTは作らない、秘密は書かない、VPSでビルドしない(WSL `Ubuntu-24.04` で `F:unops-build.sh`)、実機で確かめてから完了と書く、返答は日本語のみ。

## 2026-11-26 Versionless: the versioned (`.v1`) Connect routes were removed immediately
**English**: On the owner's instruction (2026-11-26) the Connect package was renamed from `open_redmine.v1` to **`open_redmine`** and the old `.v1` routes were deleted with no grace period (`POST /open_redmine.<Service>/<Method>`, 42 routes; names changed, meaning unchanged). The UI (`web/src/lib.rs`, with the WASM `web/pkg` regenerated), tests and docs were fixed together. `cargo test` passes 96 tests (a test that old `.v1` returns 404 was added). Production (VPS, `open-redmine` 8100 and `open-redmine-demo` 8113) was rebuilt in WSL `Ubuntu-24.04` with `check-glibc.sh` (needs glibc 2.38) and swapped in. Backups: `/root/open-redmine/target/release/rs-chiketto.before-v1del-20261126` and `/root/pkg.before-v1del-20261126`. Measured: new `Projects/Create` without auth 401, old `.v1` 404, old `/api/projects` 404, `/healthz` 200, UI and `pkg/rs_red_web.js` 200, error log 0. The first check saw 400 for a `{}` body (validation runs before auth), so the automatic rollback fired; the check was simply too strict, and after fixing it the swap was repeated successfully. The demo (8113) accepts writes without auth as before; the one test record I created was deleted. **Open**: authenticated end-to-end flows in production, and a browser re-check (no white screen; production `/` returns 200).
**Resume message (paste on another account)**: Continue `aon-co-jp/open-redmine`. Connect is unified on versionless `POST /open_redmine.<Service>/<Method>` (`.v1` removed, now 404), on `main` and in production. Clone fresh, read README, CLAUDE and PORTING, and carry on with the open items (authenticated flows, browser check) without asking. Rules: no new REST, never write secrets, never build on the VPS (WSL `Ubuntu-24.04` with `F:unops-build.sh`), verify on real systems before saying done, reply in Japanese only.

## Versionless API の方針 / Versionless API policy (2026-11-25)
**日本語**: このリポジトリのAPIは **Versionless**(URL・パッケージ名に `v1` 等を付けず、互換性を壊さず足すだけで進化させる)。欄は足すだけ・足す欄は省略可能・知らない欄は無視・名前変更は新名を足して旧を非推奨に(期限は既定で6か月以上、使用0を確認してから削除)・enumは未知の値を許容・安全上の緊急修正は例外。既存の `.v1` は呼び出し側を壊さないよう当面残し、版なしの別名を追加して段階的に畳む。詳細・出典は [`runo/API-POLICY.md`](https://github.com/aon-co-jp/runo/blob/main/API-POLICY.md) 末尾。適用はこれから順に行う。
**English**: This repository's APIs are **Versionless**: no `v1` in URLs or package names; evolve by adding only, without breaking compatibility. Only add fields, new fields are optional, ignore unknown fields, rename by adding the new name and deprecating the old (default deadline 6 months or more, remove only after confirming zero use), tolerate unknown enum values, emergency security fixes are the exception. Existing `.v1` names stay for now so callers do not break; a versionless alias is added and `.v1` is retired in stages. Details and sources: the end of [`runo/API-POLICY.md`](https://github.com/aon-co-jp/runo/blob/main/API-POLICY.md). Rollout is step by step.

## 完成まで作り切る方針 / Finish everything policy (2026-11-27)
**日本語**: 全リポジトリで、**未確認は自動で確認し、未実施は実施し、未了は完成させる**(確認を求めず進める)。使用頻度が低いと判断される機能や、**1から作り直す大規模開発になる内容でも、やめずに最後まで開発・TEST・DEBUGする**(別の方式で済ませない、後回しにしない)。進め方: 調査(英語・日本語の検索とGitHub) → 設計 → 実装 → 実機/実データで確認 → 失敗は原因を直して再試験 → README・CLAUDE・PORTING・POLICY を日英で更新 → push。「できた」と書くのは実測した範囲だけ、確認できなかった所は「未確認」と正直に書く。例外(確認を取る): 元に戻せない削除、秘密の公開、所有者の認証・決済・実機操作が要るもの、利用者のシステム設定の変更。
**English**: In every repository, **verify what is unverified, do what is not yet done, and finish what is unfinished** automatically, without asking. Even features that look rarely used, or work that means **a large rewrite from scratch, are developed, tested and debugged to the end** (no substituting another approach, no postponing). Method: research (English and Japanese search plus GitHub) -> design -> implement -> verify on real devices or data -> fix causes and retest on failure -> update README, CLAUDE, PORTING and POLICY in Japanese and English -> push. Write "done" only for what was measured, and say "unverified" honestly for the rest. Exceptions that still need confirmation: irreversible deletions, publishing secrets, anything needing the owner's authentication, payment or real-device actions, and changes to the user's system settings.