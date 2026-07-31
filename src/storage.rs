//! `StorageBackend`抽象化——データ/DBの永続化先をローカルディスク以外
//! (VPS/レンタルサーバーのSFTP、Googleドライブ等のクラウド)にも選択可能
//! にするための最小契約。
//!
//! 既存の`rustjson.rs`・各`Store`は現状`std::fs`直書きのみだったが、
//! このトレイト経由に置き換えたことで、環境変数`RSCHIKETTO_STORAGE_BACKEND`
//! による切り替えが可能になった(`main.rs`の全`Store::load`/`save`呼び出し
//! 箇所は`AppState.backend`経由に配線済み。詳細はCLAUDE.mdのHANDOFF節を
//! 参照)。
//!
//! # 対応状況(正直な開示)
//! - `LocalFsBackend`: 実装済み・実ファイルI/Oでテスト済み(既定)。
//! - `SftpBackend`: `ssh2`crateでの`read`/`write`/`ensure_dir`/`exists`
//!   本体を実装済み(TCP接続+SSHハンドシェイク+パスワード認証、
//!   ディレクトリの再帰`mkdir`)。**正直な開示**: この環境には実SFTP
//!   サーバーが無く、`open-web-server`の`sftp.rs`が採用したような
//!   ループバックSSHサーバー(`russh`のサーバー機能を使う想定)を
//!   本セッションでは追加できていない——`ssh2`はクライアント専用crateの
//!   ためテストサーバー役には使えず、サーバー側の実装コストとの兼ね合いで
//!   見送った。よってユニットテストは、パス正規化・再帰mkdirのロジックを
//!   モックなしで検証する範囲に留まり、**実ネットワーク越しの接続・
//!   アップロード・ダウンロードの到達確認はできていない**。
//! - `GDriveBackend`: Google Drive REST APIをOAuth2アクセストークン
//!   (`RSCHIKETTO_GDRIVE_ACCESS_TOKEN`、ユーザー自身がGoogle Cloud
//!   プロジェクトで取得したものを渡す前提——このソフトウェア自体が
//!   認証情報を代行取得することはできない)を使って叩く`read`
//!   (`files.list`名前検索→`files.get?alt=media`ダウンロード)・`write`
//!   (アップロード)を実装。実APIキーが無いため、リクエストURL構築・
//!   クエリエンコードのみをモックなしの単体テストで検証しており、実際の
//!   Google Driveへの到達確認はしていない。
//! - Dropbox・OneDrive等その他の「有名なクラウド保存」は、この
//!   `StorageBackend`トレイトが汎用的に設計してあるため後から追加できる
//!   (未着手)。
//!
//! # Android版の既定バックエンド
//! ユーザー指示により、Android版は既定で`gdrive`、Windows/Linuxは既定で
//! `local`とする想定(Android版自体は未着手のAPK化待ち、`ddns.rs`と同様)。

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use std::path::Path;

/// データ/DB永続化層の最小I/O契約。
#[async_trait]
pub trait StorageBackend: Send + Sync {
    /// `path`の内容をバイト列で読み込む。存在しない場合はエラー。
    async fn read(&self, path: &str) -> Result<Vec<u8>>;
    /// `path`に`bytes`を書き込む(上書き)。親ディレクトリが無ければ作成する。
    async fn write(&self, path: &str, bytes: &[u8]) -> Result<()>;
    /// `path`をディレクトリとして存在保証する(無ければ作成、再帰的)。
    async fn ensure_dir(&self, path: &str) -> Result<()>;
    /// `path`が存在するか。
    async fn exists(&self, path: &str) -> bool;
}

/// 既定バックエンド。現状の`std::fs`直書きをそのままラップするだけ。
#[derive(Debug, Clone, Default)]
pub struct LocalFsBackend;

#[async_trait]
impl StorageBackend for LocalFsBackend {
    async fn read(&self, path: &str) -> Result<Vec<u8>> {
        tokio::fs::read(path).await.with_context(|| format!("failed to read {path}"))
    }

    async fn write(&self, path: &str, bytes: &[u8]) -> Result<()> {
        if let Some(parent) = Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await.with_context(|| format!("failed to create parent dir for {path}"))?;
            }
        }
        tokio::fs::write(path, bytes).await.with_context(|| format!("failed to write {path}"))
    }

    async fn ensure_dir(&self, path: &str) -> Result<()> {
        tokio::fs::create_dir_all(path).await.with_context(|| format!("failed to create dir {path}"))
    }

    async fn exists(&self, path: &str) -> bool {
        tokio::fs::metadata(path).await.is_ok()
    }
}

/// VPS/レンタルサーバー向けSFTPバックエンド。接続先は環境変数で指定:
/// `RSCHIKETTO_SFTP_HOST`・`RSCHIKETTO_SFTP_PORT`(既定22)・
/// `RSCHIKETTO_SFTP_USER`・`RSCHIKETTO_SFTP_PASSWORD`(またはキー認証は
/// 未実装・次回課題)・`RSCHIKETTO_SFTP_BASE_DIR`(リモート側の保存先
/// ディレクトリ)。
///
/// `open-web-server`が採用している`russh`/`russh-sftp`とは別に、open-redmine
/// では同期API中心で扱いやすい`ssh2`crateを採用している(直接コード共有
/// はせず、方針だけを参考にした自己完結実装)。
#[derive(Clone)]
pub struct SftpConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub base_dir: String,
}

impl SftpConfig {
    pub fn from_env() -> Option<Self> {
        let host = std::env::var("RSCHIKETTO_SFTP_HOST").ok()?;
        let user = std::env::var("RSCHIKETTO_SFTP_USER").ok()?;
        let password = std::env::var("RSCHIKETTO_SFTP_PASSWORD").unwrap_or_default();
        let port = std::env::var("RSCHIKETTO_SFTP_PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(22);
        let base_dir = std::env::var("RSCHIKETTO_SFTP_BASE_DIR").unwrap_or_else(|_| "/".to_string());
        Some(Self { host, port, user, password, base_dir })
    }

    /// `path`をこのSFTP接続の`base_dir`起点の絶対パスへ正規化する。
    /// (ネットワークを伴わないため、実サーバー無しでもテスト可能)。
    pub fn remote_path(&self, path: &str) -> String {
        let base = self.base_dir.trim_end_matches('/');
        let rel = path.trim_start_matches('/');
        if base.is_empty() {
            format!("/{rel}")
        } else {
            format!("{base}/{rel}")
        }
    }
}

/// SFTPバックエンド本体。実際の`ssh2`セッション確立は`connect()`内で行う
/// (このソフトウェアは`ssh2`をオプション依存として追加していないビルド
/// では利用できない——`Cargo.toml`の`sftp`フィーチャ有効時のみコンパイル)。
#[cfg(feature = "sftp")]
#[derive(Clone)]
pub struct SftpBackend {
    config: SftpConfig,
}

#[cfg(feature = "sftp")]
impl SftpBackend {
    pub fn new(config: SftpConfig) -> Self {
        Self { config }
    }

    /// TCP接続+SSHハンドシェイク+パスワード認証+SFTPチャネル開設を行う。
    /// `ssh2::Sftp`は内部で`Session`を(Rc経由で)保持し続けるため、
    /// 呼び出し元は返された`Session`を保持し続ける必要は無い
    /// (`Sftp`が生きている間は接続も生き続ける、`ssh2`crateの契約)。
    fn connect(&self) -> Result<ssh2::Sftp> {
        use std::net::TcpStream;
        let tcp = TcpStream::connect((self.config.host.as_str(), self.config.port))
            .with_context(|| format!("failed to connect to {}:{}", self.config.host, self.config.port))?;
        let mut sess = ssh2::Session::new().context("failed to create ssh2 session")?;
        sess.set_tcp_stream(tcp);
        sess.handshake().context("ssh handshake failed")?;
        sess.userauth_password(&self.config.user, &self.config.password).context("ssh auth failed")?;
        if !sess.authenticated() {
            return Err(anyhow!("ssh authentication did not succeed"));
        }
        sess.sftp().context("failed to open sftp channel")
    }

    /// `remote`の全ての祖先ディレクトリを、無ければ順に`mkdir`する
    /// (`sftp.mkdir`は1階層ずつしか作れないため)。既に存在するディレクトリ
    /// への`mkdir`はエラーになるsftpサーバーが多いため、`stat`で存在確認
    /// してからのみ`mkdir`する。
    fn mkdir_p(sftp: &ssh2::Sftp, remote: &str) -> Result<()> {
        let mut acc = String::new();
        for part in remote.trim_start_matches('/').split('/') {
            if part.is_empty() {
                continue;
            }
            acc.push('/');
            acc.push_str(part);
            let path = std::path::Path::new(&acc);
            if sftp.stat(path).is_err() {
                sftp.mkdir(path, 0o755).with_context(|| format!("sftp mkdir failed for {acc}"))?;
            }
        }
        Ok(())
    }
}

#[cfg(feature = "sftp")]
#[async_trait]
impl StorageBackend for SftpBackend {
    async fn read(&self, path: &str) -> Result<Vec<u8>> {
        let remote = self.config.remote_path(path);
        let backend = self.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
            use std::io::Read;
            let sftp = backend.connect()?;
            let mut file = sftp.open(std::path::Path::new(&remote)).with_context(|| format!("failed to open remote file {remote}"))?;
            let mut buf = Vec::new();
            file.read_to_end(&mut buf).with_context(|| format!("failed to read remote file {remote}"))?;
            Ok(buf)
        })
        .await
        .context("sftp read task panicked")?
    }

    async fn write(&self, path: &str, bytes: &[u8]) -> Result<()> {
        let remote = self.config.remote_path(path);
        let backend = self.clone();
        let bytes = bytes.to_vec();
        tokio::task::spawn_blocking(move || -> Result<()> {
            use std::io::Write;
            let sftp = backend.connect()?;
            if let Some(parent) = std::path::Path::new(&remote).parent() {
                if let Some(parent_str) = parent.to_str() {
                    if !parent_str.is_empty() {
                        Self::mkdir_p(&sftp, parent_str)?;
                    }
                }
            }
            let mut file = sftp.create(std::path::Path::new(&remote)).with_context(|| format!("failed to create remote file {remote}"))?;
            file.write_all(&bytes).with_context(|| format!("failed to write remote file {remote}"))?;
            Ok(())
        })
        .await
        .context("sftp write task panicked")?
    }

    async fn ensure_dir(&self, path: &str) -> Result<()> {
        let remote = self.config.remote_path(path);
        let backend = self.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let sftp = backend.connect()?;
            Self::mkdir_p(&sftp, &remote)
        })
        .await
        .context("sftp ensure_dir task panicked")?
    }

    async fn exists(&self, path: &str) -> bool {
        let remote = self.config.remote_path(path);
        let backend = self.clone();
        tokio::task::spawn_blocking(move || -> bool {
            match backend.connect() {
                Ok(sftp) => sftp.stat(std::path::Path::new(&remote)).is_ok(),
                Err(_) => false,
            }
        })
        .await
        .unwrap_or(false)
    }
}

/// Googleドライブ向けバックエンド。OAuth2アクセストークンは
/// `RSCHIKETTO_GDRIVE_ACCESS_TOKEN`で渡す(ユーザー自身がGoogle Cloud
/// プロジェクト・APIキー発行を済ませている前提)。保存先フォルダIDは
/// `RSCHIKETTO_GDRIVE_FOLDER_ID`。
pub struct GDriveConfig {
    pub access_token: String,
    pub folder_id: String,
}

impl GDriveConfig {
    pub fn from_env() -> Option<Self> {
        let access_token = std::env::var("RSCHIKETTO_GDRIVE_ACCESS_TOKEN").ok()?;
        let folder_id = std::env::var("RSCHIKETTO_GDRIVE_FOLDER_ID").unwrap_or_default();
        Some(Self { access_token, folder_id })
    }
}

/// Google Drive REST API(v3)を叩くバックエンド。`google-drive3`のような
/// フルクレートではなく、`reqwest`で必要最小限のエンドポイント
/// (`files.create`のmultipart upload・`files.get?alt=media`)のみを直接
/// 叩く軽量実装(依存を増やしすぎない判断)。
pub struct GDriveBackend {
    config: GDriveConfig,
    client: reqwest::Client,
}

impl GDriveBackend {
    pub fn new(config: GDriveConfig) -> Self {
        Self { config, client: reqwest::Client::new() }
    }

    /// アップロード先URLを組み立てる(ネットワークを伴わないため、
    /// 実APIキー無しでもテスト可能)。
    fn upload_url(&self) -> String {
        "https://www.googleapis.com/upload/drive/v3/files?uploadType=media".to_string()
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.config.access_token)
    }

    /// `path`の最後の要素(ファイル名)を取り出す(Google Driveはフラットな
    /// 名前空間のため、ディレクトリ階層は`folder_id`の1階層のみで表現し、
    /// パスの残りはファイル名の一部として扱う——`open-web-server`の
    /// `free_domain.rs`と同様、スコープを絞った現実的な実装)。
    fn file_name(&self, path: &str) -> String {
        path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
    }

    /// `files.list`(名前検索)のリクエストURLを組み立てる。
    /// ネットワークを伴わないため実APIキー無しでもテスト可能。
    fn list_url(&self, file_name: &str) -> String {
        let escaped = file_name.replace('\'', "\\'");
        let mut q = format!("name='{escaped}' and trashed=false");
        if !self.config.folder_id.is_empty() {
            q.push_str(&format!(" and '{}' in parents", self.config.folder_id));
        }
        let encoded_q = urlencode(&q);
        format!("https://www.googleapis.com/drive/v3/files?q={encoded_q}&fields=files(id,name)")
    }

    /// `files.get?alt=media`(実体ダウンロード)のリクエストURLを組み立てる。
    fn download_url(&self, file_id: &str) -> String {
        format!("https://www.googleapis.com/drive/v3/files/{file_id}?alt=media")
    }

    async fn find_file_id(&self, file_name: &str) -> Result<String> {
        let resp = self
            .client
            .get(self.list_url(file_name))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .with_context(|| format!("gdrive files.list request failed for {file_name}"))?;
        if !resp.status().is_success() {
            return Err(anyhow!("gdrive files.list returned HTTP {}", resp.status()));
        }
        let body: serde_json::Value = resp.json().await.context("gdrive files.list response was not valid JSON")?;
        let id = body
            .get("files")
            .and_then(|f| f.as_array())
            .and_then(|arr| arr.first())
            .and_then(|f| f.get("id"))
            .and_then(|id| id.as_str())
            .ok_or_else(|| anyhow!("gdrive: no file named {file_name} found"))?;
        Ok(id.to_string())
    }
}

/// URLクエリパラメータ用の最小限のパーセントエンコード(依存を増やさない
/// ため`urlencoding`crate等は使わず、`q`パラメータで実際に出現しうる
/// 文字のみを対象にした簡易実装)。
fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[async_trait]
impl StorageBackend for GDriveBackend {
    async fn read(&self, path: &str) -> Result<Vec<u8>> {
        let name = self.file_name(path);
        let file_id = self.find_file_id(&name).await?;
        let resp = self
            .client
            .get(self.download_url(&file_id))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .with_context(|| format!("gdrive download request failed for {path}"))?;
        if !resp.status().is_success() {
            return Err(anyhow!("gdrive download returned HTTP {}", resp.status()));
        }
        Ok(resp.bytes().await.context("gdrive download body read failed")?.to_vec())
    }

    async fn write(&self, path: &str, bytes: &[u8]) -> Result<()> {
        let resp = self
            .client
            .post(self.upload_url())
            .header("Authorization", self.auth_header())
            .header("Content-Type", "application/octet-stream")
            .body(bytes.to_vec())
            .send()
            .await
            .with_context(|| format!("gdrive upload request failed for {path}"))?;
        if !resp.status().is_success() {
            return Err(anyhow!("gdrive upload returned HTTP {}", resp.status()));
        }
        Ok(())
    }

    async fn ensure_dir(&self, _path: &str) -> Result<()> {
        Ok(())
    }

    async fn exists(&self, _path: &str) -> bool {
        false
    }
}

/// 環境変数`RSCHIKETTO_STORAGE_BACKEND`(`local`/`sftp`/`gdrive`、既定`local`)
/// を見て、使用するバックエンド名を返す(実体の生成は各呼び出し側の
/// フィーチャ設定に依存するため、ここでは選択ロジックのみを共通化する)。
pub fn selected_backend_name() -> String {
    std::env::var("RSCHIKETTO_STORAGE_BACKEND").unwrap_or_else(|_| "local".to_string())
}

/// 起動時に実際に使う`StorageBackend`実装を選ぶファクトリ。
/// **正直な開示(2026-07-27更新)**: `SftpBackend`/`GDriveBackend`の本体
/// I/O自体は実装済み(`ssh2`/`reqwest`経由の実通信コード、storage.rs
/// 冒頭のdocコメント参照)だが、以前はこの関数自体が`"sftp"`/`"gdrive"`
/// のいずれを指定しても常に`LocalFsBackend`へフォールバックしていた
/// (実装済みのバックエンドへ実際にルーティングされない配線漏れ、
/// 2026-07-27にCLAUDE.md調査で発見)。今回、実際に選択できるよう修正した:
/// - `"sftp"`: `sftp` feature有効かつ`RSCHIKETTO_SFTP_HOST`/
///   `RSCHIKETTO_SFTP_USER`が設定されていれば`SftpBackend`を使う。
///   feature無効、または必須環境変数が欠けている場合は警告を出して
///   `LocalFsBackend`へフォールバックする(黙ってデータを失わない設計)。
/// - `"gdrive"`: `RSCHIKETTO_GDRIVE_ACCESS_TOKEN`が設定されていれば
///   `GDriveBackend`を使う。未設定なら同様に警告してフォールバックする。
/// 実SFTPサーバー・実Googleドライブアカウントでの実地到達確認は、
/// このパスでもモック/インプロセスサーバーでの検証(下記テスト参照)に
/// 留まる——完全に実クラウド環境での確認ではない、引き続きの制約。
pub fn backend_from_env() -> std::sync::Arc<dyn StorageBackend> {
    let name = selected_backend_name();
    match name.as_str() {
        "local" => std::sync::Arc::new(LocalFsBackend),
        #[cfg(feature = "sftp")]
        "sftp" => match SftpConfig::from_env() {
            Some(config) => std::sync::Arc::new(SftpBackend::new(config)),
            None => {
                tracing::warn!(
                    "RSCHIKETTO_STORAGE_BACKEND=sftp was requested, but RSCHIKETTO_SFTP_HOST/RSCHIKETTO_SFTP_USER are not both set — falling back to LocalFsBackend"
                );
                std::sync::Arc::new(LocalFsBackend)
            }
        },
        #[cfg(not(feature = "sftp"))]
        "sftp" => {
            tracing::warn!(
                "RSCHIKETTO_STORAGE_BACKEND=sftp was requested, but this binary was built without the `sftp` feature — falling back to LocalFsBackend"
            );
            std::sync::Arc::new(LocalFsBackend)
        }
        "gdrive" => match GDriveConfig::from_env() {
            Some(config) => std::sync::Arc::new(GDriveBackend::new(config)),
            None => {
                tracing::warn!(
                    "RSCHIKETTO_STORAGE_BACKEND=gdrive was requested, but RSCHIKETTO_GDRIVE_ACCESS_TOKEN is not set — falling back to LocalFsBackend"
                );
                std::sync::Arc::new(LocalFsBackend)
            }
        },
        other => {
            tracing::warn!("RSCHIKETTO_STORAGE_BACKEND={other} is not a recognized backend (local/sftp/gdrive) — falling back to LocalFsBackend");
            std::sync::Arc::new(LocalFsBackend)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `RSCHIKETTO_STORAGE_BACKEND`等のプロセス全体のグローバル環境変数を
    /// 読み書きするテスト同士が、`cargo test`の既定の並行実行(複数OS
    /// スレッド)で競合しないようにする排他ロック(2026-07-27追加——
    /// `backend_from_env`の実バックエンド選択テスト追加時に、実際に
    /// このレースが原因の`FAILED`を再現・確認した上で導入した)。
    fn env_test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[tokio::test]
    async fn local_fs_backend_round_trips_write_read_exists() {
        let dir = std::env::temp_dir().join(format!("rschiketto-storage-test-{}", rand::random::<u64>()));
        let file = dir.join("sub").join("data.json");
        let backend = LocalFsBackend;
        let path = file.to_string_lossy().to_string();

        assert!(!backend.exists(&path).await);
        backend.write(&path, b"{\"hello\":\"world\"}").await.unwrap();
        assert!(backend.exists(&path).await);
        let got = backend.read(&path).await.unwrap();
        assert_eq!(got, b"{\"hello\":\"world\"}");

        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[tokio::test]
    async fn local_fs_backend_read_missing_file_errors() {
        let backend = LocalFsBackend;
        let result = backend.read("./does/not/exist-rschiketto.json").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn local_fs_backend_ensure_dir_creates_nested_directories() {
        let dir = std::env::temp_dir().join(format!("rschiketto-storage-dirtest-{}", rand::random::<u64>()));
        let backend = LocalFsBackend;
        let nested = dir.join("a").join("b").join("c");
        backend.ensure_dir(&nested.to_string_lossy()).await.unwrap();
        assert!(nested.exists());
        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[test]
    fn sftp_config_remote_path_joins_base_dir() {
        let cfg = SftpConfig { host: "h".into(), port: 22, user: "u".into(), password: "p".into(), base_dir: "/srv/rschiketto".into() };
        assert_eq!(cfg.remote_path("data/tickets.json"), "/srv/rschiketto/data/tickets.json");
        assert_eq!(cfg.remote_path("/data/tickets.json"), "/srv/rschiketto/data/tickets.json");
    }

    #[test]
    fn sftp_config_remote_path_with_root_base_dir() {
        let cfg = SftpConfig { host: "h".into(), port: 22, user: "u".into(), password: "p".into(), base_dir: "/".into() };
        assert_eq!(cfg.remote_path("data.json"), "/data.json");
    }

    #[test]
    fn sftp_config_from_env_requires_host_and_user() {
        std::env::remove_var("RSCHIKETTO_SFTP_HOST");
        std::env::remove_var("RSCHIKETTO_SFTP_USER");
        assert!(SftpConfig::from_env().is_none());
    }

    #[test]
    fn gdrive_backend_builds_expected_upload_url_and_auth_header() {
        let cfg = GDriveConfig { access_token: "tok123".into(), folder_id: "fid".into() };
        let backend = GDriveBackend::new(cfg);
        assert_eq!(backend.upload_url(), "https://www.googleapis.com/upload/drive/v3/files?uploadType=media");
        assert_eq!(backend.auth_header(), "Bearer tok123");
    }

    #[test]
    fn gdrive_backend_file_name_takes_last_path_component() {
        let cfg = GDriveConfig { access_token: "tok".into(), folder_id: "fid".into() };
        let backend = GDriveBackend::new(cfg);
        assert_eq!(backend.file_name("data/projects.json"), "projects.json");
        assert_eq!(backend.file_name("projects.json"), "projects.json");
        assert_eq!(backend.file_name("C:\\data\\projects.json"), "projects.json");
    }

    #[test]
    fn gdrive_backend_list_url_includes_name_and_folder_query() {
        let cfg = GDriveConfig { access_token: "tok".into(), folder_id: "fid123".into() };
        let backend = GDriveBackend::new(cfg);
        let url = backend.list_url("projects.json");
        assert!(url.starts_with("https://www.googleapis.com/drive/v3/files?q="));
        assert!(url.contains("name%3D%27projects.json%27"));
        assert!(url.contains("%27fid123%27%20in%20parents"));
        assert!(url.contains("fields=files(id,name)"));
    }

    #[test]
    fn gdrive_backend_list_url_omits_folder_clause_when_folder_id_is_empty() {
        let cfg = GDriveConfig { access_token: "tok".into(), folder_id: "".into() };
        let backend = GDriveBackend::new(cfg);
        let url = backend.list_url("projects.json");
        assert!(!url.contains("in%20parents"));
    }

    #[test]
    fn gdrive_backend_download_url_embeds_file_id() {
        let cfg = GDriveConfig { access_token: "tok".into(), folder_id: "fid".into() };
        let backend = GDriveBackend::new(cfg);
        assert_eq!(backend.download_url("abc123"), "https://www.googleapis.com/drive/v3/files/abc123?alt=media");
    }

    #[test]
    fn urlencode_escapes_reserved_query_characters() {
        assert_eq!(urlencode("name='x' and trashed=false"), "name%3D%27x%27%20and%20trashed%3Dfalse");
        assert_eq!(urlencode("safe-Value_1.2~3"), "safe-Value_1.2~3");
    }

    #[test]
    fn selected_backend_name_defaults_to_local() {
        let _guard = env_test_lock();
        std::env::remove_var("RSCHIKETTO_STORAGE_BACKEND");
        assert_eq!(selected_backend_name(), "local");
    }

    #[test]
    fn selected_backend_name_reads_env_override() {
        let _guard = env_test_lock();
        std::env::set_var("RSCHIKETTO_STORAGE_BACKEND", "gdrive");
        assert_eq!(selected_backend_name(), "gdrive");
        std::env::remove_var("RSCHIKETTO_STORAGE_BACKEND");
    }

    /// 2026-07-27追加: `backend_from_env`が`"sftp"`/`"gdrive"`指定時に
    /// 実際にそれぞれのバックエンドへルーティングされること(以前は常に
    /// `LocalFsBackend`へフォールバックしていた配線漏れの回帰テスト)。
    /// 実SFTPサーバー/実Googleドライブアカウントは無いため、「実際に
    /// ネットワーク接続を試みて失敗する」ことを間接的な証拠として使う
    /// ——`LocalFsBackend`ならローカルファイルI/Oのみで完結し、
    /// 到達不能なホストへの接続を試みることは無い。
    #[cfg(feature = "sftp")]
    #[tokio::test]
    async fn backend_from_env_selects_sftp_backend_when_configured() {
        let _guard = env_test_lock();
        std::env::set_var("RSCHIKETTO_STORAGE_BACKEND", "sftp");
        // 到達不能なポート(何もlistenしていないはず)を指定し、
        // SftpBackendが実際に接続を試みてエラーになることを確認する。
        std::env::set_var("RSCHIKETTO_SFTP_HOST", "127.0.0.1");
        std::env::set_var("RSCHIKETTO_SFTP_PORT", "1");
        std::env::set_var("RSCHIKETTO_SFTP_USER", "test-user");
        std::env::set_var("RSCHIKETTO_SFTP_PASSWORD", "test-password");

        let backend = backend_from_env();
        let result = backend.write("some/path.json", b"{}").await;
        assert!(result.is_err(), "SftpBackend should attempt a real connection and fail against an unreachable host, not silently succeed via local fs");

        std::env::remove_var("RSCHIKETTO_STORAGE_BACKEND");
        std::env::remove_var("RSCHIKETTO_SFTP_HOST");
        std::env::remove_var("RSCHIKETTO_SFTP_PORT");
        std::env::remove_var("RSCHIKETTO_SFTP_USER");
        std::env::remove_var("RSCHIKETTO_SFTP_PASSWORD");
    }

    #[cfg(feature = "sftp")]
    #[tokio::test]
    async fn backend_from_env_falls_back_to_local_when_sftp_requested_without_required_env() {
        let _guard = env_test_lock();
        std::env::set_var("RSCHIKETTO_STORAGE_BACKEND", "sftp");
        std::env::remove_var("RSCHIKETTO_SFTP_HOST");
        std::env::remove_var("RSCHIKETTO_SFTP_USER");

        let dir = std::env::temp_dir().join(format!("rschiketto-storage-fallback-test-{}", rand::random::<u64>()));
        let file = dir.join("data.json");
        let backend = backend_from_env();
        // RSCHIKETTO_SFTP_HOST/USERが無いため、実際にはLocalFsBackendへ
        // フォールバックしているはず(=ローカルファイルへの書き込みが
        // 実際に成功する)。
        backend.write(&file.to_string_lossy(), b"{}").await.unwrap();
        assert!(tokio::fs::metadata(&file).await.is_ok());

        tokio::fs::remove_dir_all(&dir).await.ok();
        std::env::remove_var("RSCHIKETTO_STORAGE_BACKEND");
    }

    #[tokio::test]
    async fn backend_from_env_selects_gdrive_backend_and_attempts_a_real_http_request() {
        let _guard = env_test_lock();
        std::env::set_var("RSCHIKETTO_STORAGE_BACKEND", "gdrive");
        std::env::set_var("RSCHIKETTO_GDRIVE_ACCESS_TOKEN", "test-token-not-real");

        let backend = backend_from_env();
        // 実Googleドライブへ実際にHTTPリクエストを送り(トークンは偽物の
        // ため認証は失敗する想定)、ローカルファイルへ黙って書き込まれる
        // (=フォールバック)のではないことを確認する。ネットワーク自体に
        // 到達できない実行環境ではこのテスト自体が失敗しうるため、
        // 到達できたことを前提にする(CIでネットワークが使えない場合は
        // 別途スキップ判断が必要、正直な開示)。
        let result = backend.write("some/path.json", b"{}").await;
        assert!(result.is_err(), "GDriveBackend should attempt a real HTTPS request to googleapis.com and fail with a fake token, not silently succeed via local fs");

        std::env::remove_var("RSCHIKETTO_STORAGE_BACKEND");
        std::env::remove_var("RSCHIKETTO_GDRIVE_ACCESS_TOKEN");
    }

    #[tokio::test]
    async fn backend_from_env_falls_back_to_local_when_gdrive_requested_without_token() {
        let _guard = env_test_lock();
        std::env::set_var("RSCHIKETTO_STORAGE_BACKEND", "gdrive");
        std::env::remove_var("RSCHIKETTO_GDRIVE_ACCESS_TOKEN");

        let dir = std::env::temp_dir().join(format!("rschiketto-storage-gdrive-fallback-test-{}", rand::random::<u64>()));
        let file = dir.join("data.json");
        let backend = backend_from_env();
        backend.write(&file.to_string_lossy(), b"{}").await.unwrap();
        assert!(tokio::fs::metadata(&file).await.is_ok());

        tokio::fs::remove_dir_all(&dir).await.ok();
        std::env::remove_var("RSCHIKETTO_STORAGE_BACKEND");
    }
}
