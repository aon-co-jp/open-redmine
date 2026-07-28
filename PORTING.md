# PORTING.md — RS-Red を他プロジェクトへお引越しする際のガイド

## 現状(2026-07-23)

チケット(Issue)CRUD・プロジェクトCRUD+サブプロジェクト階層・コメント・
Wiki(改訂履歴保持)・OTP認証・プロジェクト単位アクセス制御・ブラウザGUI
(Rust→WebAssembly)まで実装済み。詳細は`CLAUDE.md`のHANDOFF参照。

## 0. パスプレフィックス配下マウント時の絶対パスfetch罠(2026-07-28発見、移植時の重要な注意点)

**この罠は`open-gitea`/`RS-Sync`でも過去に踏んだのと同種**——複数
プロジェクトを移植する際は必ずここを確認すること。

- **症状**: `web/`(WASMフロントエンド)が絶対パス`/api/...`で`fetch()`
  していると、`https://easy-web.tokyo/open-redmine/`のようにパス
  プレフィックス配下(open-web-serverの「分身の術」テナントルーティング、
  `path_prefix`剥がし転送)にマウントされた瞬間、ブラウザは常にオリジン
  直下(`https://easy-web.tokyo/api/...`)を叩いてしまい、そのプレフィックス
  を持たない別サービス(またはトップページ自体)にリクエストが誤到達し、
  意味不明な400/404が返る。**バックエンド自体は正しく動いているため、
  サーバー側ログ・`curl`での直接検証だけでは気づけない**——実際に
  ブラウザの実クリック+Networkタブでの検証が必要(2026-07-28、
  OTP送信ボタンが「実装されていない」ように見えた不具合の真因はこれ
  だった)。
- **修正パターン**: `web/src/lib.rs`に固定の`const BASE_PATH: &str =
  "/<マウント先>";`を定義し、`fetch()`直前に`format!("{BASE_PATH}{path}")`
  で必ず前置する(相対パス化〈`RS-Sync`方式〉でも良いが、トレイリング
  スラッシュの有無でブラウザの相対URL解決結果が変わる罠があるため、
  このリポジトリでは固定プレフィックス定数の方を採用した)。
- **正直な開示**: この定数は単一マウント先を前提としたハードコードで、
  複数の異なるプレフィックスへ同時マウントする場合は動的化が必要
  (今回はスコープ外)。

## 1. `RS-Git`からそのまま移植したパターン

- `auth.rs`(OTPログイン機構)・`mail.rs`(SMTP送信)は`RS-Git`の実装を
  そのまま移植したもの(環境変数名のみ`RSCHIKETTO_*`にリネーム)。
- 登録アカウント制(`accounts_locked`、既定`true`)・アクセス制御
  (`access.rs`、閲覧/編集の個別許可)も`RS-Git`と同じ設計を踏襲。

## 2. RustJSON経由の永続化(2026-07-23移植)

`src/rustjson.rs`は[RPoem](https://github.com/aon-co-jp/RPoem)の
`open-runo-rustjson`crateを移植したもの(トレイリングカンマ・コメント・
裸キー・シングルクォート文字列を許容する緩い構文、パース結果は標準
`serde_json::Value`)。**クロスリポジトリのCargo依存(RPoem側crateへの
直接依存)は避け、小さなモジュールとして直接コピーする**——これは
`open-web-server`/`RPoem`のリリースCIで実際にpath依存問題が発生した
教訓に基づく判断(詳細は`open-raid-z/CLAUDE.md`・`PORTING.md`参照)。

移植パターン:
```rust
// 読み込み: 緩い構文を許容
Ok(bytes) => crate::rustjson::parse_typed(&bytes).unwrap_or_default(),
// 書き込み: 引き続き整形済み標準JSON(可読性維持、RustJSONの入力としても有効)
let bytes = serde_json::to_vec_pretty(store).expect("...");
```

## 3. ブラウザGUI(`web/`、Rust→WebAssembly)

「チケット管理を行うWEBアプリである以上GUIは基本機能」という方針
(2026-07-23、ユーザー指示)。Tauri・Node.js・TypeScriptには依存しない
(このエコシステム共通方針)。移植時のポイント:

- **`GET /`はGUIを優先、無ければAPI概要ページへフォールバック**する
  設計(`RSCHIKETTO_WEB_DIR`環境変数で配置場所を変更可能)。GUIビルド
  成果物が無い環境でも壊れない。
- **オンライン専用**(オフライン/Service Worker対応は行っていない、
  ユーザー確認済みのスコープ)。
- **ピンチズームは標準の`viewport`メタタグのみで機能する**——
  Android/iOSのモバイルブラウザ向けに特別な実装は一切不要。
- `GET /pkg/:file`ハンドラでWASM成果物を配信する際は、ファイル名に
  `..`・`/`・`\`を含む場合を拒否するパストラバーサル対策を必ず入れる
  (`open-web-server`の`static_files.rs`と同じ方針)。
- ビルド成果物(`pkg/*.js`・`pkg/*.wasm`)は`.gitignore`せず**コミット
  する**方針とした——GUIをこのアプリの基本機能と位置づけたため、
  `git clone`直後にwasmツールチェーン無しで動く体験を優先した
  (他リポジトリの一部で採用している「ビルド成果物はgitignore」方針
  とは意図的に異なる判断、理由をこのファイルに明記)。

## 4. 同時並行開発の対象プロジェクト

- [open-raid-z](https://github.com/aon-co-jp/open-raid-z) — 開発ルールの正本
- [aruaru-db](https://github.com/aon-co-jp/aruaru-db) — DB層(「分身の術」共有構成、DUAL DB移行先候補)
- [open-cuda](https://github.com/aon-co-jp/open-cuda) — GPU計算基盤
- [open-web-server](https://github.com/aon-co-jp/open-web-server) — 「分身の術」基盤実装
- [RPoem](https://github.com/aon-co-jp/RPoem) — アプリケーションサーバー層、RustJSONの移植元

## 5.5 スマホ対応・英語(日本語)ハイブリッド表示(2026-07-24)

`web/index.html`は`@media (max-width: 600px)`でスマホ縦画面向けの余白・
フォントサイズ調整を追加し、全`button`/`input`/`textarea`/`select`に
`min-height: 44px`(タップ操作向けタッチターゲット推奨サイズ)を適用
している。静的HTMLシェル内のUIラベルは「英語表記の直後に(日本語)を
括弧書き」形式(例: `Login (ログイン)`)に統一済み。`web/src/lib.rs`
側の動的生成メッセージ(エラー文言等)は今回対象外(段階適用中)。

## 5. 未着手のまま残る移植候補(次回以降)

- ストレージ先選択機能(Googleドライブ・他クラウド・VPS、`StorageBackend`
  トレイト抽象化)——2026-07-23に`src/storage.rs`としてトレイト定義+
  `LocalFsBackend`実装+`SftpBackend`/`GDriveBackend`のロジック骨格まで
  実装、続く同日セッションで既存`Store`群(`project.rs`/`comments.rs`/
  `wiki.rs`/`accounts.rs`/`access.rs`)の`load`/`save`を`StorageBackend`
  経由へ実配線済み(`cargo test`52件全green、`CLAUDE.md` HANDOFF参照)。
  `SftpBackend`(`ssh2`ベース、`read`/`write`/`ensure_dir`/`exists`本体
  実装済み)・`GDriveBackend`(`read`/`write`実装済み)も本体コードは
  揃った。**残作業**: (1) 実SFTPサーバー(またはループバックSSHサーバー)
  での`SftpBackend`到達確認(本セッションでは未実施、`CLAUDE.md`
  HANDOFF参照)、(2) 実Google Drive APIキーでの`GDriveBackend`到達確認、
  (3) 上記完了後に`storage::backend_from_env()`の「`local`以外は警告して
  フォールバック」を解除、(4) Dropbox/OneDrive等追加プロバイダ。
- `aruaru-db`/PostgreSQL DUAL DB構成への移行(現状はJSONファイルのみ)。
- ガントチャート・カレンダーのGUI実装(バックエンド側の`Ticket.start_date`/
  `due_date`/`done_ratio`と`GET /api/tickets`の`status`/`project_id`絞り込み
  は2026-07-23に実装済み、`CLAUDE.md` HANDOFF参照。GUI描画は未着手)。
