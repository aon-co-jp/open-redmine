# open-redmine

(Formerly `RS-Chiketto`, renamed to `RS-Red` on 2026-07-22, then to
`open-redmine` on 2026-07-27. Public site: `https://easy-web.tokyo/open-redmine`)

**Development started: 2026-07-21** (GitHub repository creation date)

A high-speed, high-security, low-memory Rust + [poem](https://github.com/poem-web/poem) (RPoem)
port of [Redmine](https://redmine.org/). Intended to run cheaply on rental VPS hosting.

> ⚠️ As of v0.1.0, only Ticket (Issue)/Project CRUD, sub-project hierarchy, ticket comments,
> and per-project Wiki (with revision history) are implemented. Overall feature coverage
> compared to full Redmine is still roughly 20–30%. See `CLAUDE.md` (Japanese) for details.

## Browser GUI (`web/`, Rust → WebAssembly)

Since this is a ticket-management web app, a GUI is treated as a core feature — an
online-only browser frontend ships alongside the server (no Tauri, Node.js, or TypeScript).
Running the server with `cargo run` automatically serves it at `GET /` (falls back to the
legacy API-overview page if `web/index.html` + `web/pkg/` are not present).

- Supports OTP login, project listing/creation, ticket listing/creation/detail view/status
  changes/comments, and Wiki listing/creation/viewing.
- Pinch-zoom works out of the box on Android/iOS mobile browsers via the standard `viewport`
  meta tag — no special implementation needed.

### Building the GUI

```bash
cd web
rustup target add wasm32-unknown-unknown  # first time only
cargo install wasm-bindgen-cli             # first time only, match the Cargo.lock version
cargo build --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir pkg target/wasm32-unknown-unknown/debug/rs_red_web.wasm
```

## API Endpoints

| Method / Path | Description |
| --- | --- |
| `GET /healthz` | Health check |
| `POST /api/auth/request-otp` | Emails a one-time login password |
| `POST /api/auth/verify-otp` | Verifies the OTP and issues a session token |
| `POST /api/auth/logout` | Logs out (revokes the token) |
| `GET /api/accounts` / `POST /api/accounts` | List registered accounts / add one (admin only) |
| `POST /api/accounts/request` | Self-service account request (no auth required) |
| `GET /api/accounts/requests` | List pending self-service requests (admin only) |
| `POST /api/accounts/requests/:id/decide` | Approve/deny a request and grant project view/edit access (admin only) |
| `GET /api/projects` / `POST /api/projects` | List projects (no auth required) / create (admin only, `parent_id` for sub-projects) |
| `GET /api/projects/:id` / `PUT /api/projects/:id` / `DELETE /api/projects/:id` | Get project (no auth required) / update or delete (admin only; changing `parent_id` rejects cycles) |
| `GET /api/projects/:id/children` | List direct child projects (no auth required) |
| `GET /api/tickets` / `POST /api/tickets` | List tickets (only projects you can access; filterable by `status`/`project_id`) / create (requires an existing `project_id`; `start_date`/`due_date`/`done_ratio` are optional Gantt-chart fields) |
| `GET /api/tickets/:id` / `PUT /api/tickets/:id` | Get ticket / update (status and `start_date`/`due_date`/`done_ratio` changes included) |
| `GET /api/tickets/:id/comments` / `POST /api/tickets/:id/comments` | List comments (view access required) / post (edit access required) |
| `DELETE /api/comments/:id` | Delete a comment (admin or the original author only) |
| `GET /api/projects/:id/wiki` / `POST /api/projects/:id/wiki` | List a project's Wiki pages (view access required) / create (edit access required, `slug` unique per project) |
| `GET /api/wiki/:id` / `PUT /api/wiki/:id` | Get a Wiki page (with revision history, view access required) / append a new revision (edit access required, old revisions kept) |
| `DELETE /api/wiki/:id` | Delete a Wiki page (admin only) |

## DDNS operation (for connections without a fixed IP)

Set the environment variable `RSCHIKETTO_DDNS_UPDATE_URL` to a URL with `{ip}` written where
the current global IP should be substituted. Every 5 minutes the server checks its global IP
and, only if it changed, hits that URL to update it (opt-in, disabled by default — not needed
on a fixed-IP connection). Example (DuckDNS):

```
RSCHIKETTO_DDNS_UPDATE_URL=https://www.duckdns.org/update?domains=myhost&token=xxxx&ip={ip}
```

This runs on the Windows/Linux native binary. **Honest disclosure**: on Android, this
always-on DDNS updater will only actually work once APK packaging (not yet started, same
situation as `open-web-server`) is complete — see the HANDOFF section of `CLAUDE.md`
(Japanese) for the current status of the Android build.

## Choosing a data/DB storage destination (`StorageBackend`)

`src/storage.rs` implements a `StorageBackend` abstraction selectable via the
`RSCHIKETTO_STORAGE_BACKEND` environment variable (`local`/`sftp`/`gdrive`, default `local`).
Every existing `Store` (`project.rs`/`comments.rs`/`wiki.rs`/`accounts.rs`/`access.rs`)
has its `load`/`save` wired through this `StorageBackend` trait.

- **`local` (default)**: `LocalFsBackend`, verified with real file I/O tests.
- **`sftp`**: for VPS/rental-server hosting, built on the `ssh2` crate. `read`/`write`/
  `ensure_dir` are implemented, but **this development environment has no real SFTP server
  available, so end-to-end network connectivity has not yet been verified** (honest
  disclosure).
- **`gdrive`**: for Google Drive, implemented by calling the REST API directly with
  `reqwest` (`files.list` name lookup → `files.get` download, plus upload). **Verification
  against the real API with a live API key has not been performed** (honest disclosure).

**Because of the above, selecting `RSCHIKETTO_STORAGE_BACKEND=sftp`/`gdrive` currently falls
back automatically to `LocalFsBackend` as a safety measure**, until real network
connectivity has been confirmed (see `backend_from_env()` in `storage.rs`). Using Google
Drive or similar cloud APIs requires the user to obtain their own OAuth2 credentials — this
is not something the software can obtain on the user's behalf. See the HANDOFF section of
`CLAUDE.md` and `PORTING.md` (Japanese) for details and remaining work.

## Installation (prebuilt binaries with installer)

For each tagged release (`vX.Y.Z`), GitHub Actions (`.github/workflows/release.yml`)
automatically builds Linux/Windows binaries and attaches them to
[GitHub Releases](https://github.com/aon-co-jp/RS-Chiketto/releases).

### Linux (AlmaLinux, Ubuntu, Debian, Fedora, RHEL, and other major systemd distros)

The binary is a statically linked musl build, so there are no distro-specific library
dependencies.

```bash
curl -fsSL https://github.com/aon-co-jp/RS-Chiketto/releases/latest/download/rs-chiketto-linux-x86_64.tar.gz | tar xz
sudo ./install.sh
sudo systemctl edit rs-chiketto   # set RSCHIKETTO_ADMIN_EMAIL, etc.
sudo systemctl enable --now rs-chiketto
```

### Windows / Windows Server

In an administrator PowerShell prompt:

```powershell
Invoke-WebRequest -Uri "https://github.com/aon-co-jp/RS-Chiketto/releases/latest/download/rs-chiketto-windows-x86_64.zip" -OutFile rs-chiketto.zip
Expand-Archive rs-chiketto.zip -DestinationPath rs-chiketto
cd rs-chiketto
.\install.ps1
```

## Building from source

```bash
cargo build --release
```

## License

Apache-2.0
