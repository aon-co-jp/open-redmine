//! GitHubリポジトリ連携(Redmine本家のSCM/リポジトリ連携相当、
//! 2026-07-31追加)。プロジェクトに紐付けた`"owner/repo"`から、GitHub
//! REST APIで直近のコミット一覧を取得する。書き込み系の連携(Webhook
//! 受信、issueへのコミット自動リンク永続化等)は対象外——読み取り専用の
//! 一覧表示のみの最小実装(正直な開示)。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
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
fn parse_referenced_ticket_ids(message: &str) -> Vec<u64> {
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
}
