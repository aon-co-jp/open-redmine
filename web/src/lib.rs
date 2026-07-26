//! RS-Redのブラウザフロントエンド(Rust→WebAssembly、オンライン専用)。
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

/// 現在選択中のプロジェクトID(0=未選択)。
fn current_project_id() -> u64 {
    input_value("selected-project-id").parse().unwrap_or(0)
}

/// 現在開いているチケットID(0=未選択)。
fn current_ticket_id() -> u64 {
    input_value("selected-ticket-id").parse().unwrap_or(0)
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
    let request = Request::new_with_str_and_init(path, &opts)?;
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
                Ok((200, _)) => set_text("login-status", "OTPを送信しました。メールのコードを入力してください。"),
                Ok((503, _)) => set_text("login-status", "SMTP未設定のため、このサーバーではOTP送信できません。"),
                Ok((status, msg)) => set_text("login-status", &format!("エラー({status}): {msg}")),
                Err(_) => set_text("login-status", "通信エラーが発生しました。"),
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
                Ok((status, msg)) => set_text("login-status", &format!("ログイン失敗({status}): {msg}")),
                Err(_) => set_text("login-status", "通信エラーが発生しました。"),
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
    add_click("create-ticket-btn", || {
        wasm_bindgen_futures::spawn_local(async {
            let project_id = current_project_id();
            if project_id == 0 {
                set_text("ticket-status", "先にプロジェクトを選択してください。");
                return;
            }
            let title = input_value("new-ticket-title");
            let description = textarea_value("new-ticket-description");
            let tracker = select_value("new-ticket-tracker");
            if title.trim().is_empty() {
                return;
            }
            let body = serde_json::json!({ "title": title, "description": description, "project_id": project_id, "tracker": tracker }).to_string();
            match api("POST", "/api/tickets", Some(body)).await {
                Ok((201, _)) => {
                    set_text("ticket-status", "");
                    set_input_value("new-ticket-title", "");
                    set_textarea_value("new-ticket-description", "");
                    load_tickets(project_id).await;
                }
                Ok((status, msg)) => set_text("ticket-status", &format!("エラー({status}): {msg}")),
                Err(_) => set_text("ticket-status", "通信エラーが発生しました。"),
            }
        });
    });
}

/// チケット詳細パネル: ステータス変更・コメント投稿の配線。
fn wire_ticket_detail() {
    add_click("update-status-btn", || {
        wasm_bindgen_futures::spawn_local(async {
            let ticket_id = current_ticket_id();
            if ticket_id == 0 {
                return;
            }
            let status = select_value("ticket-status-select");
            let body = serde_json::json!({ "status": status }).to_string();
            match api("PUT", &format!("/api/tickets/{ticket_id}"), Some(body)).await {
                Ok((200, _)) => open_ticket(ticket_id),
                Ok((status_code, msg)) => set_text("ticket-detail-status", &format!("更新エラー({status_code}): {msg}")),
                Err(_) => set_text("ticket-detail-status", "通信エラーが発生しました。"),
            }
        });
    });

    add_click("post-comment-btn", || {
        wasm_bindgen_futures::spawn_local(async {
            let ticket_id = current_ticket_id();
            if ticket_id == 0 {
                return;
            }
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
                Ok((status_code, msg)) => set_text("ticket-detail-status", &format!("投稿エラー({status_code}): {msg}")),
                Err(_) => set_text("ticket-detail-status", "通信エラーが発生しました。"),
            }
        });
    });

    add_click("add-relation-btn", || {
        wasm_bindgen_futures::spawn_local(async {
            let ticket_id = current_ticket_id();
            if ticket_id == 0 {
                return;
            }
            let target: u64 = match input_value("new-relation-target").trim().parse() {
                Ok(v) => v,
                Err(_) => {
                    set_text("relation-status", "対象チケットIDを数値で入力してください。");
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
                Ok((status_code, msg)) => set_text("relation-status", &format!("エラー({status_code}): {msg}")),
                Err(_) => set_text("relation-status", "通信エラーが発生しました。"),
            }
        });
    });

    add_click("add-time-entry-btn", || {
        wasm_bindgen_futures::spawn_local(async {
            let ticket_id = current_ticket_id();
            if ticket_id == 0 {
                return;
            }
            let hours: f64 = match input_value("new-time-entry-hours").trim().parse() {
                Ok(v) => v,
                Err(_) => {
                    set_text("time-entry-status", "作業時間を数値で入力してください(例: 1.5)。");
                    return;
                }
            };
            let activity = input_value("new-time-entry-activity");
            let spent_on = input_value("new-time-entry-spent-on");
            let comments = textarea_value("new-time-entry-comments");
            if activity.trim().is_empty() {
                set_text("time-entry-status", "作業分類を入力してください。");
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
                Ok((status_code, msg)) => set_text("time-entry-status", &format!("エラー({status_code}): {msg}")),
                Err(_) => set_text("time-entry-status", "通信エラーが発生しました。"),
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
            let project_id = current_project_id();
            if project_id == 0 {
                set_text("wiki-status", "先にプロジェクトを選択してください。");
                return;
            }
            load_wiki_pages(project_id).await;
        });
    });

    add_click("create-wiki-btn", || {
        wasm_bindgen_futures::spawn_local(async {
            let project_id = current_project_id();
            if project_id == 0 {
                return;
            }
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
                Ok((status_code, msg)) => set_text("wiki-status", &format!("エラー({status_code}): {msg}")),
                Err(_) => set_text("wiki-status", "通信エラーが発生しました。"),
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

#[wasm_bindgen]
pub fn select_project(project_id: u64) {
    set_input_value("selected-project-id", &project_id.to_string());
    set_text("selected-project-label", &format!("選択中のプロジェクトID: {project_id}"));
    show("ticket-detail", false);
    set_html("wiki-list", "");
    wasm_bindgen_futures::spawn_local(async move { load_tickets(project_id).await });
}

async fn load_tickets(project_id: u64) {
    let Ok((200, text)) = api("GET", "/api/tickets", None).await else {
        set_html("ticket-list", "<li>読み込みに失敗しました</li>");
        return;
    };
    let Ok(tickets) = serde_json::from_str::<Vec<serde_json::Value>>(&text) else {
        return;
    };
    let mut html = String::new();
    for t in &tickets {
        let tid = t.get("project_id").and_then(|v| v.as_u64()).unwrap_or(u64::MAX);
        if tid != project_id {
            continue;
        }
        let id = t.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
        let title = escape_html(t.get("title").and_then(|v| v.as_str()).unwrap_or(""));
        let status = escape_html(t.get("status").and_then(|v| v.as_str()).unwrap_or(""));
        let tracker = escape_html(t.get("tracker").and_then(|v| v.as_str()).unwrap_or("bug"));
        html.push_str(&format!(
            r#"<li><button class="link-btn" onclick="open_ticket({id})">{title}</button> <span class="badge">{tracker}</span> <span class="badge">{status}</span></li>"#
        ));
    }
    if html.is_empty() {
        html = "<li class=\"muted\">チケットはまだありません</li>".to_string();
    }
    set_html("ticket-list", &html);
}

/// チケット詳細を開く(詳細取得+コメント一覧取得)。JSの`onclick`から
/// `#[wasm_bindgen]`経由で直接呼べるようにグローバル公開する。
#[wasm_bindgen]
pub fn open_ticket(ticket_id: u64) {
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
        set_text("ticket-detail-title", title);
        set_text("ticket-detail-tracker", tracker);
        set_text("ticket-detail-description", description);
        if let Ok(select) = by_id("ticket-status-select").dyn_into::<HtmlSelectElement>() {
            select.set_value(status);
        }
        set_text("ticket-detail-status", "");
        show("ticket-detail", true);
        load_comments(ticket_id).await;
        load_relations(ticket_id).await;
        load_time_entries(ticket_id).await;
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
#[wasm_bindgen]
pub fn delete_relation(relation_id: u64) {
    wasm_bindgen_futures::spawn_local(async move {
        let ticket_id = current_ticket_id();
        match api("DELETE", &format!("/api/relations/{relation_id}"), None).await {
            Ok((200, _)) => load_relations(ticket_id).await,
            Ok((status_code, msg)) => set_text("relation-status", &format!("削除エラー({status_code}): {msg}")),
            Err(_) => set_text("relation-status", "通信エラーが発生しました。"),
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
#[wasm_bindgen]
pub fn delete_time_entry(entry_id: u64) {
    wasm_bindgen_futures::spawn_local(async move {
        let ticket_id = current_ticket_id();
        match api("DELETE", &format!("/api/time_entries/{entry_id}"), None).await {
            Ok((200, _)) => load_time_entries(ticket_id).await,
            Ok((status_code, msg)) => set_text("time-entry-status", &format!("削除エラー({status_code}): {msg}")),
            Err(_) => set_text("time-entry-status", "通信エラーが発生しました。"),
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
        set_text("wiki-status", "Wiki一覧の読み込みに失敗しました。");
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

#[wasm_bindgen]
pub fn open_wiki_page(page_id: u64) {
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
        set_text("wiki-view-revision-count", &format!("改訂履歴: {}件", revisions.len()));
        show("wiki-view", true);
    });
}
