# RS-Red

(旧名`RS-Chiketto`、2026-07-22に`RS-Red`へ改名。公開先: `https://runo.tokyo/RS-Red`)

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
> Redmine全体の機能網羅率としてはまだ3割程度。詳細は`CLAUDE.md`参照。

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

## ライセンス

Apache-2.0
