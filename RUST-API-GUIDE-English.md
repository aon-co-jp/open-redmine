# open-redmine Rust API Client Guide

open-redmine exposes a plain JSON-over-HTTP API — there is no dedicated Rust
SDK crate. You call it directly with any HTTP client (`reqwest` in the
examples below). This guide documents the request/response patterns and
gives working Rust code for every endpoint.

**Honest disclosure**: this guide documents *how to call* the API (auth,
endpoint list, request/response shapes). It does not claim full feature
parity with upstream Redmine — custom fields, saved queries, a Gantt/
calendar GUI, forums/news, and SCM integration are not implemented. See
`CLAUDE.md`'s HANDOFF log and the "Unimplemented features" section below
for the current, honest scope.

## Table of contents

1. [Authentication (email OTP login)](#1-authentication-email-otp-login)
2. [Common request pattern](#2-common-request-pattern)
3. [Projects](#3-projects)
4. [Tickets](#4-tickets)
5. [Comments](#5-comments)
6. [Issue relations](#6-issue-relations)
7. [Time entries](#7-time-entries)
8. [Attachments](#8-attachments)
9. [Wiki](#9-wiki)
10. [Accounts / access requests](#10-accounts--access-requests)
11. [Unimplemented features (honest disclosure)](#11-unimplemented-features-honest-disclosure)

## Dependencies (caller's Cargo.toml)

```toml
[dependencies]
reqwest = { version = "0.12", features = ["json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
```

## 1. Authentication (email OTP login)

open-redmine has no fixed password — login is a one-time-password sent by
email.

```
POST /api/auth/request-otp   {"email": "you@example.com"}   -> 200 (OTP emailed)
POST /api/auth/verify-otp    {"email": "...", "code": "123456"} -> 200 {"token": "..."}
POST /api/auth/logout        (Authorization: Bearer <token>)  -> 200
```

Attach `Authorization: Bearer <token>` to every subsequent request (some
endpoints don't require it — see each section below).

```rust
use serde::{Deserialize, Serialize};

const BASE_URL: &str = "http://127.0.0.1:8100"; // production: https://easy-web.tokyo/open-redmine

#[derive(Serialize)]
struct RequestOtp<'a> {
    email: &'a str,
}

#[derive(Serialize)]
struct VerifyOtp<'a> {
    email: &'a str,
    code: &'a str,
}

#[derive(Deserialize)]
struct VerifyOtpResponse {
    token: String,
}

/// Trigger an OTP email. (For development/demo without SMTP, start the
/// server with `RSCHIKETTO_DEV_LOG_OTP=true` to have the code logged to
/// the server console instead — never enable this in production, see
/// CLAUDE.md.)
async fn request_otp(client: &reqwest::Client, email: &str) -> anyhow::Result<()> {
    client
        .post(format!("{BASE_URL}/api/auth/request-otp"))
        .json(&RequestOtp { email })
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

/// Exchange the OTP code received by email for a session token.
async fn verify_otp(client: &reqwest::Client, email: &str, code: &str) -> anyhow::Result<String> {
    let resp: VerifyOtpResponse = client
        .post(format!("{BASE_URL}/api/auth/verify-otp"))
        .json(&VerifyOtp { email, code })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(resp.token)
}
```

## 2. Common request pattern

- **Base URL**: production is `https://easy-web.tokyo/open-redmine`
  (demo: `https://easy-web.tokyo/open-redmine/demo` — currently an alias
  to the same backend as production, not an isolated demo dataset). Local
  development is `http://127.0.0.1:<RSCHIKETTO_PORT>` (default `8100`).
- **Auth header**: `Authorization: Bearer <token>`, obtained from
  `verify_otp`.
- **Status codes**: `401` = not logged in, `403` = logged in but lacking
  permission, `404` = target doesn't exist (many handlers check existence
  before permission, so which of 404/403 comes first varies by endpoint —
  see the individual HANDOFF entries in `CLAUDE.md` for details), `400` =
  validation failure (invalid email, out-of-range `done_ratio`, nonexistent
  `project_id`, etc).
- **Access control**: view/edit permission is granted per project
  (`access.rs`). Administrators always have full access.

A reusable client wrapper (used by every example below as `ApiClient`):

```rust
use reqwest::Client;
use serde::de::DeserializeOwned;

struct ApiClient {
    http: Client,
    base_url: String,
    token: String,
}

impl ApiClient {
    fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self { http: Client::new(), base_url: base_url.into(), token: token.into() }
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        Ok(self
            .http
            .get(format!("{}{}", self.base_url, path))
            .bearer_auth(&self.token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    async fn post_json<B: serde::Serialize, T: DeserializeOwned>(&self, path: &str, body: &B) -> anyhow::Result<T> {
        Ok(self
            .http
            .post(format!("{}{}", self.base_url, path))
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    async fn put_json<B: serde::Serialize, T: DeserializeOwned>(&self, path: &str, body: &B) -> anyhow::Result<T> {
        Ok(self
            .http
            .put(format!("{}{}", self.base_url, path))
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    async fn delete(&self, path: &str) -> anyhow::Result<()> {
        self.http.delete(format!("{}{}", self.base_url, path)).bearer_auth(&self.token).send().await?.error_for_status()?;
        Ok(())
    }
}
```

## 3. Projects

```
POST   /api/projects              {"name","description","parent_id"(optional)} -> 201 Project  [admin only]
GET    /api/projects                                                           -> 200 [Project]  [no auth required]
GET    /api/projects/:id                                                       -> 200 Project    [no auth required]
PUT    /api/projects/:id          (only supplied fields are updated)            -> 200 Project    [admin only]
DELETE /api/projects/:id                                                       -> 200             [admin only]
GET    /api/projects/:id/children                                             -> 200 [Project]  [no auth required]
```

```rust
#[derive(serde::Deserialize, Debug)]
struct Project {
    id: u64,
    name: String,
    description: String,
    parent_id: Option<u64>,
    created_at: String,
    updated_at: String,
}

#[derive(serde::Serialize)]
struct CreateProjectReq<'a> {
    name: &'a str,
    description: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_id: Option<u64>,
}

async fn create_project(api: &ApiClient, name: &str, description: &str) -> anyhow::Result<Project> {
    api.post_json("/api/projects", &CreateProjectReq { name, description, parent_id: None }).await
}

async fn list_projects(api: &ApiClient) -> anyhow::Result<Vec<Project>> {
    api.get("/api/projects").await
}
```

## 4. Tickets

```
POST /api/tickets            {"title","description","project_id","tracker"?,"start_date"?,"due_date"?,"done_ratio"?,"assignee"?} -> 201 Ticket
GET  /api/tickets             ?status=open|in_progress|closed  ?project_id=  ?tracker=  ?assignee=  -> 200 [Ticket]
GET  /api/tickets/:id                                                        -> 200 Ticket
PUT  /api/tickets/:id         (only supplied fields are updated, including status) -> 200 Ticket
```

`tracker` is one of `"bug"` / `"feature"` / `"support"` / `"task"` (defaults
to `bug`). `assignee` must be either the admin email or an email already
registered in `accounts`, otherwise `400`. `done_ratio` outside 0–100
returns `400`.

```rust
#[derive(serde::Deserialize, Debug)]
struct Ticket {
    id: u64,
    project_id: u64,
    title: String,
    description: String,
    status: String, // "open" | "in_progress" | "closed"
    tracker: String, // "bug" | "feature" | "support" | "task"
    start_date: Option<String>,
    due_date: Option<String>,
    done_ratio: u8,
    assignee: Option<String>,
}

#[derive(serde::Serialize)]
struct CreateTicketReq<'a> {
    title: &'a str,
    description: &'a str,
    project_id: u64,
}

async fn create_ticket(api: &ApiClient, project_id: u64, title: &str, description: &str) -> anyhow::Result<Ticket> {
    api.post_json("/api/tickets", &CreateTicketReq { title, description, project_id }).await
}

async fn list_open_tickets(api: &ApiClient, project_id: u64) -> anyhow::Result<Vec<Ticket>> {
    api.get(&format!("/api/tickets?project_id={project_id}&status=open")).await
}

#[derive(serde::Serialize)]
struct UpdateTicketStatusReq<'a> {
    status: &'a str,
}

async fn close_ticket(api: &ApiClient, id: u64) -> anyhow::Result<Ticket> {
    api.put_json(&format!("/api/tickets/{id}"), &UpdateTicketStatusReq { status: "closed" }).await
}
```

## 5. Comments

```
POST /api/tickets/:id/comments  {"body": "..."}  -> 201 Comment
GET  /api/tickets/:id/comments                    -> 200 [Comment]
DELETE /api/comments/:id                           -> 200  [admin or the comment's author only]
```

## 6. Issue relations

```
POST /api/tickets/:id/relations  {"to_ticket_id": u64, "kind": "blocks"|"duplicates"|"precedes"} -> 201 IssueRelation
GET  /api/tickets/:id/relations                                                                   -> 200 [IssueRelation]  (both from/to sides)
DELETE /api/relations/:id                                                                          -> 200
```

Self-reference, a nonexistent `to_ticket_id`, or a duplicate
`(from, to, kind)` triple all return `400`.

## 7. Time entries

```
POST /api/tickets/:id/time_entries  {"hours": f64, "activity": "...", "comments"?: "...", "spent_on": "YYYY-MM-DD"} -> 201 TimeEntry
GET  /api/tickets/:id/time_entries                                                                                   -> 200 [TimeEntry]
DELETE /api/time_entries/:id                                                                                          -> 200  [admin or the entry's author only]
```

`hours` must be greater than 0 and at most 24, otherwise `400`.

## 8. Attachments

Only attachment creation uses `multipart/form-data`; everything else in
this API is JSON. Use `reqwest`'s `multipart::Form`.

```
POST /api/tickets/:id/attachments   (multipart/form-data, single part, any field name) -> 201 Attachment
GET  /api/tickets/:id/attachments                                                       -> 200 [Attachment]
GET  /api/attachments/:id/download                                                       -> 200 (raw bytes)
DELETE /api/attachments/:id                                                               -> 200
```

```rust
async fn upload_attachment(api: &ApiClient, ticket_id: u64, filename: &str, bytes: Vec<u8>) -> anyhow::Result<serde_json::Value> {
    let part = reqwest::multipart::Part::bytes(bytes).file_name(filename.to_string());
    let form = reqwest::multipart::Form::new().part("file", part);
    let resp = api
        .http
        .post(format!("{}/api/tickets/{ticket_id}/attachments", api.base_url))
        .bearer_auth(&api.token)
        .multipart(form)
        .send()
        .await?
        .error_for_status()?;
    Ok(resp.json().await?)
}

async fn download_attachment(api: &ApiClient, attachment_id: u64) -> anyhow::Result<Vec<u8>> {
    let resp = api
        .http
        .get(format!("{}/api/attachments/{attachment_id}/download", api.base_url))
        .bearer_auth(&api.token)
        .send()
        .await?
        .error_for_status()?;
    Ok(resp.bytes().await?.to_vec())
}
```

## 9. Wiki

```
POST /api/projects/:id/wiki  {"slug","title","body"} -> 201 WikiPage  (slug must be unique within the project)
GET  /api/projects/:id/wiki                            -> 200 [WikiPage]
GET  /api/wiki/:id                                      -> 200 WikiPage (latest revision included)
PUT  /api/wiki/:id           {"body","title"?}          -> 200 WikiPage (appends a new revision, old content kept)
DELETE /api/wiki/:id                                    -> 200  [admin only]
```

## 10. Accounts / access requests

```
POST /api/accounts              {"email"}                     -> 201  [admin only, register a login-capable address]
GET  /api/accounts                                             -> 200 [String]  [admin only]
POST /api/accounts/request      {"email","project_id"?, ...}   -> 201  [no auth required, self-service request]
GET  /api/accounts/requests                                    -> 200 [AccessRequest]  [admin only]
POST /api/accounts/requests/:id/decide  {"approve": bool, "project_id"?, "allow_view"?, "allow_edit"?, "allow_manage_members"?} -> 200  [admin, or an account with ManageMembers on that project_id]
```

Granting `allow_manage_members` for the first time is rejected with `403`
unless the reviewer is a global administrator (privilege-escalation
prevention, see the 2026-07-27 HANDOFF entry in `CLAUDE.md`).

## 11. Unimplemented features (honest disclosure)

The following exist in upstream Redmine but are **not** implemented here
(calling them returns `404` as an unknown route). Check `CLAUDE.md`'s
"次にすべきこと" (next steps) HANDOFF entries for current status.

- Custom fields
- Saved custom queries/filters
- Gantt chart / calendar aggregation & rendering API (the `start_date` /
  `due_date` / `done_ratio` fields on `Ticket` exist, so a client can build
  its own chart from them)
- Named role presets (Manager/Developer/Reporter, etc — currently only
  the individual `allow_view` / `allow_edit` / `allow_manage_members` flags)
- Forums, news, and documents modules
- SCM (repository) integration
- Watchers
- Notification emails (automatic email on ticket update)
