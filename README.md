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

### Deutsch (KI-Übersetzung, noch nicht von Muttersprachlern geprüft)
- **REST wird abgeschafft. Neue APIs werden nicht als REST gebaut.** APIs, die aus dem Browser oder mit `curl` aufgerufen werden, nutzen **Connect-Unary-Aufrufe** (`POST /<Paket>.<Service>/<Methode>`, `content-type: application/json`, JSON-Body); zwischen Diensten gilt **gRPC / gRPC-web** (wie bei der Admin-API von RPoem, `open_runo.admin.v1.Admin`). Streaming erst, wenn es wirklich nötig ist.
- **Bestehende REST-Endpunkte werden einzeln migriert, sobald man sie anfasst.** Nach der Migration wird die alte REST-Route entfernt. Bis dahin bleiben sie bestehen, es werden aber **keine neuen hinzugefügt**.
- **JSON wird auf RS-JSON vereinheitlicht**: Lesen mit `from_str_secure` / `parse_secure` (Grenzen für Größe, Tiefe und doppelte Schlüssel), Schreiben mit `rust_json::fast`. `prost` kommt nicht in Browser-WASM (die Datei würde größer).
- **Ausnahmen (Formate, die ein fremder Standard vorgibt, nicht unsere eigene API)**: ACME, OAuth/OIDC-Callbacks, Webhooks von Dritten (z. B. GitHub), Health-Checks, statische Dateien, OTLP usw. Diese nehmen schlichtes HTTP entgegen, wie es der Standard verlangt.
- Referenzimplementierungen: `RPoem` (`open_runo.admin.v1.Admin`) und `open-easy-web` (`server/src/connect.rs`, `proto/open_easy_web/admin/v1/admin.proto`).
- Hinweis: Ob der Code eines Repositorys diese Richtlinie schon befolgt, hängt vom jeweiligen Repository ab. Dieser Abschnitt hält die Richtlinie fest; er bedeutet nicht, dass der Code bereits geändert wurde.

### Русский (машинный перевод, носителями языка ещё не проверен)
- **REST полностью отменяется. Новые API не создаются как REST.** API, вызываемые из браузера или через `curl`, используют **унарные вызовы Connect** (`POST /<пакет>.<Service>/<Method>`, `content-type: application/json`, тело — JSON); между сервисами — **gRPC / gRPC-web** (так же, как административный API RPoem, `open_runo.admin.v1.Admin`). Потоковая передача — только когда она действительно нужна.
- **Существующие REST-эндпоинты переносятся по одному, когда до них доходят руки.** После переноса старый REST-маршрут удаляется. До завершения переноса они остаются, но **новые не добавляются**.
- **JSON унифицируется на RS-JSON**: чтение через `from_str_secure` / `parse_secure` (ограничения на размер, глубину и повторяющиеся ключи), запись через `rust_json::fast`. `prost` не добавляется в браузерный WASM (размер вырос бы).
- **Исключения (форматы, заданные чужим стандартом, а не наш собственный API)**: ACME, обратные вызовы OAuth/OIDC, вебхуки сторонних сервисов (например, GitHub), проверки работоспособности, раздача статических файлов, OTLP и т. п. Они принимают обычный HTTP, как требует стандарт.
- Эталонные реализации: `RPoem` (`open_runo.admin.v1.Admin`) и `open-easy-web` (`server/src/connect.rs`, `proto/open_easy_web/admin/v1/admin.proto`).
- Примечание: соответствует ли код конкретного репозитория этой политике, зависит от самого репозитория. Этот раздел фиксирует политику и не означает, что код уже изменён.
