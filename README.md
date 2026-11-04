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
