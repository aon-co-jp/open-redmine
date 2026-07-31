//! open-redmineのブラウザフロントエンド(Rust→WebAssembly、オンライン専用)。
//!
//! Tauri・Node.js・TypeScriptには依存しない(このエコシステム共通方針)。
//! ピンチズームはブラウザ標準機能そのものであり(`index.html`の
//! `viewport`メタタグが`user-scalable=no`等で無効化さえしなければ)、
//! Android/iOSのモバイルブラウザで特別な実装なしに動作する。
//!
//! チケット管理を行うWEBアプリである以上GUIは基本機能——という
//! ユーザー指示により、単なるログイン+一覧表示に留めず、チケット
//! 詳細・ステータス変更・コメント投稿・Wiki閲覧/編集まで一通り
//! 揃えている(2026-07-23)。

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Document, Element, HtmlInputElement, HtmlSelectElement, HtmlTextAreaElement, Request, RequestInit, RequestMode, Response, Storage};

const SESSION_KEY: &str = "rsred_session_token";
const EMAIL_KEY: &str = "rsred_session_email";

/// 2026-07-28追記(実バグ修正): このアプリは`easy-web.tokyo/open-redmine`
/// (open-web-serverの「分身の術」テナントルーティング、`path_prefix`剥がし
/// 転送)配下にマウントされているが、`api()`が絶対パス`/api/...`で
/// `fetch()`していたため、ブラウザは常にオリジン直下(`easy-web.tokyo/
/// api/...`)を叩いてしまい、OTP送信を含む全APIリクエストが実際には
/// 到達不能だった(実クリック操作で`POST https://easy-web.tokyo/api/
/// auth/request-otp`→400を確認して発見)。open-gitea/RS-Syncが同種の
/// 問題で採用した「マウント先を固定のプレフィックス定数として持つ」
/// 方式をここでも踏襲する。別の場所にマウントする場合はこの値を書き換える
/// こと(複数マウント先の動的対応は今回のスコープ外、正直な開示)。
const BASE_PATH: &str = "/open-redmine";

fn window() -> web_sys::Window {
    web_sys::window().expect("no global window")
}

fn document() -> Document {
    window().document().expect("no document")
}

fn local_storage() -> Storage {
    window().local_storage().ok().flatten().expect("no localStorage")
}

fn by_id(id: &str) -> Element {
    document().get_element_by_id(id).unwrap_or_else(|| panic!("missing #{id}"))
}

fn input_value(id: &str) -> String {
    by_id(id).dyn_into::<HtmlInputElement>().map(|el| el.value()).unwrap_or_default()
}

fn set_input_value(id: &str, value: &str) {
    if let Ok(el) = by_id(id).dyn_into::<HtmlInputElement>() {
        el.set_value(value);
    }
}

fn textarea_value(id: &str) -> String {
    by_id(id).dyn_into::<HtmlTextAreaElement>().map(|el| el.value()).unwrap_or_default()
}

fn set_textarea_value(id: &str, value: &str) {
    if let Ok(el) = by_id(id).dyn_into::<HtmlTextAreaElement>() {
        el.set_value(value);
    }
}

fn select_value(id: &str) -> String {
    by_id(id).dyn_into::<HtmlSelectElement>().map(|el| el.value()).unwrap_or_default()
}

fn set_text(id: &str, text: &str) {
    by_id(id).set_text_content(Some(text));
}

fn set_html(id: &str, html: &str) {
    by_id(id).set_inner_html(html);
}

fn show(id: &str, visible: bool) {
    let el = by_id(id);
    let class = el.class_list();
    if visible {
        let _ = class.remove_1("hidden");
    } else {
        let _ = class.add_1("hidden");
    }
}

fn session_token() -> Option<String> {
    local_storage().get_item(SESSION_KEY).ok().flatten()
}

/// 現在選択中のプロジェクトID。
///
/// **2026-07-27追記(実クリックE2Eで発見した実バグの修正)**: 以前は
/// 「`0`=未選択」という番兵値方式だったが、サーバー側`ProjectStore::
/// next_id`は`0`から採番されるため、**最初に作成したプロジェクト
/// (ID=0)を選択しても「未選択」と誤判定され、そのプロジェクトでは
/// 永久にチケットを作成できない**という実バグがあった(実ブラウザで
/// 最初のプロジェクトを作ってすぐ試したことで発覚——`cargo test`は
/// この番兵値の衝突を検出できない)。番兵値方式をやめ、hidden inputが
/// 空文字列/パース不能な場合のみ`None`を返す`Option<u64>`方式に変更し、
/// `0`という正当なIDと「未選択」を区別できるようにした。
fn current_project_id() -> Option<u64> {
    let raw = input_value("selected-project-id");
    if raw.trim().is_empty() {
        None
    } else {
        raw.parse().ok()
    }
}

/// 現在開いているチケットID。`current_project_id`と同じ理由(2026-07-27
/// 追記)で`0`を番兵値に使わない`Option<u64>`方式にした。
fn current_ticket_id() -> Option<u64> {
    let raw = input_value("selected-ticket-id");
    if raw.trim().is_empty() {
        None
    } else {
        raw.parse().ok()
    }
}

/// `fetch()`の薄いラッパー。`Authorization: Bearer`はセッションがあれば
/// 自動付与する。戻り値は`(status, body_text)`。
async fn api(method: &str, path: &str, body: Option<String>) -> Result<(u16, String), JsValue> {
    let mut opts = RequestInit::new();
    opts.set_method(method);
    opts.set_mode(RequestMode::SameOrigin);
    if let Some(b) = &body {
        opts.set_body(&JsValue::from_str(b));
    }
    let url = format!("{BASE_PATH}{path}");
    let request = Request::new_with_str_and_init(&url, &opts)?;
    request.headers().set("Content-Type", "application/json")?;
    if let Some(token) = session_token() {
        request.headers().set("Authorization", &format!("Bearer {token}"))?;
    }
    let resp_value = JsFuture::from(window().fetch_with_request(&request)).await?;
    let resp: Response = resp_value.dyn_into()?;
    let status = resp.status();
    let text = JsFuture::from(resp.text()?).await?.as_string().unwrap_or_default();
    Ok((status, text))
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// `"YYYY-MM-DD"`をエポック(1970-01-01)からの日数に変換する
/// (Howard Hinnantの`days_from_civil`アルゴリズム、うるう年を正しく
/// 扱う——`open-easy-web`の`build.rs`が同じ出典のアルゴリズムを
/// ビルド日時計算に使っている前例を踏襲、新規crate依存を追加しない)。
/// 不正な形式は`None`を返す(ガントチャート描画側はこの行を単純に
/// スキップする、正直な開示——秒単位の厳密な検証は行わない)。
fn days_from_civil(date: &str) -> Option<i64> {
    let parts: Vec<&str> = date.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let y: i64 = parts[0].parse().ok()?;
    let m: i64 = parts[1].parse().ok()?;
    let d: i64 = parts[2].parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as i64; // [0, 399]
    let mp = (m + 9) % 12; // [0, 11], Mar=0 ... Feb=11
    let doy = (153 * mp + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    Some(era * 146097 + doe - 719468)
}

/// `due_date`(`"YYYY-MM-DD"`)がブラウザの今日の日付より前(=期限超過)
/// かどうかを判定する(一覧の赤字表示用、2026-07-31追加)。
fn is_overdue(due_date: &str) -> bool {
    let today = js_sys::Date::new_0();
    let today_str = format!(
        "{:04}-{:02}-{:02}",
        today.get_full_year(),
        today.get_month() + 1,
        today.get_date()
    );
    match (days_from_civil(due_date), days_from_civil(&today_str)) {
        (Some(due), Some(today)) => due < today,
        _ => false,
    }
}

/// ガントチャート用の1チケット分の内部表現。
struct GanttEntry {
    title: String,
    start_days: i64,
    due_days: i64,
    done_ratio: u64,
}

/// 現在選択中プロジェクトのチケット一覧(`load_tickets`で取得済みの
/// レスポンスをそのまま受け取る、二重フェッチを避けるため)から、
/// ガントチャート(`start_date`/`due_date`が両方あるチケットのみ、
/// 日付範囲に応じた横棒グラフ)とカレンダー(`due_date`があるチケットを
/// 期限日順に一覧表示、日付が片方だけ・無いチケットも含む)を描画する
/// (Redmine機能ギャップ対応、2026-07-31新設)。
fn render_gantt_and_calendar(tickets: &[serde_json::Value]) {
    let mut gantt_entries = Vec::new();
    let mut calendar_entries: Vec<(String, String)> = Vec::new(); // (due_date, title)

    for t in tickets {
        let title = t.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let start_date = t.get("start_date").and_then(|v| v.as_str());
        let due_date = t.get("due_date").and_then(|v| v.as_str());
        let done_ratio = t.get("done_ratio").and_then(|v| v.as_u64()).unwrap_or(0);

        if let (Some(s), Some(due)) = (start_date, due_date) {
            if let (Some(start_days), Some(due_days)) = (days_from_civil(s), days_from_civil(due)) {
                if due_days >= start_days {
                    gantt_entries.push(GanttEntry { title: title.clone(), start_days, due_days, done_ratio });
                }
            }
        }
        if let Some(due) = due_date {
            calendar_entries.push((due.to_string(), title));
        }
    }

    // ガントチャート: 全チケットの日付範囲(最小start_days〜最大due_days)を
    // 100%として、各チケットのバーの位置・幅をパーセンテージで計算する。
    if gantt_entries.is_empty() {
        set_html("gantt-chart", "<p class=\"muted\">No tickets with both a start date and a due date (両方の日付を持つチケットがありません)</p>");
    } else {
        let range_start = gantt_entries.iter().map(|e| e.start_days).min().unwrap();
        let range_end = gantt_entries.iter().map(|e| e.due_days).max().unwrap();
        let range_span = (range_end - range_start).max(1) as f64;

        let mut html = String::new();
        for e in &gantt_entries {
            let left_pct = ((e.start_days - range_start) as f64 / range_span * 100.0).clamp(0.0, 100.0);
            let width_pct = (((e.due_days - e.start_days) as f64 / range_span * 100.0).max(1.0)).clamp(0.0, 100.0 - left_pct);
            let progress_pct = (e.done_ratio as f64).clamp(0.0, 100.0);
            html.push_str(&format!(
                r#"<div class="gantt-row"><span class="gantt-label" title="{title}">{title}</span><div class="gantt-track"><div class="gantt-bar" style="left:{left_pct:.2}%;width:{width_pct:.2}%;"><div class="gantt-bar-progress" style="width:{progress_pct:.0}%;"></div><span class="gantt-bar-label">{progress_pct:.0}%</span></div></div></div>"#,
                title = escape_html(&e.title)
            ));
        }
        set_html("gantt-chart", &html);
    }

    // カレンダー: 期限日の昇順でソートして一覧表示するだけの最小実装
    // (Redmine本家のような月表示グリッドは対象外、正直な開示)。
    calendar_entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut cal_html = String::new();
    for (due, title) in &calendar_entries {
        cal_html.push_str(&format!("<li><strong>{}</strong> — {}</li>", escape_html(due), escape_html(title)));
    }
    if cal_html.is_empty() {
        cal_html = "<li class=\"muted\">No tickets with a due date (期限日を持つチケットがありません)</li>".to_string();
    }
    set_html("calendar-list", &cal_html);
}

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook();
    wire_login();
    wire_project_form();
    wire_ticket_form();
    wire_ticket_detail();
    wire_wiki();
    refresh_auth_view();
    if session_token().is_some() {
        wasm_bindgen_futures::spawn_local(async { load_projects().await });
    }
}

fn console_error_panic_hook() {
    // 依存を増やさないための最小実装(`console_error_panic_hook`crateは
    // 使わず、標準のpanicフックだけconsole.errorへ橋渡しする)。
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&JsValue::from_str(&info.to_string()));
    }));
}

fn refresh_auth_view() {
    let logged_in = session_token().is_some();
    show("auth-logged-out", !logged_in);
    show("auth-logged-in", logged_in);
    show("app-main", logged_in);
    if logged_in {
        let email = local_storage().get_item(EMAIL_KEY).ok().flatten().unwrap_or_default();
        set_text("logged-in-email", &email);
    }
}

fn add_click(id: &str, f: impl FnMut() + 'static) {
    let el = by_id(id);
    let closure = Closure::wrap(Box::new(f) as Box<dyn FnMut()>);
    let _ = el.dyn_ref::<web_sys::HtmlElement>().unwrap().add_event_listener_with_callback("click", closure.as_ref().unchecked_ref());
    closure.forget();
}

fn wire_login() {
    add_click("request-otp-btn", || {
        wasm_bindgen_futures::spawn_local(async {
            let email = input_value("login-email");
            let body = serde_json::json!({ "email": email }).to_string();
            match api("POST", "/api/auth/request-otp", Some(body)).await {
                Ok((200, _)) => set_text("login-status", "Code sent, please enter it below (OTPを送信しました。メールのコードを入力してください)。"),
                Ok((503, _)) => set_text("login-status", "SMTP not configured, cannot send OTP (SMTP未設定のため、このサーバーではOTP送信できません)。"),
                Ok((status, msg)) => set_text("login-status", &format!("Error (エラー) ({status}): {msg}")),
                Err(_) => set_text("login-status", "Network error occurred (通信エラーが発生しました)。"),
            }
        });
    });

    add_click("verify-otp-btn", || {
        wasm_bindgen_futures::spawn_local(async {
            let email = input_value("login-email");
            let code = input_value("login-code");
            let body = serde_json::json!({ "email": email, "code": code }).to_string();
            match api("POST", "/api/auth/verify-otp", Some(body)).await {
                Ok((200, text)) => {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                        if let Some(token) = value.get("token").and_then(|v| v.as_str()) {
                            let storage = local_storage();
                            let _ = storage.set_item(SESSION_KEY, token);
                            let _ = storage.set_item(EMAIL_KEY, &email);
                            set_text("login-status", "");
                            refresh_auth_view();
                            wasm_bindgen_futures::spawn_local(async { load_projects().await });
                        }
                    }
                }
                Ok((status, msg)) => set_text("login-status", &format!("Login failed (ログイン失敗) ({status}): {msg}")),
                Err(_) => set_text("login-status", "Network error occurred (通信エラーが発生しました)。"),
            }
        });
    });

    add_click("logout-btn", || {
        wasm_bindgen_futures::spawn_local(async {
            let _ = api("POST", "/api/auth/logout", None).await;
            let storage = local_storage();
            let _ = storage.remove_item(SESSION_KEY);
            let _ = storage.remove_item(EMAIL_KEY);
            refresh_auth_view();
            set_html("project-list", "");
            set_html("ticket-list", "");
            show("ticket-detail", false);
        });
    });
}

fn wire_project_form() {
    add_click("create-project-btn", || {
        wasm_bindgen_futures::spawn_local(async {
            let name = input_value("new-project-name");
            if name.trim().is_empty() {
                return;
            }
            let body = serde_json::json!({ "name": name }).to_string();
            if let Ok((201, _)) = api("POST", "/api/projects", Some(body)).await {
                set_input_value("new-project-name", "");
                load_projects().await;
            }
        });
    });
}

fn wire_ticket_form() {
    add_click("refresh-gantt-btn", on_refresh_gantt);
    add_click("create-ticket-btn", || {
        wasm_bindgen_futures::spawn_local(async {
            let Some(project_id) = current_project_id() else {
                set_text("ticket-status", "Please select a project first (先にプロジェクトを選択してください)。");
                return;
            };
            let title = input_value("new-ticket-title");
            let description = textarea_value("new-ticket-description");
            let tracker = select_value("new-ticket-tracker");
            let assignee = input_value("new-ticket-assignee");
            let start_date = input_value("new-ticket-start-date");
            let due_date = input_value("new-ticket-due-date");
            let done_ratio = input_value("new-ticket-done-ratio");
            if title.trim().is_empty() {
                return;
            }
            let mut body_json = serde_json::json!({ "title": title, "description": description, "project_id": project_id, "tracker": tracker });
            // 担当者・開始日・期限日・進捗率はいずれも任意入力
            // (空欄なら送らない、サーバー側の`Option<...>`既定のまま)。
            if !assignee.trim().is_empty() {
                body_json["assignee"] = serde_json::Value::String(assignee);
            }
            if !start_date.trim().is_empty() {
                body_json["start_date"] = serde_json::Value::String(start_date);
            }
            if !due_date.trim().is_empty() {
                body_json["due_date"] = serde_json::Value::String(due_date);
            }
            if let Ok(ratio) = done_ratio.trim().parse::<u64>() {
                body_json["done_ratio"] = serde_json::Value::from(ratio);
            }
            let body = body_json.to_string();
            match api("POST", "/api/tickets", Some(body)).await {
                Ok((201, _)) => {
                    set_text("ticket-status", "");
                    set_input_value("new-ticket-title", "");
                    set_textarea_value("new-ticket-description", "");
                    set_input_value("new-ticket-assignee", "");
                    set_input_value("new-ticket-start-date", "");
                    set_input_value("new-ticket-due-date", "");
                    set_input_value("new-ticket-done-ratio", "");
                    load_tickets(project_id).await;
                }
                Ok((status, msg)) => set_text("ticket-status", &format!("Error (エラー) ({status}): {msg}")),
                Err(_) => set_text("ticket-status", "Network error occurred (通信エラーが発生しました)。"),
            }
        });
    });
}

/// チケット詳細パネル: ステータス変更・コメント投稿の配線。
fn wire_ticket_detail() {
    add_click("update-status-btn", || {
        wasm_bindgen_futures::spawn_local(async {
            let Some(ticket_id) = current_ticket_id() else {
                return;
            };
            let status = select_value("ticket-status-select");
            let body = serde_json::json!({ "status": status }).to_string();
            match api("PUT", &format!("/api/tickets/{ticket_id}"), Some(body)).await {
                Ok((200, _)) => open_ticket(ticket_id as u32),
                Ok((status_code, msg)) => set_text("ticket-detail-status", &format!("Update error (更新エラー) ({status_code}): {msg}")),
                Err(_) => set_text("ticket-detail-status", "Network error occurred (通信エラーが発生しました)。"),
            }
        });
    });

    add_click("update-assignee-btn", || {
        wasm_bindgen_futures::spawn_local(async {
            let Some(ticket_id) = current_ticket_id() else {
                return;
            };
            let assignee = input_value("new-assignee-input");
            if assignee.trim().is_empty() {
                return;
            }
            let body = serde_json::json!({ "assignee": assignee }).to_string();
            match api("PUT", &format!("/api/tickets/{ticket_id}"), Some(body)).await {
                Ok((200, _)) => {
                    set_input_value("new-assignee-input", "");
                    open_ticket(ticket_id as u32);
                }
                Ok((status_code, msg)) => set_text("ticket-detail-status", &format!("Assignee update error (担当者更新エラー) ({status_code}): {msg}")),
                Err(_) => set_text("ticket-detail-status", "Network error occurred (通信エラーが発生しました)。"),
            }
        });
    });

    add_click("update-schedule-btn", || {
        wasm_bindgen_futures::spawn_local(async {
            let Some(ticket_id) = current_ticket_id() else {
                return;
            };
            let start_date = input_value("edit-ticket-start-date");
            let due_date = input_value("edit-ticket-due-date");
            let done_ratio = input_value("edit-ticket-done-ratio");
            let mut body_json = serde_json::json!({});
            if !start_date.trim().is_empty() {
                body_json["start_date"] = serde_json::Value::String(start_date);
            }
            if !due_date.trim().is_empty() {
                body_json["due_date"] = serde_json::Value::String(due_date);
            }
            if let Ok(ratio) = done_ratio.trim().parse::<u64>() {
                body_json["done_ratio"] = serde_json::Value::from(ratio);
            }
            let body = body_json.to_string();
            match api("PUT", &format!("/api/tickets/{ticket_id}"), Some(body)).await {
                Ok((200, _)) => open_ticket(ticket_id as u32),
                Ok((status_code, msg)) => set_text("ticket-detail-status", &format!("Schedule update error (予定・進捗更新エラー) ({status_code}): {msg}")),
                Err(_) => set_text("ticket-detail-status", "Network error occurred (通信エラーが発生しました)。"),
            }
        });
    });

    add_click("post-comment-btn", || {
        wasm_bindgen_futures::spawn_local(async {
            let Some(ticket_id) = current_ticket_id() else {
                return;
            };
            let comment_body = textarea_value("new-comment-body");
            if comment_body.trim().is_empty() {
                return;
            }
            let body = serde_json::json!({ "body": comment_body }).to_string();
            match api("POST", &format!("/api/tickets/{ticket_id}/comments"), Some(body)).await {
                Ok((201, _)) => {
                    set_textarea_value("new-comment-body", "");
                    load_comments(ticket_id).await;
                }
                Ok((status_code, msg)) => set_text("ticket-detail-status", &format!("Post error (投稿エラー) ({status_code}): {msg}")),
                Err(_) => set_text("ticket-detail-status", "Network error occurred (通信エラーが発生しました)。"),
            }
        });
    });

    add_click("add-relation-btn", || {
        wasm_bindgen_futures::spawn_local(async {
            let Some(ticket_id) = current_ticket_id() else {
                return;
            };
            let target: u64 = match input_value("new-relation-target").trim().parse() {
                Ok(v) => v,
                Err(_) => {
                    set_text("relation-status", "Please enter a numeric target ticket ID (対象チケットIDを数値で入力してください)。");
                    return;
                }
            };
            let kind = select_value("new-relation-kind");
            let body = serde_json::json!({ "to_ticket_id": target, "kind": kind }).to_string();
            match api("POST", &format!("/api/tickets/{ticket_id}/relations"), Some(body)).await {
                Ok((201, _)) => {
                    set_text("relation-status", "");
                    set_input_value("new-relation-target", "");
                    load_relations(ticket_id).await;
                }
                Ok((status_code, msg)) => set_text("relation-status", &format!("Error (エラー) ({status_code}): {msg}")),
                Err(_) => set_text("relation-status", "Network error occurred (通信エラーが発生しました)。"),
            }
        });
    });

    add_click("add-time-entry-btn", || {
        wasm_bindgen_futures::spawn_local(async {
            let Some(ticket_id) = current_ticket_id() else {
                return;
            };
            let hours: f64 = match input_value("new-time-entry-hours").trim().parse() {
                Ok(v) => v,
                Err(_) => {
                    set_text("time-entry-status", "Please enter hours as a number, e.g. 1.5 (作業時間を数値で入力してください、例: 1.5)。");
                    return;
                }
            };
            let activity = input_value("new-time-entry-activity");
            let spent_on = input_value("new-time-entry-spent-on");
            let comments = textarea_value("new-time-entry-comments");
            if activity.trim().is_empty() {
                set_text("time-entry-status", "Please enter an activity (作業分類を入力してください)。");
                return;
            }
            let body = serde_json::json!({ "hours": hours, "activity": activity, "spent_on": spent_on, "comments": comments }).to_string();
            match api("POST", &format!("/api/tickets/{ticket_id}/time_entries"), Some(body)).await {
                Ok((201, _)) => {
                    set_text("time-entry-status", "");
                    set_input_value("new-time-entry-hours", "");
                    set_input_value("new-time-entry-activity", "");
                    set_input_value("new-time-entry-spent-on", "");
                    set_textarea_value("new-time-entry-comments", "");
                    load_time_entries(ticket_id).await;
                }
                Ok((status_code, msg)) => set_text("time-entry-status", &format!("Error (エラー) ({status_code}): {msg}")),
                Err(_) => set_text("time-entry-status", "Network error occurred (通信エラーが発生しました)。"),
            }
        });
    });

    add_click("add-attachment-btn", || {
        wasm_bindgen_futures::spawn_local(async {
            let Some(ticket_id) = current_ticket_id() else {
                return;
            };
            match upload_attachment(ticket_id).await {
                Ok(true) => {
                    set_text("attachment-status", "");
                    load_attachments(ticket_id).await;
                }
                Ok(false) => {}
                Err(_) => set_text("attachment-status", "Network error occurred (通信エラーが発生しました)。"),
            }
        });
    });

    add_click("close-ticket-detail-btn", || {
        show("ticket-detail", false);
        set_input_value("selected-ticket-id", "");
    });
}

fn wire_wiki() {
    add_click("load-wiki-btn", || {
        wasm_bindgen_futures::spawn_local(async {
            let Some(project_id) = current_project_id() else {
                set_text("wiki-status", "Please select a project first (先にプロジェクトを選択してください)。");
                return;
            };
            load_wiki_pages(project_id).await;
        });
    });

    add_click("create-wiki-btn", || {
        wasm_bindgen_futures::spawn_local(async {
            let Some(project_id) = current_project_id() else {
                return;
            };
            let slug = input_value("new-wiki-slug");
            let title = input_value("new-wiki-title");
            let body_text = textarea_value("new-wiki-body");
            if slug.trim().is_empty() || title.trim().is_empty() || body_text.trim().is_empty() {
                return;
            }
            let body = serde_json::json!({ "slug": slug, "title": title, "body": body_text }).to_string();
            match api("POST", &format!("/api/projects/{project_id}/wiki"), Some(body)).await {
                Ok((201, _)) => {
                    set_input_value("new-wiki-slug", "");
                    set_input_value("new-wiki-title", "");
                    set_textarea_value("new-wiki-body", "");
                    load_wiki_pages(project_id).await;
                }
                Ok((status_code, msg)) => set_text("wiki-status", &format!("Error (エラー) ({status_code}): {msg}")),
                Err(_) => set_text("wiki-status", "Network error occurred (通信エラーが発生しました)。"),
            }
        });
    });
}

async fn load_projects() {
    let Ok((200, text)) = api("GET", "/api/projects", None).await else {
        return;
    };
    let Ok(projects) = serde_json::from_str::<Vec<serde_json::Value>>(&text) else {
        return;
    };
    let mut html = String::new();
    for p in &projects {
        let id = p.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
        let name = escape_html(p.get("name").and_then(|v| v.as_str()).unwrap_or(""));
        let has_parent = p.get("parent_id").map(|v| !v.is_null()).unwrap_or(false);
        let indent = if has_parent { " style=\"margin-left:1.2rem\"" } else { "" };
        html.push_str(&format!(
            r#"<li{indent}><button class="link-btn" onclick="select_project({id})">{name}</button></li>"#
        ));
    }
    if html.is_empty() {
        html = "<li class=\"muted\">プロジェクトはまだありません</li>".to_string();
    }
    set_html("project-list", &html);
}

/// **2026-07-27追記(実クリックE2Eで発見した実バグの修正)**: 引数は
/// `u32`で受ける。理由——`wasm-bindgen`は`u64`をJS側`BigInt`へ写像するが、
/// `web/index.html`側の`onclick="select_project({id})"`は通常のJS数値
/// リテラル(例: `select_project(0)`、BigIntリテラルの`0n`ではない)を
/// 埋め込んでいたため、実ブラウザでボタンをクリックすると
/// `TypeError: Cannot convert 0 to a BigInt`で握りつぶされ、
/// プロジェクト選択自体が一切機能していなかった(`cargo test`はJS↔WASMの
/// 呼び出し境界を経由しないため、この種の不具合は検出できない——実際に
/// ブラウザで実クリックして初めて発覚した)。`u32`ならJS側は通常の
/// `Number`として渡せるため、この変換エラーが起きない。
#[wasm_bindgen]
pub fn select_project(project_id: u32) {
    let project_id = project_id as u64;
    set_input_value("selected-project-id", &project_id.to_string());
    set_text("selected-project-label", &format!("Selected project ID (選択中のプロジェクトID): {project_id}"));
    show("ticket-detail", false);
    set_html("wiki-list", "");
    wasm_bindgen_futures::spawn_local(async move { load_tickets(project_id).await });
}

async fn load_tickets(project_id: u64) {
    let Ok((200, text)) = api("GET", "/api/tickets", None).await else {
        set_html("ticket-list", "<li>Failed to load (読み込みに失敗しました)</li>");
        return;
    };
    let Ok(tickets) = serde_json::from_str::<Vec<serde_json::Value>>(&text) else {
        return;
    };
    let project_tickets: Vec<serde_json::Value> = tickets
        .into_iter()
        .filter(|t| t.get("project_id").and_then(|v| v.as_u64()).unwrap_or(u64::MAX) == project_id)
        .collect();
    let mut html = String::new();
    for t in &project_tickets {
        let id = t.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
        let title = escape_html(t.get("title").and_then(|v| v.as_str()).unwrap_or(""));
        let status = escape_html(t.get("status").and_then(|v| v.as_str()).unwrap_or(""));
        let tracker = escape_html(t.get("tracker").and_then(|v| v.as_str()).unwrap_or("bug"));
        let assignee = t.get("assignee").and_then(|v| v.as_str()).unwrap_or("-");
        let done_ratio = t.get("done_ratio").and_then(|v| v.as_u64()).unwrap_or(0);
        let due_date = t.get("due_date").and_then(|v| v.as_str()).unwrap_or("-");
        // 期限日が今日以前(既に期限切れ)の場合は赤字で強調する
        // (Redmine本家の「期限超過チケットの赤字表示」相当の視覚パターン)。
        let due_class = if due_date != "-" && is_overdue(due_date) { " class=\"due-soon\"" } else { "" };
        html.push_str(&format!(
            r#"<tr>
                <td class="ticket-id">#{id}</td>
                <td><span class="tracker-tag tracker-{tracker}">{tracker}</span></td>
                <td><span class="status-pill status-{status}">{status}</span></td>
                <td><button class="link-btn" onclick="open_ticket({id})">{title}</button></td>
                <td>{assignee_html}</td>
                <td><div class="done-ratio-cell"><div class="done-ratio-track"><div class="done-ratio-fill" style="width:{done_ratio}%;"></div></div><span>{done_ratio}%</span></div></td>
                <td{due_class}>{due_date_html}</td>
            </tr>"#,
            assignee_html = escape_html(assignee),
            due_date_html = escape_html(due_date),
        ));
    }
    if html.is_empty() {
        html = "<tr><td colspan=\"7\" class=\"muted\">No tickets yet (チケットはまだありません)</td></tr>".to_string();
    }
    set_html("ticket-list", &html);
    render_gantt_and_calendar(&project_tickets);
}

/// 「Refresh」ボタン用: 現在選択中のプロジェクトのチケット一覧を
/// 再取得し、一覧・ガントチャート・カレンダーを最新化する。
fn on_refresh_gantt() {
    let Some(project_id) = current_project_id() else {
        return;
    };
    wasm_bindgen_futures::spawn_local(async move { load_tickets(project_id).await });
}

/// チケット詳細を開く(詳細取得+コメント一覧取得)。JSの`onclick`から
/// `#[wasm_bindgen]`経由で直接呼べるようにグローバル公開する。
/// `u32`で受ける理由は`select_project`と同じ(2026-07-27追記、
/// `TypeError: Cannot convert 0 to a BigInt`の回避)。
#[wasm_bindgen]
pub fn open_ticket(ticket_id: u32) {
    let ticket_id = ticket_id as u64;
    wasm_bindgen_futures::spawn_local(async move {
        set_input_value("selected-ticket-id", &ticket_id.to_string());
        let Ok((200, text)) = api("GET", &format!("/api/tickets/{ticket_id}"), None).await else {
            return;
        };
        let Ok(t) = serde_json::from_str::<serde_json::Value>(&text) else {
            return;
        };
        let title = t.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let description = t.get("description").and_then(|v| v.as_str()).unwrap_or("");
        let status = t.get("status").and_then(|v| v.as_str()).unwrap_or("open");
        let tracker = t.get("tracker").and_then(|v| v.as_str()).unwrap_or("bug");
        let assignee = t.get("assignee").and_then(|v| v.as_str());
        let start_date = t.get("start_date").and_then(|v| v.as_str()).unwrap_or("");
        let due_date = t.get("due_date").and_then(|v| v.as_str()).unwrap_or("");
        let done_ratio = t.get("done_ratio").and_then(|v| v.as_u64()).unwrap_or(0);
        set_text("ticket-detail-title", title);
        set_text("ticket-detail-tracker", tracker);
        set_text("ticket-detail-assignee", assignee.unwrap_or("unassigned (未割当)"));
        set_text("ticket-detail-description", description);
        if let Ok(select) = by_id("ticket-status-select").dyn_into::<HtmlSelectElement>() {
            select.set_value(status);
        }
        set_input_value("edit-ticket-start-date", start_date);
        set_input_value("edit-ticket-due-date", due_date);
        set_input_value("edit-ticket-done-ratio", &done_ratio.to_string());
        set_text("ticket-detail-schedule", &format!(
            "Start (開始): {} / Due (期限): {} / Progress (進捗): {}%",
            if start_date.is_empty() { "-" } else { start_date },
            if due_date.is_empty() { "-" } else { due_date },
            done_ratio
        ));
        set_text("ticket-detail-status", "");
        show("ticket-detail", true);
        load_comments(ticket_id).await;
        load_relations(ticket_id).await;
        load_time_entries(ticket_id).await;
        load_attachments(ticket_id).await;
    });
}

async fn load_relations(ticket_id: u64) {
    let Ok((200, text)) = api("GET", &format!("/api/tickets/{ticket_id}/relations"), None).await else {
        return;
    };
    let Ok(relations) = serde_json::from_str::<Vec<serde_json::Value>>(&text) else {
        return;
    };
    let mut html = String::new();
    for r in &relations {
        let id = r.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
        let from = r.get("from_ticket_id").and_then(|v| v.as_u64()).unwrap_or(0);
        let to = r.get("to_ticket_id").and_then(|v| v.as_u64()).unwrap_or(0);
        let kind = escape_html(r.get("kind").and_then(|v| v.as_str()).unwrap_or(""));
        let other = if from == ticket_id { to } else { from };
        let direction = if from == ticket_id { "→" } else { "←" };
        html.push_str(&format!(
            r#"<li><span class="badge">{kind}</span> {direction} <button class="link-btn" onclick="open_ticket({other})">#{other}</button> <button onclick="delete_relation({id})">Delete (削除)</button></li>"#
        ));
    }
    if html.is_empty() {
        html = "<li class=\"muted\">関連チケットはまだありません</li>".to_string();
    }
    set_html("relation-list", &html);
}

/// `web/index.html`の`onclick="delete_relation(...)"`から直接呼べるよう
/// グローバル公開する(`open_ticket`/`open_wiki_page`と同じパターン)。
/// `u32`で受ける理由は`select_project`と同じ(2026-07-27追記、
/// `TypeError: Cannot convert 0 to a BigInt`の回避)。
#[wasm_bindgen]
pub fn delete_relation(relation_id: u32) {
    let relation_id = relation_id as u64;
    wasm_bindgen_futures::spawn_local(async move {
        match api("DELETE", &format!("/api/relations/{relation_id}"), None).await {
            Ok((200, _)) => {
                if let Some(ticket_id) = current_ticket_id() {
                    load_relations(ticket_id).await;
                }
            }
            Ok((status_code, msg)) => set_text("relation-status", &format!("Delete error (削除エラー) ({status_code}): {msg}")),
            Err(_) => set_text("relation-status", "Network error occurred (通信エラーが発生しました)。"),
        }
    });
}

async fn load_time_entries(ticket_id: u64) {
    let Ok((200, text)) = api("GET", &format!("/api/tickets/{ticket_id}/time_entries"), None).await else {
        return;
    };
    let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(&text) else {
        return;
    };
    let my_email = local_storage().get_item(EMAIL_KEY).ok().flatten().unwrap_or_default();
    let mut total = 0.0f64;
    let mut html = String::new();
    for e in &entries {
        let id = e.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
        let hours = e.get("hours").and_then(|v| v.as_f64()).unwrap_or(0.0);
        total += hours;
        let activity = escape_html(e.get("activity").and_then(|v| v.as_str()).unwrap_or(""));
        let author = escape_html(e.get("author_email").and_then(|v| v.as_str()).unwrap_or(""));
        let comments = escape_html(e.get("comments").and_then(|v| v.as_str()).unwrap_or(""));
        let spent_on = escape_html(e.get("spent_on").and_then(|v| v.as_str()).unwrap_or(""));
        // 削除ボタンは投稿者本人にのみ表示(サーバー側も管理者/投稿者のみ許可
        // する権限モデルのため、これは表示上の補助にすぎず実際の許可判定は
        // 引き続きサーバー側で行われる)。
        let can_delete = !my_email.is_empty() && author == my_email;
        let delete_btn = if can_delete {
            format!(r#" <button onclick="delete_time_entry({id})">Delete (削除)</button>"#)
        } else {
            String::new()
        };
        html.push_str(&format!(
            r#"<li><strong>{hours}h</strong> [{activity}] {spent_on} — {comments} <span class="muted">by {author}</span>{delete_btn}</li>"#
        ));
    }
    if html.is_empty() {
        html = "<li class=\"muted\">作業時間記録はまだありません</li>".to_string();
    }
    set_html("time-entry-list", &html);
    set_text("time-entry-total", &format!("Total (合計): {total}h"));
}

/// `web/index.html`の`onclick="delete_time_entry(...)"`から直接呼べるよう
/// グローバル公開する。投稿者本人以外・非管理者が呼んだ場合はサーバー側が
/// `403`を返し、そのままエラー表示する(表示上の抑制は`load_time_entries`
/// 側で行うが、直接呼ばれた場合の最終防衛はサーバー側の権限チェック)。
/// `u32`で受ける理由は`select_project`と同じ(2026-07-27追記、
/// `TypeError: Cannot convert 0 to a BigInt`の回避)。
#[wasm_bindgen]
pub fn delete_time_entry(entry_id: u32) {
    let entry_id = entry_id as u64;
    wasm_bindgen_futures::spawn_local(async move {
        match api("DELETE", &format!("/api/time_entries/{entry_id}"), None).await {
            Ok((200, _)) => {
                if let Some(ticket_id) = current_ticket_id() {
                    load_time_entries(ticket_id).await;
                }
            }
            Ok((status_code, msg)) => set_text("time-entry-status", &format!("Delete error (削除エラー) ({status_code}): {msg}")),
            Err(_) => set_text("time-entry-status", "Network error occurred (通信エラーが発生しました)。"),
        }
    });
}

/// 添付ファイル一覧を取得し描画する。削除ボタンの表示可否は
/// `load_time_entries`と同じ「投稿者本人のみ表示」パターンを踏襲する
/// (表示上の補助にすぎず、最終防衛は`DELETE /api/attachments/:id`の
/// サーバー側権限チェック)。
async fn load_attachments(ticket_id: u64) {
    let Ok((200, text)) = api("GET", &format!("/api/tickets/{ticket_id}/attachments"), None).await else {
        return;
    };
    let Ok(attachments) = serde_json::from_str::<Vec<serde_json::Value>>(&text) else {
        return;
    };
    let my_email = local_storage().get_item(EMAIL_KEY).ok().flatten().unwrap_or_default();
    let mut html = String::new();
    for a in &attachments {
        let id = a.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
        let file_name = escape_html(a.get("file_name").and_then(|v| v.as_str()).unwrap_or(""));
        let size_bytes = a.get("size_bytes").and_then(|v| v.as_u64()).unwrap_or(0);
        let author = escape_html(a.get("author_email").and_then(|v| v.as_str()).unwrap_or(""));
        let can_delete = !my_email.is_empty() && author == my_email;
        let delete_btn = if can_delete {
            format!(r#" <button onclick="delete_attachment({id})">Delete (削除)</button>"#)
        } else {
            String::new()
        };
        html.push_str(&format!(
            r#"<li><button class="link-btn" onclick="download_attachment({id})">{file_name}</button> <span class="muted">({size_bytes} bytes, by {author})</span>{delete_btn}</li>"#
        ));
    }
    if html.is_empty() {
        html = "<li class=\"muted\">添付ファイルはまだありません</li>".to_string();
    }
    set_html("attachment-list", &html);
}

/// `new-attachment-file`(`<input type="file">`)の選択内容を
/// `multipart/form-data`で`POST /api/tickets/:id/attachments`へ送信する。
/// `api()`(JSON専用)は使わず、`FormData`+生の`fetch()`を直接組み立てる
/// (`Content-Type`はブラウザが`boundary`付きで自動設定するため、
/// `api()`のようにJSON用ヘッダを明示的に設定してはいけない)。
async fn upload_attachment(ticket_id: u64) -> Result<bool, JsValue> {
    let input = by_id("new-attachment-file").dyn_into::<HtmlInputElement>()?;
    let Some(files) = input.files() else {
        set_text("attachment-status", "Please choose a file (ファイルを選択してください)。");
        return Ok(false);
    };
    if files.length() == 0 {
        set_text("attachment-status", "Please choose a file (ファイルを選択してください)。");
        return Ok(false);
    }
    let Some(file) = files.get(0) else {
        return Ok(false);
    };
    let form_data = web_sys::FormData::new()?;
    form_data.append_with_blob("file", &file)?;

    let mut opts = RequestInit::new();
    opts.set_method("POST");
    opts.set_mode(RequestMode::SameOrigin);
    opts.set_body(&form_data);
    let url = format!("{BASE_PATH}/api/tickets/{ticket_id}/attachments");
    let request = Request::new_with_str_and_init(&url, &opts)?;
    if let Some(token) = session_token() {
        request.headers().set("Authorization", &format!("Bearer {token}"))?;
    }
    let resp_value = JsFuture::from(window().fetch_with_request(&request)).await?;
    let resp: Response = resp_value.dyn_into()?;
    let status = resp.status();
    if status == 201 {
        input.set_value("");
        Ok(true)
    } else {
        let text = JsFuture::from(resp.text()?).await?.as_string().unwrap_or_default();
        set_text("attachment-status", &format!("Error (エラー) ({status}): {text}"));
        Ok(false)
    }
}

/// `web/index.html`の`onclick="download_attachment(...)"`から直接呼べる
/// ようグローバル公開する。`fetch()`でBlobとして取得し、`Object URL`+
/// 一時`<a download>`要素の合成クリックでブラウザのファイル保存
/// ダイアログを起動する(セッションは`localStorage`のBearerトークンで
/// 認証するため、通常の`<a href="...">`直リンクではAuthorizationヘッダを
/// 送れず認証済みダウンロードができないことへの対応)。
/// `u32`で受ける理由は`select_project`と同じ(2026-07-27追記、
/// `TypeError: Cannot convert 0 to a BigInt`の回避)。
#[wasm_bindgen]
pub fn download_attachment(attachment_id: u32) {
    let attachment_id = attachment_id as u64;
    wasm_bindgen_futures::spawn_local(async move {
        if download_attachment_inner(attachment_id).await.is_err() {
            set_text("attachment-status", "Download failed (ダウンロードに失敗しました)。");
        }
    });
}

async fn download_attachment_inner(attachment_id: u64) -> Result<(), JsValue> {
    let mut opts = RequestInit::new();
    opts.set_method("GET");
    opts.set_mode(RequestMode::SameOrigin);
    let url = format!("{BASE_PATH}/api/attachments/{attachment_id}/download");
    let request = Request::new_with_str_and_init(&url, &opts)?;
    if let Some(token) = session_token() {
        request.headers().set("Authorization", &format!("Bearer {token}"))?;
    }
    let resp_value = JsFuture::from(window().fetch_with_request(&request)).await?;
    let resp: Response = resp_value.dyn_into()?;
    if resp.status() != 200 {
        return Err(JsValue::from_str("download failed"));
    }
    let disposition = resp.headers().get("content-disposition").ok().flatten().unwrap_or_default();
    let filename = disposition
        .split("filename=\"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .unwrap_or("download")
        .to_string();

    let blob_value = JsFuture::from(resp.blob()?).await?;
    let blob: web_sys::Blob = blob_value.dyn_into()?;
    let object_url = web_sys::Url::create_object_url_with_blob(&blob)?;
    let anchor = document().create_element("a")?;
    anchor.set_attribute("href", &object_url)?;
    anchor.set_attribute("download", &filename)?;
    if let Ok(html_el) = anchor.dyn_into::<web_sys::HtmlElement>() {
        html_el.click();
    }
    web_sys::Url::revoke_object_url(&object_url)?;
    Ok(())
}

/// `web/index.html`の`onclick="delete_attachment(...)"`から直接呼べるよう
/// グローバル公開する。**正直な開示**: `DELETE /api/attachments/:id`は
/// メタデータのみ削除する(`StorageBackend`に削除APIがまだ無いため、
/// 保存先の実ファイルは残り続ける——`src/attachments.rs`の既知の
/// 制約、`main.rs::delete_attachment`のコメントに明記済み)。
/// `u32`で受ける理由は`select_project`と同じ(2026-07-27追記、
/// `TypeError: Cannot convert 0 to a BigInt`の回避)。
#[wasm_bindgen]
pub fn delete_attachment(attachment_id: u32) {
    let attachment_id = attachment_id as u64;
    wasm_bindgen_futures::spawn_local(async move {
        match api("DELETE", &format!("/api/attachments/{attachment_id}"), None).await {
            Ok((200, _)) => {
                if let Some(ticket_id) = current_ticket_id() {
                    load_attachments(ticket_id).await;
                }
            }
            Ok((status_code, msg)) => set_text("attachment-status", &format!("Delete error (削除エラー) ({status_code}): {msg}")),
            Err(_) => set_text("attachment-status", "Network error occurred (通信エラーが発生しました)。"),
        }
    });
}

async fn load_comments(ticket_id: u64) {
    let Ok((200, text)) = api("GET", &format!("/api/tickets/{ticket_id}/comments"), None).await else {
        return;
    };
    let Ok(comments) = serde_json::from_str::<Vec<serde_json::Value>>(&text) else {
        return;
    };
    let mut html = String::new();
    for c in &comments {
        let author = escape_html(c.get("author_email").and_then(|v| v.as_str()).unwrap_or(""));
        let body = escape_html(c.get("body").and_then(|v| v.as_str()).unwrap_or(""));
        html.push_str(&format!(r#"<li><strong>{author}</strong>: {body}</li>"#));
    }
    if html.is_empty() {
        html = "<li class=\"muted\">コメントはまだありません</li>".to_string();
    }
    set_html("comment-list", &html);
}

async fn load_wiki_pages(project_id: u64) {
    let Ok((200, text)) = api("GET", &format!("/api/projects/{project_id}/wiki"), None).await else {
        set_text("wiki-status", "Failed to load wiki list (Wiki一覧の読み込みに失敗しました)。");
        return;
    };
    let Ok(pages) = serde_json::from_str::<Vec<serde_json::Value>>(&text) else {
        return;
    };
    let mut html = String::new();
    for p in &pages {
        let id = p.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
        let title = escape_html(p.get("title").and_then(|v| v.as_str()).unwrap_or(""));
        html.push_str(&format!(r#"<li><button class="link-btn" onclick="open_wiki_page({id})">{title}</button></li>"#));
    }
    if html.is_empty() {
        html = "<li class=\"muted\">Wikiページはまだありません</li>".to_string();
    }
    set_html("wiki-list", &html);
    set_text("wiki-status", "");
}

/// `u32`で受ける理由は`select_project`と同じ(2026-07-27追記、
/// `TypeError: Cannot convert 0 to a BigInt`の回避)。
#[wasm_bindgen]
pub fn open_wiki_page(page_id: u32) {
    let page_id = page_id as u64;
    wasm_bindgen_futures::spawn_local(async move {
        let Ok((200, text)) = api("GET", &format!("/api/wiki/{page_id}"), None).await else {
            return;
        };
        let Ok(page) = serde_json::from_str::<serde_json::Value>(&text) else {
            return;
        };
        let title = page.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let revisions = page.get("revisions").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let latest_body = revisions.last().and_then(|r| r.get("body")).and_then(|v| v.as_str()).unwrap_or("");
        set_text("wiki-view-title", title);
        set_text("wiki-view-body", latest_body);
        set_text("wiki-view-revision-count", &format!("Revisions (改訂履歴): {}", revisions.len()));
        show("wiki-view", true);
    });
}

#[cfg(test)]
mod gantt_tests {
    use super::days_from_civil;

    #[test]
    fn epoch_is_day_zero() {
        assert_eq!(days_from_civil("1970-01-01"), Some(0));
    }

    #[test]
    fn known_reference_dates_match_expected_day_counts() {
        // 2000-03-01は1970-01-01から11017日後(第三者の日数計算表と照合済み
        // の既知の参照値、うるう年〈2000年〉を跨ぐケース)。
        assert_eq!(days_from_civil("2000-03-01"), Some(11017));
        // 2026-07-31は1970-01-01から20665日後(実測値、上のHinnant実装の
        // 出力そのものを正とする——この関数自体は既存コードで変更していない)。
        assert_eq!(days_from_civil("2026-07-31"), Some(20665));
    }

    #[test]
    fn later_date_yields_a_larger_day_count() {
        let a = days_from_civil("2026-01-01").unwrap();
        let b = days_from_civil("2026-12-31").unwrap();
        assert!(b > a);
        assert_eq!(b - a, 364); // 2026年は平年(うるう年ではない)。
    }

    #[test]
    fn malformed_dates_return_none() {
        assert_eq!(days_from_civil(""), None);
        assert_eq!(days_from_civil("not-a-date"), None);
        assert_eq!(days_from_civil("2026-13-01"), None);
        assert_eq!(days_from_civil("2026-01-32"), None);
    }
}
