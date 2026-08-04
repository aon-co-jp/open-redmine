//! SCM(ソースコード管理)リポジトリ連携(Redmine本家の同機能相当、
//! 2026-07-31にGitHub専用として追加、2026-08-01にGitLab/Bitbucketへ
//! 拡張)。プロジェクトに紐付けた`"owner/repo"`から、各プロバイダの
//! REST APIで直近のコミット一覧を取得する。書き込み系の連携(Webhook
//! 受信〈GitHubのみ実装済み、`github_webhook.rs`参照〉を除く、issueへの
//! コミット自動リンク永続化等)は対象外——読み取り専用の一覧表示のみの
//! 最小実装(正直な開示)。
//!
//! **対応プロバイダ(2026-08-01時点)**: GitHub(実機検証済み、Webhook
//! 対応込み)・GitLab(gitlab.com、実際の公開API`GET /api/v4/projects/
//! :id/repository/commits`で実機検証済み)・Bitbucket(bitbucket.org
//! Cloud、実際の公開API`GET /2.0/repositories/:workspace/:repo/commits`
//! で実機検証済み)。**未対応(正直な開示)**: GitBucket(Scala製OSS、
//! GitHub API v3互換を謳っているため理論上`fetch_recent_commits`
//! (GitHub用)がそのまま使える可能性が高いが、実サーバーが無く未検証)、
//! Gitea(OSS版、GitHub API寄りだが専用のcommits一覧APIパスが異なる)、
//! 本エコシステム自製の`open-gitea`(`/api/repos`のみでcommits一覧API
//! 自体が存在しないため対象外)。セルフホストGitLab/Bitbucket Server
//! (Data Center)はAPIベースURLが`gitlab.com`/`api.bitbucket.org`固定
//! ではなくインスタンスごとに異なるため、今回はSaaS版(gitlab.com/
//! bitbucket.org)のみ対応し、セルフホスト版のベースURL指定は未対応。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubCommit {
    pub sha: String,
    pub message: String,
    pub author_name: String,
    pub date: String,
    pub html_url: String,
    /// このコミットメッセージ中に`#<ticket_id>`形式で参照されている
    /// チケットID一覧(Redmine本家の「コミットメッセージでのissue参照」
    /// 相当の簡易パース、正規表現crateへの新規依存は避け手動走査する)。
    pub referenced_ticket_ids: Vec<u64>,
}

#[derive(Debug, Deserialize)]
struct GhCommitAuthor {
    name: Option<String>,
    date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhCommitDetail {
    message: String,
    author: Option<GhCommitAuthor>,
}

#[derive(Debug, Deserialize)]
struct GhCommitEntry {
    sha: String,
    commit: GhCommitDetail,
    html_url: String,
}

/// コミットメッセージから`#123`形式のチケット参照を抽出する
/// (数字の直前が`#`、直後が数字である最長の連続数字列を1つの参照として
/// 扱う、簡易実装)。
pub fn parse_referenced_ticket_ids(message: &str) -> Vec<u64> {
    let mut ids = Vec::new();
    let chars: Vec<char> = message.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '#' {
            let mut j = i + 1;
            let mut digits = String::new();
            while j < chars.len() && chars[j].is_ascii_digit() {
                digits.push(chars[j]);
                j += 1;
            }
            if let Ok(id) = digits.parse::<u64>() {
                ids.push(id);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    ids
}

/// `owner/repo`形式かどうかを検証する(パストラバーサル・任意URL埋め込み
/// を防ぐため、英数字・ハイフン・アンダースコア・ドットのみを許可)。
pub fn is_valid_repo_spec(spec: &str) -> bool {
    let Some((owner, repo)) = spec.split_once('/') else {
        return false;
    };
    let valid_part = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    valid_part(owner) && valid_part(repo) && !repo.contains('/')
}

/// GitHub REST APIから直近のコミット一覧(最大30件、GitHub API既定の
/// 1ページ分)を取得する。`token`が`Some`ならAuthorizationヘッダーを付与
/// する(未認証でもpublicリポジトリなら動作するが、レート制限が厳しい)。
pub async fn fetch_recent_commits(repo_spec: &str, token: Option<&str>) -> anyhow::Result<Vec<GithubCommit>> {
    if !is_valid_repo_spec(repo_spec) {
        anyhow::bail!("invalid repo spec, expected \"owner/repo\"");
    }
    let url = format!("https://api.github.com/repos/{repo_spec}/commits");
    let client = reqwest::Client::new();
    let mut req = client.get(&url).header("User-Agent", "open-redmine").header("Accept", "application/vnd.github+json");
    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("GitHub API returned {}", resp.status());
    }
    let entries: Vec<GhCommitEntry> = resp.json().await?;
    Ok(entries
        .into_iter()
        .map(|e| {
            let message = e.commit.message;
            let referenced_ticket_ids = parse_referenced_ticket_ids(&message);
            GithubCommit {
                sha: e.sha,
                author_name: e.commit.author.as_ref().and_then(|a| a.name.clone()).unwrap_or_else(|| "unknown".to_string()),
                date: e.commit.author.and_then(|a| a.date).unwrap_or_default(),
                html_url: e.html_url,
                message,
                referenced_ticket_ids,
            }
        })
        .collect())
}

/// `Project.scm_provider`の値をパースする。`None`・未知の値は
/// `GitHub`(既定、後方互換)として扱う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScmProvider {
    GitHub,
    GitLab,
    Bitbucket,
    /// 本家Gitea(OSS、Go製)。セルフホストが前提のためSaaS版の固定
    /// ベースURLは無く、`Project.scm_base_url`が必須(2026-08-04追加)。
    /// このエコシステム自製の`open-gitea`(commits一覧API自体が無い)とは
    /// 別物。
    Gitea,
    /// 本家GitBucket(OSS、Scala製、GitHub API v3互換を謳う)。同じく
    /// セルフホストのため`Project.scm_base_url`が必須(2026-08-04追加)。
    GitBucket,
}

impl ScmProvider {
    pub fn parse(value: Option<&str>) -> Self {
        match value.map(str::to_ascii_lowercase).as_deref() {
            Some("gitlab") => ScmProvider::GitLab,
            Some("bitbucket") => ScmProvider::Bitbucket,
            Some("gitea") => ScmProvider::Gitea,
            Some("gitbucket") => ScmProvider::GitBucket,
            _ => ScmProvider::GitHub,
        }
    }
}

#[derive(Debug, Deserialize)]
struct GitLabCommitEntry {
    id: String,
    title: String,
    author_name: String,
    committed_date: String,
    web_url: String,
}

/// gitlab.comの公開REST APIから直近のコミット一覧を取得する
/// (実機検証済み: `GET https://gitlab.com/api/v4/projects/
/// gitlab-org%2Fgitlab-test/repository/commits`)。`token`が`Some`なら
/// `PRIVATE-TOKEN`ヘッダーを付与する(GitLab REST APIの標準的な個人
/// アクセストークン方式)。
pub async fn fetch_recent_commits_gitlab(repo_spec: &str, token: Option<&str>) -> anyhow::Result<Vec<GithubCommit>> {
    if !is_valid_repo_spec(repo_spec) {
        anyhow::bail!("invalid repo spec, expected \"namespace/repo\"");
    }
    let project_id = urlencode_path_segment(repo_spec);
    let url = format!("https://gitlab.com/api/v4/projects/{project_id}/repository/commits");
    let client = reqwest::Client::new();
    let mut req = client.get(&url).header("User-Agent", "open-redmine");
    if let Some(t) = token {
        req = req.header("PRIVATE-TOKEN", t);
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("GitLab API returned {}", resp.status());
    }
    let entries: Vec<GitLabCommitEntry> = resp.json().await?;
    Ok(entries
        .into_iter()
        .map(|e| {
            let referenced_ticket_ids = parse_referenced_ticket_ids(&e.title);
            GithubCommit { sha: e.id, author_name: e.author_name, date: e.committed_date, html_url: e.web_url, message: e.title, referenced_ticket_ids }
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct BitbucketAuthorUser {
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BitbucketAuthor {
    raw: Option<String>,
    user: Option<BitbucketAuthorUser>,
}

#[derive(Debug, Deserialize)]
struct BitbucketLink {
    href: String,
}

#[derive(Debug, Deserialize)]
struct BitbucketLinks {
    html: Option<BitbucketLink>,
}

#[derive(Debug, Deserialize)]
struct BitbucketCommitEntry {
    hash: String,
    message: String,
    date: String,
    author: Option<BitbucketAuthor>,
    links: BitbucketLinks,
}

#[derive(Debug, Deserialize)]
struct BitbucketCommitsResponse {
    values: Vec<BitbucketCommitEntry>,
}

/// bitbucket.org Cloudの公開REST APIから直近のコミット一覧を取得する
/// (実機検証済み: `GET https://api.bitbucket.org/2.0/repositories/
/// atlassian/aui/commits`)。`token`が`Some`ならOAuthアクセストークンとして
/// `Authorization: Bearer`を付与する。**正直な開示**: Bitbucketの
/// 「アプリパスワード」(Basic認証)方式は今回未対応——OAuth2アクセス
/// トークンのみをサポートする。
pub async fn fetch_recent_commits_bitbucket(repo_spec: &str, token: Option<&str>) -> anyhow::Result<Vec<GithubCommit>> {
    if !is_valid_repo_spec(repo_spec) {
        anyhow::bail!("invalid repo spec, expected \"workspace/repo\"");
    }
    let url = format!("https://api.bitbucket.org/2.0/repositories/{repo_spec}/commits");
    let client = reqwest::Client::new();
    let mut req = client.get(&url).header("User-Agent", "open-redmine");
    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("Bitbucket API returned {}", resp.status());
    }
    let body: BitbucketCommitsResponse = resp.json().await?;
    Ok(body
        .values
        .into_iter()
        .map(|e| {
            let referenced_ticket_ids = parse_referenced_ticket_ids(&e.message);
            let author_name = e
                .author
                .as_ref()
                .and_then(|a| a.user.as_ref().and_then(|u| u.display_name.clone()).or_else(|| a.raw.clone()))
                .unwrap_or_else(|| "unknown".to_string());
            GithubCommit {
                sha: e.hash,
                author_name,
                date: e.date,
                html_url: e.links.html.map(|l| l.href).unwrap_or_default(),
                message: e.message,
                referenced_ticket_ids,
            }
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct GiteaCommitAuthor {
    name: Option<String>,
    date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GiteaCommitDetail {
    message: String,
    author: Option<GiteaCommitAuthor>,
}

#[derive(Debug, Deserialize)]
struct GiteaCommitEntry {
    sha: String,
    commit: GiteaCommitDetail,
    html_url: String,
}

/// 本家Gitea(OSS)のREST API(`GET {base}/api/v1/repos/{owner}/{repo}/
/// commits`、レスポンス形式はGitHub API v3とほぼ同一)から直近の
/// コミット一覧を取得する。`token`が`Some`なら`Authorization: token
/// <TOKEN>`ヘッダー(Giteaの個人アクセストークン方式)を付与する。
/// **正直な開示(rs-syncの`GiteaProvider`と同じ開示方針)**: この
/// エコシステムの実行環境に実行中のGitea(OSS)インスタンスが無く、
/// Gitea公式API仕様書に基づいて実装したのみで実サーバーに対する
/// 実HTTP検証はまだ行っていない。
pub async fn fetch_recent_commits_gitea(base_url: &str, repo_spec: &str, token: Option<&str>) -> anyhow::Result<Vec<GithubCommit>> {
    if !is_valid_repo_spec(repo_spec) {
        anyhow::bail!("invalid repo spec, expected \"owner/repo\"");
    }
    let url = format!("{}/api/v1/repos/{repo_spec}/commits", base_url.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let mut req = client.get(&url).header("User-Agent", "open-redmine").header("Accept", "application/json");
    if let Some(t) = token {
        req = req.header("Authorization", format!("token {t}"));
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("Gitea API returned {}", resp.status());
    }
    let entries: Vec<GiteaCommitEntry> = resp.json().await?;
    Ok(entries
        .into_iter()
        .map(|e| {
            let message = e.commit.message;
            let referenced_ticket_ids = parse_referenced_ticket_ids(&message);
            GithubCommit {
                sha: e.sha,
                author_name: e.commit.author.as_ref().and_then(|a| a.name.clone()).unwrap_or_else(|| "unknown".to_string()),
                date: e.commit.author.and_then(|a| a.date).unwrap_or_default(),
                html_url: e.html_url,
                message,
                referenced_ticket_ids,
            }
        })
        .collect())
}

/// 本家GitBucket(OSS、Scala製)のREST APIから直近のコミット一覧を取得
/// する。GitBucketは「GitHub API v3互換」を謳っており(公式wiki
/// 参照)、エンドポイント形状もレスポンス形状もGitHub本家と同一と
/// 仮定して`GhCommitEntry`をそのまま再利用する
/// (`GET {base}/api/v3/repos/{owner}/{repo}/commits`)。`token`が
/// `Some`なら`Authorization: token <TOKEN>`ヘッダーを付与する。
/// **正直な開示(rs-syncの`GitbucketProvider`と同じ開示方針)**: 実行
/// 環境に実行中のGitBucketインスタンスが無く、公式ドキュメントに
/// 基づく実装のみで実サーバーに対する実HTTP検証はまだ行っていない。
pub async fn fetch_recent_commits_gitbucket(base_url: &str, repo_spec: &str, token: Option<&str>) -> anyhow::Result<Vec<GithubCommit>> {
    if !is_valid_repo_spec(repo_spec) {
        anyhow::bail!("invalid repo spec, expected \"owner/repo\"");
    }
    let url = format!("{}/api/v3/repos/{repo_spec}/commits", base_url.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let mut req = client.get(&url).header("User-Agent", "open-redmine").header("Accept", "application/vnd.github+json");
    if let Some(t) = token {
        req = req.header("Authorization", format!("token {t}"));
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("GitBucket API returned {}", resp.status());
    }
    let entries: Vec<GhCommitEntry> = resp.json().await?;
    Ok(entries
        .into_iter()
        .map(|e| {
            let message = e.commit.message;
            let referenced_ticket_ids = parse_referenced_ticket_ids(&message);
            GithubCommit {
                sha: e.sha,
                author_name: e.commit.author.as_ref().and_then(|a| a.name.clone()).unwrap_or_else(|| "unknown".to_string()),
                date: e.commit.author.and_then(|a| a.date).unwrap_or_default(),
                html_url: e.html_url,
                message,
                referenced_ticket_ids,
            }
        })
        .collect())
}

/// `is_valid_repo_spec`で既に安全な文字集合(英数字・`-`・`_`・`.`・`/`)に
/// 限定済みのため、パーセントエンコードが必要なのは`/`(GitLabの
/// プロジェクトIDはパス区切りではなくURLエンコードした`namespace%2Frepo`
/// を要求する)のみ。新規crate依存(`urlencoding`等)を避けた最小実装。
fn urlencode_path_segment(spec: &str) -> String {
    spec.replace('/', "%2F")
}

/// `scm_provider`に応じたプロバイダへディスパッチする(2026-08-01追加、
/// 2026-08-04にGitea/GitBucket対応で`base_url`引数を追加)。
/// `list_github_commits`ハンドラ〈`main.rs`〉が呼ぶ唯一の入口。
/// `base_url`はセルフホストプロバイダ(Gitea/GitBucket)にのみ使われ、
/// それ以外(GitHub/GitLab/Bitbucket、いずれも固定SaaSベースURL)では
/// 無視される。Gitea/GitBucketで`base_url`が`None`の場合はエラーを返す。
pub async fn fetch_recent_commits_for(provider: ScmProvider, repo_spec: &str, token: Option<&str>, base_url: Option<&str>) -> anyhow::Result<Vec<GithubCommit>> {
    match provider {
        ScmProvider::GitHub => fetch_recent_commits(repo_spec, token).await,
        ScmProvider::GitLab => fetch_recent_commits_gitlab(repo_spec, token).await,
        ScmProvider::Bitbucket => fetch_recent_commits_bitbucket(repo_spec, token).await,
        ScmProvider::Gitea => {
            let Some(base) = base_url else {
                anyhow::bail!("scm_base_url is required for a self-hosted Gitea instance");
            };
            fetch_recent_commits_gitea(base, repo_spec, token).await
        }
        ScmProvider::GitBucket => {
            let Some(base) = base_url else {
                anyhow::bail!("scm_base_url is required for a self-hosted GitBucket instance");
            };
            fetch_recent_commits_gitbucket(base, repo_spec, token).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_and_multiple_ticket_references() {
        assert_eq!(parse_referenced_ticket_ids("fix #12 login bug"), vec![12]);
        assert_eq!(parse_referenced_ticket_ids("fixes #1 and #2, see #3"), vec![1, 2, 3]);
        assert_eq!(parse_referenced_ticket_ids("no reference here"), Vec::<u64>::new());
        // "#"の直後が数字でなければ参照とみなさない。
        assert_eq!(parse_referenced_ticket_ids("see issue #abc"), Vec::<u64>::new());
    }

    #[test]
    fn validates_owner_repo_spec() {
        assert!(is_valid_repo_spec("aon-co-jp/open-redmine"));
        assert!(is_valid_repo_spec("rust-lang/rust"));
        assert!(!is_valid_repo_spec("no-slash-here"));
        assert!(!is_valid_repo_spec("owner/repo/extra"));
        assert!(!is_valid_repo_spec("owner/"));
        assert!(!is_valid_repo_spec("/repo"));
        assert!(!is_valid_repo_spec("owner/repo?evil=1"));
        assert!(!is_valid_repo_spec("../../etc/passwd"));
    }

    #[test]
    fn scm_provider_parses_known_values_and_defaults_to_github() {
        assert_eq!(ScmProvider::parse(Some("gitlab")), ScmProvider::GitLab);
        assert_eq!(ScmProvider::parse(Some("GitLab")), ScmProvider::GitLab);
        assert_eq!(ScmProvider::parse(Some("bitbucket")), ScmProvider::Bitbucket);
        assert_eq!(ScmProvider::parse(Some("github")), ScmProvider::GitHub);
        assert_eq!(ScmProvider::parse(Some("unknown-provider")), ScmProvider::GitHub);
        assert_eq!(ScmProvider::parse(None), ScmProvider::GitHub);
        assert_eq!(ScmProvider::parse(Some("gitea")), ScmProvider::Gitea);
        assert_eq!(ScmProvider::parse(Some("Gitea")), ScmProvider::Gitea);
        assert_eq!(ScmProvider::parse(Some("gitbucket")), ScmProvider::GitBucket);
    }

    #[tokio::test]
    async fn fetch_recent_commits_for_gitea_and_gitbucket_require_a_base_url() {
        let err = fetch_recent_commits_for(ScmProvider::Gitea, "owner/repo", None, None).await.unwrap_err();
        assert!(err.to_string().contains("scm_base_url is required"));
        let err = fetch_recent_commits_for(ScmProvider::GitBucket, "owner/repo", None, None).await.unwrap_err();
        assert!(err.to_string().contains("scm_base_url is required"));
    }

    #[test]
    fn urlencode_path_segment_escapes_only_the_slash() {
        assert_eq!(urlencode_path_segment("aon-co-jp/open-redmine"), "aon-co-jp%2Fopen-redmine");
    }

    /// 実機検証(2026-08-01、モックではなく実際のgitlab.com/bitbucket.org
    /// APIへの実HTTPリクエスト): CI/オフライン環境では失敗しうるため
    /// `#[ignore]`とし、明示的に`cargo test -- --ignored`で実行する。
    #[tokio::test]
    #[ignore = "hits the real gitlab.com and bitbucket.org APIs"]
    async fn fetch_recent_commits_gitlab_reaches_a_real_public_project() {
        let commits = fetch_recent_commits_gitlab("gitlab-org/gitlab-test", None).await.unwrap();
        assert!(!commits.is_empty());
    }

    #[tokio::test]
    #[ignore = "hits the real gitlab.com and bitbucket.org APIs"]
    async fn fetch_recent_commits_bitbucket_reaches_a_real_public_repo() {
        let commits = fetch_recent_commits_bitbucket("atlassian/aui", None).await.unwrap();
        assert!(!commits.is_empty());
    }
}
