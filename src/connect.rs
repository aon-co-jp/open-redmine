//! Connect(単純呼び出し)層 — REST全廃方針(2026-11-04)の公開API面。
//!
//! 公開する経路は `POST /open_redmine.v1.<Service>/<Method>`
//! (`content-type: application/json`、本文はJSONオブジェクト)のみ。
//! 認証は従来どおり `Authorization: Bearer <token>`。成功は常にHTTP 200
//! (本文JSON)、失敗は `{"code","message"}` とHTTPステータス。
//!
//! 実装方針: 認可・検証・永続化を持つ既存ハンドラは**内部ルーター**
//! (どのポートにも公開しない)に残し、この層が `<Service>/<Method>` を
//! 内部ルートへ写して呼ぶ。内部のURL形はもう外へ出ない
//! (旧 `/api/...` を直接叩くと404)。リクエストJSONの `id` は
//! 「その呼び出しが対象とする親/本体リソースのid」(例: Comments/List は
//! チケットid、Comments/Delete はコメントid)で、残りのフィールドが本文
//! (一覧系はフィルタ条件)になる。

use std::sync::Arc;

use poem::http::{header, Method, StatusCode};
use poem::{Endpoint, EndpointExt, IntoResponse, Request, Response};
use serde_json::{Map, Value};

pub type Shared = Arc<poem::endpoint::BoxEndpoint<'static, Response>>;

pub const PACKAGE: &str = "open_redmine.v1";
/// 一般の呼び出しの本文上限。
const MAX_BODY: usize = 256 * 1024;
/// 添付アップロード(base64込み)の本文上限。
const MAX_UPLOAD_BODY: usize = 16 * 1024 * 1024;
const MAX_DEPTH: usize = 32;

struct Rpc {
    service: &'static str,
    method: &'static str,
    http: &'static str,
    /// 内部ルート。`{id}` は本文の `id` で置換する。
    path: &'static str,
    /// 残りのフィールドをクエリ文字列として渡す(GETの絞り込み)。
    query: bool,
}

const fn rpc(service: &'static str, method: &'static str, http: &'static str, path: &'static str, query: bool) -> Rpc {
    Rpc { service, method, http, path, query }
}

const TABLE: &[Rpc] = &[
    rpc("Auth", "Config", "GET", "/api/auth/config", false),
    rpc("Auth", "RequestOtp", "POST", "/api/auth/request-otp", false),
    rpc("Auth", "VerifyOtp", "POST", "/api/auth/verify-otp", false),
    rpc("Auth", "Logout", "POST", "/api/auth/logout", false),
    rpc("Accounts", "List", "GET", "/api/accounts", false),
    rpc("Accounts", "Add", "POST", "/api/accounts", false),
    rpc("Accounts", "RequestAccess", "POST", "/api/accounts/request", false),
    rpc("Accounts", "ListRequests", "GET", "/api/accounts/requests", false),
    rpc("Accounts", "DecideRequest", "POST", "/api/accounts/requests/{id}/decide", false),
    rpc("Projects", "List", "GET", "/api/projects", false),
    rpc("Projects", "Create", "POST", "/api/projects", false),
    rpc("Projects", "Get", "GET", "/api/projects/{id}", false),
    rpc("Projects", "Update", "PUT", "/api/projects/{id}", false),
    rpc("Projects", "Delete", "DELETE", "/api/projects/{id}", false),
    rpc("Projects", "ListChildren", "GET", "/api/projects/{id}/children", false),
    rpc("Projects", "ListGithubCommits", "GET", "/api/projects/{id}/github/commits", false),
    rpc("Tickets", "List", "GET", "/api/tickets", true),
    rpc("Tickets", "Create", "POST", "/api/tickets", false),
    rpc("Tickets", "Get", "GET", "/api/tickets/{id}", false),
    rpc("Tickets", "Update", "PUT", "/api/tickets/{id}", false),
    rpc("Comments", "List", "GET", "/api/tickets/{id}/comments", false),
    rpc("Comments", "Create", "POST", "/api/tickets/{id}/comments", false),
    rpc("Comments", "Delete", "DELETE", "/api/comments/{id}", false),
    rpc("Attachments", "List", "GET", "/api/tickets/{id}/attachments", false),
    rpc("Attachments", "Create", "POST", "/api/tickets/{id}/attachments", false),
    rpc("Attachments", "Download", "GET", "/api/attachments/{id}/download", false),
    rpc("Attachments", "Delete", "DELETE", "/api/attachments/{id}", false),
    rpc("Relations", "List", "GET", "/api/tickets/{id}/relations", false),
    rpc("Relations", "Create", "POST", "/api/tickets/{id}/relations", false),
    rpc("Relations", "Delete", "DELETE", "/api/relations/{id}", false),
    rpc("TimeEntries", "List", "GET", "/api/tickets/{id}/time_entries", false),
    rpc("TimeEntries", "Create", "POST", "/api/tickets/{id}/time_entries", false),
    rpc("TimeEntries", "Delete", "DELETE", "/api/time_entries/{id}", false),
    rpc("Wiki", "List", "GET", "/api/projects/{id}/wiki", false),
    rpc("Wiki", "Create", "POST", "/api/projects/{id}/wiki", false),
    rpc("Wiki", "Get", "GET", "/api/wiki/{id}", false),
    rpc("Wiki", "Update", "PUT", "/api/wiki/{id}", false),
    rpc("Wiki", "Delete", "DELETE", "/api/wiki/{id}", false),
    rpc("SavedQueries", "List", "GET", "/api/saved_queries", false),
    rpc("SavedQueries", "Create", "POST", "/api/saved_queries", false),
    rpc("SavedQueries", "Delete", "DELETE", "/api/saved_queries/{id}", false),
    rpc("SavedQueries", "Run", "GET", "/api/saved_queries/{id}/run", false),
];

/// 公開するConnect経路の一覧(`/open_redmine.v1.Tickets/List` の形)。
pub fn paths() -> Vec<String> {
    TABLE.iter().map(|r| format!("/{PACKAGE}.{}/{}", r.service, r.method)).collect()
}

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    let code = match status.as_u16() {
        400 => "invalid_argument",
        401 => "unauthenticated",
        403 => "permission_denied",
        404 => "not_found",
        409 => "already_exists",
        413 => "resource_exhausted",
        415 => "invalid_argument",
        501 => "unimplemented",
        502 | 503 => "unavailable",
        s if s >= 500 => "internal",
        _ => "unknown",
    };
    let body = serde_json::json!({ "code": code, "message": message.into() });
    Response::builder().status(status).content_type("application/json").body(serde_json::to_vec(&body).unwrap_or_default())
}

// ---- 厳格なJSON読み込み(重複キー・深さの上限つき) ----

struct Strict {
    depth: usize,
}

impl<'de> serde::de::DeserializeSeed<'de> for Strict {
    type Value = Value;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        d.deserialize_any(StrictVisitor { depth: self.depth })
    }
}

struct StrictVisitor {
    depth: usize,
}

impl<'de> serde::de::Visitor<'de> for StrictVisitor {
    type Value = Value;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a JSON value")
    }
    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }
    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(v.into())
    }
    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        Ok(v.into())
    }
    fn visit_f64<E>(self, v: f64) -> Result<Value, E> {
        Ok(serde_json::Number::from_f64(v).map(Value::Number).unwrap_or(Value::Null))
    }
    fn visit_str<E>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_string()))
    }
    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }
    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        if self.depth >= MAX_DEPTH {
            return Err(serde::de::Error::custom("nesting too deep"));
        }
        let mut out = Vec::new();
        while let Some(v) = seq.next_element_seed(Strict { depth: self.depth + 1 })? {
            out.push(v);
        }
        Ok(Value::Array(out))
    }
    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        if self.depth >= MAX_DEPTH {
            return Err(serde::de::Error::custom("nesting too deep"));
        }
        let mut out = Map::new();
        while let Some(k) = map.next_key::<String>()? {
            if out.contains_key(&k) {
                return Err(serde::de::Error::custom(format!("duplicate key: {k}")));
            }
            let v = map.next_value_seed(Strict { depth: self.depth + 1 })?;
            out.insert(k, v);
        }
        Ok(Value::Object(out))
    }
}

fn parse_strict(bytes: &[u8]) -> Result<Value, String> {
    use serde::de::DeserializeSeed;
    let mut de = serde_json::Deserializer::from_slice(bytes);
    let v = Strict { depth: 0 }.deserialize(&mut de).map_err(|e| e.to_string())?;
    de.end().map_err(|e| e.to_string())?;
    Ok(v)
}

fn pct(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

fn multipart_body(file_name: &str, content_type: &str, data: &[u8]) -> (String, Vec<u8>) {
    let boundary = format!("----openredmine{:016x}", rand::random::<u64>());
    let safe_name = file_name.replace(['"', '\r', '\n'], "_");
    let safe_type = content_type.replace(['\r', '\n'], "");
    let mut body = Vec::new();
    body.extend_from_slice(
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{safe_name}\"\r\nContent-Type: {safe_type}\r\n\r\n").as_bytes(),
    );
    body.extend_from_slice(data);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

async fn handle(inner: Shared, req: Request) -> Response {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;

    // /open_redmine.v1.Service/Method
    let path = req.uri().path().trim_start_matches('/').to_string();
    let Some((svc_full, method)) = path.split_once('/') else {
        return error(StatusCode::NOT_FOUND, "unknown method");
    };
    let svc = svc_full.strip_prefix(&format!("{PACKAGE}.")).unwrap_or("");
    let Some(r) = TABLE.iter().find(|r| r.service == svc && r.method == method) else {
        return error(StatusCode::NOT_FOUND, "unknown method");
    };
    let ct = req.header(header::CONTENT_TYPE).unwrap_or("").to_ascii_lowercase();
    if !ct.starts_with("application/json") {
        return error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "content-type must be application/json");
    }
    let is_upload = r.service == "Attachments" && r.method == "Create";
    let limit = if is_upload { MAX_UPLOAD_BODY } else { MAX_BODY };
    let mut headers = Vec::new();
    for (k, v) in req.headers() {
        if k != header::CONTENT_TYPE && k != header::CONTENT_LENGTH && k != header::HOST && k != header::TRANSFER_ENCODING {
            headers.push((k.clone(), v.clone()));
        }
    }
    let bytes = match req.into_body().into_bytes_limit(limit).await {
        Ok(b) => b,
        Err(_) => return error(StatusCode::PAYLOAD_TOO_LARGE, "request body too large"),
    };
    let mut obj = if bytes.iter().all(|b| b.is_ascii_whitespace()) {
        Map::new()
    } else {
        match parse_strict(&bytes) {
            Ok(Value::Object(m)) => m,
            Ok(_) => return error(StatusCode::BAD_REQUEST, "request body must be a JSON object"),
            Err(e) => return error(StatusCode::BAD_REQUEST, format!("invalid JSON: {e}")),
        }
    };

    let mut path = r.path.to_string();
    if path.contains("{id}") {
        let id = match obj.remove("id").as_ref().and_then(|v| v.as_u64()) {
            Some(id) => id,
            None => return error(StatusCode::BAD_REQUEST, "`id` (unsigned integer) is required"),
        };
        path = path.replace("{id}", &id.to_string());
    }

    let mut builder = Request::builder().method(Method::from_bytes(r.http.as_bytes()).unwrap());
    for (k, v) in headers {
        builder = builder.header(k, v);
    }
    let inner_req = if r.query {
        let qs: Vec<String> = obj
            .iter()
            .filter_map(|(k, v)| {
                let s = match v {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    _ => return None,
                };
                Some(format!("{}={}", pct(k), pct(&s)))
            })
            .collect();
        let uri = if qs.is_empty() { path } else { format!("{path}?{}", qs.join("&")) };
        builder.uri(parse_uri(&uri)).finish()
    } else if is_upload {
        let name = obj.get("file_name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let ctype = obj.get("content_type").and_then(|v| v.as_str()).unwrap_or("application/octet-stream").to_string();
        let Some(data) = obj.get("data_base64").and_then(|v| v.as_str()) else {
            return error(StatusCode::BAD_REQUEST, "`file_name` and `data_base64` are required");
        };
        let Ok(data) = b64.decode(data) else {
            return error(StatusCode::BAD_REQUEST, "`data_base64` is not valid base64");
        };
        if name.trim().is_empty() {
            return error(StatusCode::BAD_REQUEST, "`file_name` and `data_base64` are required");
        }
        let (mime, body) = multipart_body(&name, &ctype, &data);
        builder.uri(parse_uri(&path)).content_type(&mime).body(body)
    } else if r.http == "GET" || r.http == "DELETE" {
        builder.uri(parse_uri(&path)).finish()
    } else {
        builder.uri(parse_uri(&path)).content_type("application/json").body(serde_json::to_vec(&Value::Object(obj)).unwrap_or_default())
    };

    let resp = inner.get_response(inner_req).await;
    let status = resp.status();
    let resp_ct = resp.header(header::CONTENT_TYPE).unwrap_or("").to_string();
    let disposition = resp.header(header::CONTENT_DISPOSITION).unwrap_or("").to_string();
    let body = match resp.into_body().into_bytes().await {
        Ok(b) => b,
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "failed to read response"),
    };
    if status.as_u16() >= 400 {
        return error(status, String::from_utf8_lossy(&body).to_string());
    }
    if r.service == "Attachments" && r.method == "Download" {
        let file_name = disposition.split("filename=\"").nth(1).and_then(|s| s.split('"').next()).unwrap_or("download").to_string();
        let out = serde_json::json!({ "file_name": file_name, "content_type": resp_ct, "data_base64": b64.encode(&body) });
        return Response::builder().status(StatusCode::OK).content_type("application/json").body(serde_json::to_vec(&out).unwrap_or_default());
    }
    if resp_ct.starts_with("application/json") {
        Response::builder().status(StatusCode::OK).content_type("application/json").body(body)
    } else {
        let out = serde_json::json!({ "message": String::from_utf8_lossy(&body) });
        Response::builder().status(StatusCode::OK).content_type("application/json").body(serde_json::to_vec(&out).unwrap_or_default())
    }
}

fn parse_uri(s: &str) -> poem::http::Uri {
    s.parse().unwrap_or_else(|_| poem::http::Uri::from_static("/"))
}

/// 内部ルーターを共有可能な形にする。
pub fn share<E>(inner: E) -> Shared
where
    E: Endpoint + 'static,
    E::Output: IntoResponse,
{
    Arc::new(inner.map_to_response().boxed())
}

/// 内部ルーターを包んだConnectエンドポイントを作る。
pub fn endpoint(inner: Shared) -> impl Endpoint {
    poem::endpoint::make(move |req: Request| {
        let inner = inner.clone();
        async move { handle(inner, req).await }
    })
}
