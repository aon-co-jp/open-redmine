//! 固定IPを持たない自宅サーバー/レンタルサーバー等向けの、簡易DDNS更新。
//!
//! `open-web-server`(`crates/open-web-server-gateway/src/ddns.rs`)と同じ
//! 設計パターンを、直接依存はせずopen-redmine側で自己完結実装したもの
//! (汎用URLテンプレート方式、`{ip}`プレースホルダ、`api.ipify.org`での
//! グローバルIP検知、変化時のみ更新)。
//!
//! 使い方: `RSCHIKETTO_DDNS_UPDATE_URL`に、現在のグローバルIPを埋め込み
//! たい箇所を`{ip}`と書いたURLを設定する。例(DuckDNS):
//! `https://www.duckdns.org/update?domains=myhost&token=xxxx&ip={ip}`
//!
//! 既定は無効(オプトイン)。固定IP環境では不要な機能のため。
//!
//! **正直な開示(Android)**: Android版でこの常駐DDNS更新が実際に動くのは、
//! `open-web-server`と同様にAPK化(未着手)が完了してから。open-redmine自体は
//! 現状Windows/Linux向けのネイティブバイナリとして動作する設計であり、
//! このモジュールもそのネイティブバイナリ上でのみ動作を検証している。

use std::time::Duration;

const CHECK_INTERVAL: Duration = Duration::from_secs(5 * 60);
const IP_ECHO_URL: &str = "https://api.ipify.org";

/// 環境変数`RSCHIKETTO_DDNS_UPDATE_URL`が設定されていれば、バックグラウンド
/// タスクとして定期的(既定5分ごと)にグローバルIPを確認し、前回から
/// 変化していれば更新URLを叩く。設定が無ければ何もしない。
pub fn spawn_if_configured() {
    let Ok(template) = std::env::var("RSCHIKETTO_DDNS_UPDATE_URL") else {
        return;
    };
    if !template.contains("{ip}") {
        tracing::warn!(
            "RSCHIKETTO_DDNS_UPDATE_URL is set but doesn't contain '{{ip}}' placeholder; DDNS updates disabled"
        );
        return;
    }
    tracing::info!("DDNS: enabled, checking every {:?}", CHECK_INTERVAL);
    tokio::spawn(run_loop(template));
}

async fn run_loop(template: String) {
    let client = reqwest::Client::new();
    let mut last_ip: Option<String> = None;
    loop {
        match fetch_current_ip(&client).await {
            Ok(ip) => {
                if last_ip.as_deref() != Some(ip.as_str()) {
                    tracing::info!("DDNS: detected IP change (was {:?}, now {ip}), updating", last_ip);
                    match update_ddns(&client, &template, &ip).await {
                        Ok(status) if status.is_success() => {
                            tracing::info!("DDNS: update succeeded (HTTP {status})");
                            last_ip = Some(ip);
                        }
                        Ok(status) => tracing::warn!("DDNS: update endpoint returned HTTP {status}"),
                        Err(e) => tracing::warn!("DDNS: update request failed: {e}"),
                    }
                }
            }
            Err(e) => tracing::warn!("DDNS: failed to fetch current IP: {e}"),
        }
        tokio::time::sleep(CHECK_INTERVAL).await;
    }
}

async fn fetch_current_ip(client: &reqwest::Client) -> Result<String, reqwest::Error> {
    let text = client.get(IP_ECHO_URL).send().await?.text().await?;
    Ok(text.trim().to_string())
}

async fn update_ddns(client: &reqwest::Client, template: &str, ip: &str) -> Result<reqwest::StatusCode, reqwest::Error> {
    let url = template.replace("{ip}", ip);
    let resp = client.get(&url).send().await?;
    Ok(resp.status())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_template_substitution_replaces_placeholder() {
        let template = "https://example.com/update?ip={ip}&host=test";
        let expected = "https://example.com/update?ip=203.0.113.5&host=test";
        assert_eq!(template.replace("{ip}", "203.0.113.5"), expected);
    }

    #[test]
    fn spawn_if_configured_is_a_noop_without_env_var() {
        std::env::remove_var("RSCHIKETTO_DDNS_UPDATE_URL");
        spawn_if_configured();
    }

    #[test]
    fn spawn_if_configured_warns_and_noops_without_placeholder() {
        std::env::set_var("RSCHIKETTO_DDNS_UPDATE_URL", "https://example.com/update?ip=static");
        spawn_if_configured();
        std::env::remove_var("RSCHIKETTO_DDNS_UPDATE_URL");
    }
}
