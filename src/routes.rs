use crate::auth::{hash_password, new_session_token, require_auth, verify_password};
use crate::backup;
use crate::config::CrashAction;
use crate::files;
use crate::plugins;
use crate::region;
use crate::state::AppState;
use crate::ws::{console_ws, tasks_ws};
use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path as AxPath, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{get, post, delete},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use tokio_util::io::ReaderStream;

const STATIC_INDEX: &str = include_str!("../static/index.html");
const STATIC_CONTROL: &str = include_str!("../static/control.html");
const STATIC_LOGIN: &str = include_str!("../static/login.html");
const STATIC_SETUP: &str = include_str!("../static/setup.html");
const STATIC_APP_JS: &str = include_str!("../static/app.js");
const STATIC_STYLE_CSS: &str = include_str!("../static/style.css");
const STATIC_DEALS: &str = include_str!("../static/deals.html");
const STATIC_DEAL: &str = include_str!("../static/deal.html");

pub fn router(state: AppState) -> Router {
    crate::backup::spawn_auto_backup_task(state.clone());
    let api = Router::new()
        .route("/setup/status", get(setup_status))
        .route("/setup", post(do_setup))
        .route("/setup/task/:id", get(setup_task_progress))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/me", get(me))
        .route("/server/info", get(server_info))
        .route("/server/start", post(server_start))
        .route("/server/stop", post(server_stop))
        .route("/server/restart", post(server_restart))
        .route("/server/kill", post(server_kill))
        .route("/server/command", post(server_command))
        .route("/console/snapshot", get(console_snapshot))
        .route("/logs", get(logs_list))
        .route("/logs/:name", get(logs_download))
        .route("/config", get(get_config).put(put_config))
        .route("/server-properties", get(get_server_properties).put(put_server_properties))
        .route("/files", get(list_files))
        .route("/files/read", get(read_file))
        .route("/files/write", post(write_file))
        .route("/files/delete", post(delete_file))
        .route("/files/upload", post(upload_file))
        .route("/files/download", get(download_file))
        .route("/files/mkdir", post(mkdir))
        .route("/plugins", get(list_plugins))
        .route("/plugins/upload", post(upload_plugin))
        .route("/plugins/url", post(plugin_from_url))
        .route("/plugins/delete", post(delete_plugin))
        .route("/plugins/toggle", post(toggle_plugin))
        .route("/plugins/config", get(get_plugin_config).put(put_plugin_config))
        .route("/plugins/configs", get(list_plugin_configs))
        .route("/backups", get(list_backups).post(create_backup))
        .route("/backups/:name", delete(delete_backup))
        .route("/backups/:name/download", get(download_backup))
        .route("/regions/dimensions", get(list_dimensions))
        .route("/regions/:dim", get(list_region_files))
        .route("/regions/:dim/:file/chunks", get(read_chunks))
        .route("/regions/:dim/:file/chunk/clear", post(clear_chunk))
        .route("/metrics", get(get_metrics))
        .route("/metrics/history", get(get_metrics_history))
        .route("/tasks", get(list_tasks))
        .route("/players", get(get_players))
        .route("/players/kick", post(kick_player))
        .route("/marketplace/search", get(marketplace_search))
        .route("/marketplace/install", post(marketplace_install))
        .route("/deals", get(admin_list_deals))
        .route("/deals/:id", delete(admin_delete_deal));

    let ws_router = Router::new()
        .route("/ws/console", get(console_ws))
        .route("/ws/tasks", get(tasks_ws));

    // Public mcsapi for the in-game plugin / external integrations.
    let mcsapi = Router::new()
        .route("/record/list", get(mcs_list))
        .route("/record/view", get(mcs_view))
        .route("/record/create", post(mcs_create_post).get(mcs_create_get))
        .route("/record/approve", get(mcs_approve))
        .route("/record/reject", get(mcs_reject));

    Router::new()
        .route("/", get(root_redirect))
        .route("/control", get(serve_control))
        .route("/login", get(serve_login))
        .route("/setup", get(serve_setup))
        .route("/deals", get(serve_deals))
        .route("/deals/:id", get(serve_deal))
        .route("/static/app.js", get(serve_app_js))
        .route("/static/style.css", get(serve_style_css))
        .nest("/api", api)
        .nest("/mcsapi", mcsapi)
        .merge(ws_router)
        .layer(DefaultBodyLimit::max(512 * 1024 * 1024)) // 512 MiB uploads
        .layer(axum::middleware::from_fn_with_state(state.clone(), require_auth))
        .with_state(state)
}

async fn root_redirect() -> Response {
    Response::builder().status(StatusCode::FOUND).header("location", "/control").body(Body::empty()).unwrap()
}

async fn serve_control() -> impl IntoResponse { html(STATIC_CONTROL) }
async fn serve_login() -> impl IntoResponse { html(STATIC_LOGIN) }
async fn serve_setup() -> impl IntoResponse { html(STATIC_SETUP) }
async fn serve_deals() -> impl IntoResponse { html(STATIC_DEALS) }
async fn serve_deal(AxPath(_id): AxPath<String>) -> impl IntoResponse { html(STATIC_DEAL) }

async fn serve_app_js() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "application/javascript; charset=utf-8")], STATIC_APP_JS)
}
async fn serve_style_css() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], STATIC_STYLE_CSS)
}

fn html(body: &'static str) -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(Body::from(body))
        .unwrap()
}

// ---------- Setup ----------

#[derive(Serialize)]
struct SetupStatus { setup_complete: bool }

async fn setup_status(State(s): State<AppState>) -> Json<SetupStatus> {
    Json(SetupStatus { setup_complete: s.config().setup_complete })
}

#[derive(Deserialize)]
struct SetupReq {
    password: String,
    server_dir: Option<String>,
    paper_url: String,
    min_ram_mb: u32,
    max_ram_mb: u32,
    java_path: Option<String>,
    crash_action: CrashAction,
    auto_restart_max: Option<u32>,
    host: Option<String>,
}

async fn do_setup(State(s): State<AppState>, Json(req): Json<SetupReq>) -> Result<Json<serde_json::Value>, ApiError> {
    if s.config().setup_complete {
        return Err(ApiError::bad("already set up"));
    }
    if req.password.len() < 6 { return Err(ApiError::bad("password too short")); }
    if req.min_ram_mb < 256 || req.max_ram_mb < req.min_ram_mb { return Err(ApiError::bad("invalid RAM")); }

    let mut cfg = s.config();
    cfg.password_hash = Some(hash_password(&req.password).map_err(ApiError::server)?);
    cfg.server_dir = PathBuf::from(req.server_dir.unwrap_or_else(|| "server".into()));
    cfg.paper_url = Some(req.paper_url.clone());
    cfg.min_ram_mb = req.min_ram_mb;
    cfg.max_ram_mb = req.max_ram_mb;
    if let Some(j) = req.java_path { cfg.java_path = j; }
    cfg.crash_action = req.crash_action;
    if let Some(n) = req.auto_restart_max { cfg.auto_restart_max = n; }
    if let Some(h) = req.host { cfg.host = h; }

    tokio::fs::create_dir_all(&cfg.server_dir).await.ok();
    let jar = cfg.server_dir.join("server.jar");
    let task = if !jar.exists() {
        let t = s.inner.tasks.create("download_jar", "Downloading server.jar");
        let tasks = s.inner.tasks.clone();
        let url = req.paper_url.clone();
        let target = jar.clone();
        let tid = t.id.clone();
        tokio::spawn(async move {
            match crate::tasks::download_with_progress(&tasks, &tid, &url, &target).await {
                Ok(()) => tasks.finish_done(&tid, "server.jar downloaded"),
                Err(e) => tasks.finish_failed(&tid, format!("download failed: {e}")),
            }
        });
        Some(t)
    } else { None };

    cfg.setup_complete = true;
    cfg.save().await.map_err(ApiError::server)?;
    s.set_config(cfg);
    Ok(Json(json!({ "ok": true, "task": task })))
}

// ---------- Auth ----------

#[derive(Deserialize)]
struct LoginReq { password: String }

async fn login(State(s): State<AppState>, Json(req): Json<LoginReq>) -> Result<Response, ApiError> {
    let cfg = s.config();
    if !cfg.setup_complete { return Err(ApiError::status(StatusCode::SERVICE_UNAVAILABLE, "setup not complete")); }
    let hash = cfg.password_hash.as_deref().ok_or_else(|| ApiError::bad("no password set"))?;
    if !verify_password(&req.password, hash) {
        return Err(ApiError::status(StatusCode::UNAUTHORIZED, "bad password"));
    }
    let token = new_session_token();
    s.inner.sessions.write().insert(token.clone());
    let cookie = format!("cmc_session={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=2592000");
    let mut resp = Json(json!({ "ok": true, "token": token })).into_response();
    resp.headers_mut().insert(header::SET_COOKIE, HeaderValue::from_str(&cookie).unwrap());
    Ok(resp)
}

async fn logout(State(s): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(t) = crate::auth::extract_session(&headers) {
        s.inner.sessions.write().remove(&t);
    }
    let mut resp = Json(json!({ "ok": true })).into_response();
    resp.headers_mut().insert(header::SET_COOKIE,
        HeaderValue::from_static("cmc_session=; Path=/; Max-Age=0"));
    resp
}

async fn me(State(s): State<AppState>) -> Json<serde_json::Value> {
    let cfg = s.config();
    Json(json!({ "setup_complete": cfg.setup_complete, "host": cfg.host }))
}

// ---------- Server lifecycle ----------

async fn server_info(State(s): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::to_value(s.inner.server.info()).unwrap())
}
async fn server_start(State(s): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    s.inner.server.start(s.config()).await.map_err(ApiError::server)?;
    Ok(Json(json!({"ok":true})))
}
async fn server_stop(State(s): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    s.inner.server.stop(false).await.map_err(ApiError::server)?;
    Ok(Json(json!({"ok":true})))
}
async fn server_kill(State(s): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    s.inner.server.stop(true).await.map_err(ApiError::server)?;
    Ok(Json(json!({"ok":true})))
}
async fn server_restart(State(s): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let cfg = s.config();
    let s2 = s.clone();
    tokio::spawn(async move { let _ = s2.inner.server.restart(cfg).await; });
    Ok(Json(json!({"ok":true})))
}

#[derive(Deserialize)]
struct CmdReq { cmd: String }

async fn server_command(State(s): State<AppState>, Json(r): Json<CmdReq>) -> Result<Json<serde_json::Value>, ApiError> {
    s.inner.server.send_command(&r.cmd).await.map_err(ApiError::server)?;
    Ok(Json(json!({"ok":true})))
}

#[derive(Deserialize)]
struct SnapQuery { last: Option<usize> }

async fn console_snapshot(State(s): State<AppState>, Query(q): Query<SnapQuery>) -> Json<serde_json::Value> {
    let lines = s.inner.console.snapshot(q.last.unwrap_or(500));
    Json(json!({ "lines": lines }))
}

// ---------- Logs ----------

async fn logs_list() -> Result<Json<serde_json::Value>, ApiError> {
    let dir = PathBuf::from("data/logs");
    tokio::fs::create_dir_all(&dir).await.ok();
    let mut out = Vec::new();
    let mut rd = tokio::fs::read_dir(&dir).await.map_err(ApiError::server)?;
    while let Some(e) = rd.next_entry().await.map_err(ApiError::server)? {
        let m = e.metadata().await.map_err(ApiError::server)?;
        if !m.is_file() { continue; }
        out.push(json!({
            "name": e.file_name().to_string_lossy(),
            "size": m.len(),
        }));
    }
    Ok(Json(json!({ "logs": out })))
}

async fn logs_download(AxPath(name): AxPath<String>) -> Result<Response, ApiError> {
    let safe = std::path::Path::new(&name).file_name().ok_or_else(|| ApiError::bad("bad name"))?
        .to_string_lossy().to_string();
    let p = PathBuf::from("data/logs").join(&safe);
    file_response(p, &safe).await
}

// ---------- Config ----------

async fn get_config(State(s): State<AppState>) -> Json<serde_json::Value> {
    let mut c = serde_json::to_value(&s.config()).unwrap();
    if let Some(o) = c.as_object_mut() { o.remove("password_hash"); }
    Json(c)
}

#[derive(Deserialize)]
struct ConfigUpdate {
    paper_url: Option<String>,
    min_ram_mb: Option<u32>,
    max_ram_mb: Option<u32>,
    java_path: Option<String>,
    jvm_args: Option<Vec<String>>,
    crash_action: Option<CrashAction>,
    auto_restart_max: Option<u32>,
    host: Option<String>,
    motd: Option<String>,
    auto_backup_enabled: Option<bool>,
    auto_backup_interval_minutes: Option<u64>,
    backup_keep: Option<usize>,
}

async fn put_config(State(s): State<AppState>, Json(u): Json<ConfigUpdate>) -> Result<Json<serde_json::Value>, ApiError> {
    let mut cfg = s.config();
    if let Some(v) = u.paper_url { cfg.paper_url = Some(v); }
    if let Some(v) = u.min_ram_mb { cfg.min_ram_mb = v; }
    if let Some(v) = u.max_ram_mb { cfg.max_ram_mb = v; }
    if let Some(v) = u.java_path { cfg.java_path = v; }
    if let Some(v) = u.jvm_args { cfg.jvm_args = v; }
    if let Some(v) = u.crash_action { cfg.crash_action = v; }
    if let Some(v) = u.auto_restart_max { cfg.auto_restart_max = v; }
    if let Some(v) = u.host { cfg.host = v; }
    if let Some(v) = u.motd { cfg.motd = v; }
    if let Some(v) = u.auto_backup_enabled { cfg.auto_backup_enabled = v; }
    if let Some(v) = u.auto_backup_interval_minutes { cfg.auto_backup_interval_minutes = v; }
    if let Some(v) = u.backup_keep { cfg.backup_keep = v; }
    if cfg.max_ram_mb < cfg.min_ram_mb { return Err(ApiError::bad("max < min RAM")); }
    cfg.save().await.map_err(ApiError::server)?;
    s.set_config(cfg);
    Ok(Json(json!({"ok":true})))
}

async fn get_server_properties(State(s): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let p = s.config().server_dir.join("server.properties");
    let text = if p.exists() { tokio::fs::read_to_string(&p).await.map_err(ApiError::server)? } else { String::new() };
    Ok(Json(json!({ "content": text })))
}

#[derive(Deserialize)]
struct SetText { content: String }

async fn put_server_properties(State(s): State<AppState>, Json(b): Json<SetText>) -> Result<Json<serde_json::Value>, ApiError> {
    let p = s.config().server_dir.join("server.properties");
    if let Some(parent) = p.parent() { tokio::fs::create_dir_all(parent).await.ok(); }
    tokio::fs::write(&p, b.content).await.map_err(ApiError::server)?;
    Ok(Json(json!({"ok":true})))
}

// ---------- Files ----------

#[derive(Deserialize)]
struct PathQuery { path: Option<String> }

async fn list_files(State(s): State<AppState>, Query(q): Query<PathQuery>) -> Result<Json<serde_json::Value>, ApiError> {
    let root = s.config().server_dir.clone();
    tokio::fs::create_dir_all(&root).await.ok();
    let entries = files::list_dir(&root, q.path.as_deref().unwrap_or("")).await.map_err(ApiError::server)?;
    Ok(Json(json!({ "path": q.path.unwrap_or_default(), "entries": entries })))
}

async fn read_file(State(s): State<AppState>, Query(q): Query<PathQuery>) -> Result<Json<serde_json::Value>, ApiError> {
    let p = files::safe_join(&s.config().server_dir, q.path.as_deref().unwrap_or("")).map_err(ApiError::server)?;
    let meta = tokio::fs::metadata(&p).await.map_err(ApiError::server)?;
    if meta.len() > 4 * 1024 * 1024 { return Err(ApiError::bad("file too large for editor (>4MiB)")); }
    let bytes = tokio::fs::read(&p).await.map_err(ApiError::server)?;
    match String::from_utf8(bytes) {
        Ok(s) => Ok(Json(json!({ "content": s, "binary": false }))),
        Err(_) => Ok(Json(json!({ "binary": true }))),
    }
}

#[derive(Deserialize)]
struct WriteReq { path: String, content: String }

async fn write_file(State(s): State<AppState>, Json(r): Json<WriteReq>) -> Result<Json<serde_json::Value>, ApiError> {
    let p = files::safe_join(&s.config().server_dir, &r.path).map_err(ApiError::server)?;
    if let Some(parent) = p.parent() { tokio::fs::create_dir_all(parent).await.ok(); }
    tokio::fs::write(&p, r.content).await.map_err(ApiError::server)?;
    Ok(Json(json!({"ok":true})))
}

#[derive(Deserialize)]
struct DeleteReq { path: String }

async fn delete_file(State(s): State<AppState>, Json(r): Json<DeleteReq>) -> Result<Json<serde_json::Value>, ApiError> {
    let p = files::safe_join(&s.config().server_dir, &r.path).map_err(ApiError::server)?;
    let m = tokio::fs::metadata(&p).await.map_err(ApiError::server)?;
    if m.is_dir() { tokio::fs::remove_dir_all(&p).await.map_err(ApiError::server)?; }
    else { tokio::fs::remove_file(&p).await.map_err(ApiError::server)?; }
    Ok(Json(json!({"ok":true})))
}

#[derive(Deserialize)]
struct MkdirReq { path: String }

async fn mkdir(State(s): State<AppState>, Json(r): Json<MkdirReq>) -> Result<Json<serde_json::Value>, ApiError> {
    let p = files::safe_join(&s.config().server_dir, &r.path).map_err(ApiError::server)?;
    tokio::fs::create_dir_all(&p).await.map_err(ApiError::server)?;
    Ok(Json(json!({"ok":true})))
}

#[derive(Deserialize)]
struct UploadDirQuery { dir: Option<String> }

async fn upload_file(State(s): State<AppState>, Query(q): Query<UploadDirQuery>, mut mp: Multipart) -> Result<Json<serde_json::Value>, ApiError> {
    let mut saved = Vec::new();
    while let Some(field) = mp.next_field().await.map_err(ApiError::server)? {
        let filename = field.file_name().map(|n| n.to_string()).unwrap_or_else(|| "upload.bin".into());
        let safe = std::path::Path::new(&filename).file_name().ok_or_else(|| ApiError::bad("bad name"))?
            .to_string_lossy().to_string();
        let bytes = field.bytes().await.map_err(ApiError::server)?;
        let dir = q.dir.clone().unwrap_or_default();
        let target_dir = files::safe_join(&s.config().server_dir, &dir).map_err(ApiError::server)?;
        tokio::fs::create_dir_all(&target_dir).await.ok();
        let target = target_dir.join(&safe);
        tokio::fs::write(&target, &bytes).await.map_err(ApiError::server)?;
        saved.push(safe);
    }
    Ok(Json(json!({"ok":true, "files": saved})))
}

async fn download_file(State(s): State<AppState>, Query(q): Query<PathQuery>) -> Result<Response, ApiError> {
    let rel = q.path.unwrap_or_default();
    let p = files::safe_join(&s.config().server_dir, &rel).map_err(ApiError::server)?;
    let name = p.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
    file_response(p, &name).await
}

async fn file_response(p: PathBuf, name: &str) -> Result<Response, ApiError> {
    let f = tokio::fs::File::open(&p).await.map_err(|_| ApiError::status(StatusCode::NOT_FOUND, "not found"))?;
    let stream = ReaderStream::new(f);
    let body = Body::from_stream(stream);
    let mime = mime_guess::from_path(&p).first_or_octet_stream();
    let resp = Response::builder()
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\""))
        .body(body)
        .unwrap();
    Ok(resp)
}

// ---------- Plugins ----------

async fn list_plugins(State(s): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let v = plugins::list(&s.config().server_dir).await.map_err(ApiError::server)?;
    Ok(Json(json!({ "plugins": v })))
}

async fn upload_plugin(State(s): State<AppState>, Query(q): Query<ReplaceQuery>, mut mp: Multipart) -> Result<Json<serde_json::Value>, ApiError> {
    let mut names = Vec::new();
    let replace = q.replace.unwrap_or(false);
    while let Some(field) = mp.next_field().await.map_err(ApiError::server)? {
        let filename = field.file_name().unwrap_or("plugin.jar").to_string();
        let bytes = field.bytes().await.map_err(ApiError::server)?;
        let n = plugins::save_uploaded(&s.config().server_dir, &filename, &bytes, replace).await.map_err(ApiError::server)?;
        names.push(n);
    }
    Ok(Json(json!({"ok":true, "files": names})))
}

#[derive(Deserialize)]
struct ReplaceQuery { replace: Option<bool> }

#[derive(Deserialize)]
struct PluginUrlReq { url: String, replace: Option<bool> }

async fn plugin_from_url(State(s): State<AppState>, Json(r): Json<PluginUrlReq>) -> Result<Json<serde_json::Value>, ApiError> {
    let url = r.url.clone();
    let replace = r.replace.unwrap_or(false);
    let parsed = url::Url::parse(&url).map_err(|e| ApiError::bad(format!("invalid URL: {e}")))?;
    let last = parsed.path_segments().and_then(|s| s.last()).unwrap_or("plugin.jar").to_string();
    let name = if last.to_lowercase().ends_with(".jar") { last } else { format!("{last}.jar") };
    if !name.to_lowercase().ends_with(".jar") {
        return Err(ApiError::bad("URL must point to a .jar"));
    }
    let plugins_dir = s.config().server_dir.join("plugins");
    tokio::fs::create_dir_all(&plugins_dir).await.ok();
    let target = plugins_dir.join(&name);
    if target.exists() && !replace {
        return Err(ApiError::bad("plugin already exists; pass replace=true"));
    }
    let task = s.inner.tasks.create("install_plugin", &format!("Install {name}"));
    let tid = task.id.clone();
    let tasks = s.inner.tasks.clone();
    tokio::spawn(async move {
        match crate::tasks::download_with_progress(&tasks, &tid, &url, &target).await {
            Ok(()) => tasks.finish_done(&tid, format!("Installed {name}")),
            Err(e) => tasks.finish_failed(&tid, format!("Failed: {e}")),
        }
    });
    Ok(Json(json!({"ok":true, "task": task})))
}

#[derive(Deserialize)]
struct PluginNameReq { name: String }

async fn delete_plugin(State(s): State<AppState>, Json(r): Json<PluginNameReq>) -> Result<Json<serde_json::Value>, ApiError> {
    plugins::delete(&s.config().server_dir, &r.name).await.map_err(ApiError::server)?;
    Ok(Json(json!({"ok":true})))
}

#[derive(Deserialize)]
struct ToggleReq { name: String, enable: bool }

async fn toggle_plugin(State(s): State<AppState>, Json(r): Json<ToggleReq>) -> Result<Json<serde_json::Value>, ApiError> {
    plugins::toggle(&s.config().server_dir, &r.name, r.enable).await.map_err(ApiError::server)?;
    Ok(Json(json!({"ok":true})))
}

#[derive(Deserialize)]
struct PluginCfgQuery { plugin: String, file: Option<String> }

async fn list_plugin_configs(State(s): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let dir = s.config().server_dir.join("plugins");
    tokio::fs::create_dir_all(&dir).await.ok();
    let mut out = Vec::new();
    let mut rd = tokio::fs::read_dir(&dir).await.map_err(ApiError::server)?;
    while let Some(e) = rd.next_entry().await.map_err(ApiError::server)? {
        let m = e.metadata().await.map_err(ApiError::server)?;
        if !m.is_dir() { continue; }
        let name = e.file_name().to_string_lossy().to_string();
        let mut configs = Vec::new();
        if let Ok(mut sub) = tokio::fs::read_dir(e.path()).await {
            while let Ok(Some(f)) = sub.next_entry().await {
                let fname = f.file_name().to_string_lossy().to_string();
                if fname.ends_with(".yml") || fname.ends_with(".yaml") || fname.ends_with(".json") || fname.ends_with(".conf") || fname.ends_with(".toml") || fname.ends_with(".properties") {
                    configs.push(fname);
                }
            }
        }
        out.push(json!({"plugin": name, "configs": configs}));
    }
    Ok(Json(json!({ "plugins": out })))
}

async fn get_plugin_config(State(s): State<AppState>, Query(q): Query<PluginCfgQuery>) -> Result<Json<serde_json::Value>, ApiError> {
    let file = q.file.ok_or_else(|| ApiError::bad("file required"))?;
    let safe_plugin = std::path::Path::new(&q.plugin).file_name().ok_or_else(|| ApiError::bad("bad plugin"))?.to_string_lossy().to_string();
    let safe_file = std::path::Path::new(&file).file_name().ok_or_else(|| ApiError::bad("bad file"))?.to_string_lossy().to_string();
    let p = s.config().server_dir.join("plugins").join(safe_plugin).join(safe_file);
    let text = tokio::fs::read_to_string(&p).await.map_err(ApiError::server)?;
    Ok(Json(json!({ "content": text })))
}

#[derive(Deserialize)]
struct PutPluginCfg { plugin: String, file: String, content: String }

async fn put_plugin_config(State(s): State<AppState>, Json(r): Json<PutPluginCfg>) -> Result<Json<serde_json::Value>, ApiError> {
    let safe_plugin = std::path::Path::new(&r.plugin).file_name().ok_or_else(|| ApiError::bad("bad plugin"))?.to_string_lossy().to_string();
    let safe_file = std::path::Path::new(&r.file).file_name().ok_or_else(|| ApiError::bad("bad file"))?.to_string_lossy().to_string();
    let dir = s.config().server_dir.join("plugins").join(safe_plugin);
    tokio::fs::create_dir_all(&dir).await.ok();
    tokio::fs::write(dir.join(safe_file), r.content).await.map_err(ApiError::server)?;
    Ok(Json(json!({"ok":true})))
}

// ---------- Backups ----------

async fn list_backups() -> Result<Json<serde_json::Value>, ApiError> {
    let v = backup::list().await.map_err(ApiError::server)?;
    Ok(Json(json!({ "backups": v })))
}

async fn create_backup(State(s): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let cfg = s.config();
    let server_dir = cfg.server_dir.clone();
    let keep = cfg.backup_keep;
    let s_clone = s.clone();
    tokio::spawn(async move {
        s_clone.inner.console.push("system", "Manual backup starting...".into());
        match backup::create_world_backup(server_dir).await {
            Ok(n) => {
                s_clone.inner.console.push("system", format!("Backup created: {n}"));
                let _ = backup::prune(keep).await;
            }
            Err(e) => s_clone.inner.console.push("system", format!("Backup failed: {e}")),
        }
    });
    Ok(Json(json!({"ok":true,"started":true})))
}

async fn delete_backup(AxPath(name): AxPath<String>) -> Result<Json<serde_json::Value>, ApiError> {
    backup::delete(&name).await.map_err(ApiError::server)?;
    Ok(Json(json!({"ok":true})))
}

async fn download_backup(AxPath(name): AxPath<String>) -> Result<Response, ApiError> {
    let p = backup::backup_path(&name).map_err(ApiError::server)?;
    file_response(p, &name).await
}

// ---------- Region ----------

async fn list_dimensions(State(s): State<AppState>) -> Json<serde_json::Value> {
    let dirs = region::dimension_region_dirs(&s.config().server_dir);
    let names: Vec<_> = dirs.into_iter().map(|(d, p)| json!({"name": d, "path": p.to_string_lossy()})).collect();
    Json(json!({"dimensions": names}))
}

async fn list_region_files(State(s): State<AppState>, AxPath(dim): AxPath<String>) -> Result<Json<serde_json::Value>, ApiError> {
    let v = region::list_region_files(&s.config().server_dir, &dim).await.map_err(ApiError::server)?;
    Ok(Json(json!({"files": v})))
}

async fn read_chunks(State(s): State<AppState>, AxPath((dim, file)): AxPath<(String, String)>) -> Result<Json<serde_json::Value>, ApiError> {
    let path = region::region_file_path(&s.config().server_dir, &dim, &file).map_err(ApiError::server)?;
    let chunks = tokio::task::spawn_blocking(move || region::read_chunks(&path))
        .await.map_err(ApiError::server)?
        .map_err(ApiError::server)?;
    Ok(Json(json!({"chunks": chunks})))
}

#[derive(Deserialize)]
struct ClearChunkReq { x: u32, z: u32 }

async fn clear_chunk(State(s): State<AppState>, AxPath((dim, file)): AxPath<(String, String)>, Json(r): Json<ClearChunkReq>) -> Result<Json<serde_json::Value>, ApiError> {
    if !matches!(s.inner.server.status(), crate::server::ServerStatus::Stopped | crate::server::ServerStatus::Crashed) {
        return Err(ApiError::bad("stop the server before editing region files"));
    }
    let path = region::region_file_path(&s.config().server_dir, &dim, &file).map_err(ApiError::server)?;
    tokio::task::spawn_blocking(move || region::clear_chunk(&path, r.x, r.z))
        .await.map_err(ApiError::server)?
        .map_err(ApiError::server)?;
    Ok(Json(json!({"ok":true})))
}

// ---------- Metrics & Tasks ----------

async fn get_metrics(State(s): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({ "latest": s.inner.metrics.latest(), "server": s.inner.server.info() }))
}

async fn get_metrics_history(State(s): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({ "history": s.inner.metrics.history() }))
}

async fn list_tasks(State(s): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({ "tasks": s.inner.tasks.list() }))
}

async fn get_players(State(s): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({ "players": s.inner.players.list() }))
}

#[derive(Deserialize)]
struct KickReq { name: String, reason: Option<String> }

async fn kick_player(State(s): State<AppState>, Json(r): Json<KickReq>) -> Result<Json<serde_json::Value>, ApiError> {
    if !crate::players::is_valid_name_pub(&r.name) { return Err(ApiError::bad("invalid name")); }
    let cmd = match r.reason {
        Some(reason) if !reason.is_empty() => format!("kick {} {}", r.name, reason),
        _ => format!("kick {}", r.name),
    };
    s.inner.server.send_command(&cmd).await.map_err(ApiError::server)?;
    Ok(Json(json!({"ok":true})))
}

#[derive(Deserialize)]
struct MarketSearchQuery { q: String, source: Option<String> }

async fn marketplace_search(Query(q): Query<MarketSearchQuery>) -> Result<Json<serde_json::Value>, ApiError> {
    let source = q.source.as_deref().unwrap_or("all");
    let mut results: Vec<crate::marketplace::MarketItem> = Vec::new();
    let query = q.q.trim().to_string();
    if query.is_empty() { return Err(ApiError::bad("empty query")); }

    let do_modrinth = source == "all" || source == "modrinth";
    let do_hangar = source == "all" || source == "hangar";

    let (mr, hg) = tokio::join!(
        async {
            if do_modrinth { crate::marketplace::search_modrinth(&query).await.ok() } else { None }
        },
        async {
            if do_hangar { crate::marketplace::search_hangar(&query).await.ok() } else { None }
        }
    );
    if let Some(mut v) = mr { results.append(&mut v); }
    if let Some(mut v) = hg { results.append(&mut v); }
    results.sort_by(|a, b| b.downloads.cmp(&a.downloads));
    Ok(Json(json!({ "results": results })))
}

#[derive(Deserialize)]
struct MarketInstallReq { source: String, slug: String, replace: Option<bool> }

async fn marketplace_install(State(s): State<AppState>, Json(r): Json<MarketInstallReq>) -> Result<Json<serde_json::Value>, ApiError> {
    let (filename, url) = match r.source.as_str() {
        "modrinth" => crate::marketplace::install_modrinth(&r.slug).await.map_err(ApiError::server)?,
        "hangar" => crate::marketplace::install_hangar(&r.slug).await.map_err(ApiError::server)?,
        _ => return Err(ApiError::bad("unknown source")),
    };
    let plugins_dir = s.config().server_dir.join("plugins");
    tokio::fs::create_dir_all(&plugins_dir).await.ok();
    let target = plugins_dir.join(&filename);
    if target.exists() && !r.replace.unwrap_or(false) {
        return Err(ApiError::bad("plugin already exists; pass replace=true"));
    }
    let task = s.inner.tasks.create("install_plugin", &format!("Install {filename}"));
    let tid = task.id.clone();
    let tasks = s.inner.tasks.clone();
    let fname = filename.clone();
    tokio::spawn(async move {
        match crate::tasks::download_with_progress(&tasks, &tid, &url, &target).await {
            Ok(()) => tasks.finish_done(&tid, format!("Installed {fname}")),
            Err(e) => tasks.finish_failed(&tid, format!("Failed: {e}")),
        }
    });
    Ok(Json(json!({"ok": true, "task": task, "filename": filename })))
}

async fn setup_task_progress(State(s): State<AppState>, AxPath(id): AxPath<String>) -> Result<Json<serde_json::Value>, ApiError> {
    let t = s.inner.tasks.list().into_iter().find(|t| t.id == id)
        .ok_or_else(|| ApiError::status(StatusCode::NOT_FOUND, "task not found"))?;
    if t.kind != "download_jar" { return Err(ApiError::status(StatusCode::FORBIDDEN, "not allowed")); }
    Ok(Json(json!({ "task": t })))
}

// ---------- mcsapi (public) ----------

#[derive(Deserialize)]
struct McsViewQuery { #[serde(rename = "dealId")] deal_id: String }
#[derive(Deserialize)]
struct McsApproveQuery { name: String, #[serde(rename = "dealId")] deal_id: String }
#[derive(Deserialize)]
struct McsRejectQuery { name: String, #[serde(rename = "dealId")] deal_id: String, reason: Option<String> }
#[derive(Deserialize)]
struct McsCreateGet { title: String, body: Option<String>, parties: String, by: Option<String> }
#[derive(Deserialize)]
struct McsCreatePost { title: String, body: Option<String>, parties: Vec<String>, by: Option<String> }

async fn mcs_list(State(s): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "records": s.inner.deals.list() }))
}

async fn mcs_view(State(s): State<AppState>, Query(q): Query<McsViewQuery>) -> Result<Json<serde_json::Value>, ApiError> {
    let v = s.inner.deals.get(&q.deal_id).ok_or_else(|| ApiError::status(StatusCode::NOT_FOUND, "deal not found"))?;
    Ok(Json(json!({ "ok": true, "record": v })))
}

async fn mcs_approve(State(s): State<AppState>, Query(q): Query<McsApproveQuery>) -> Result<Json<serde_json::Value>, ApiError> {
    let v = s.inner.deals.approve(&q.deal_id, &q.name).await.map_err(ApiError::bad_str)?;
    Ok(Json(json!({ "ok": true, "record": v })))
}

async fn mcs_reject(State(s): State<AppState>, Query(q): Query<McsRejectQuery>) -> Result<Json<serde_json::Value>, ApiError> {
    let v = s.inner.deals.reject(&q.deal_id, &q.name, q.reason).await.map_err(ApiError::bad_str)?;
    Ok(Json(json!({ "ok": true, "record": v })))
}

async fn mcs_create_post(State(s): State<AppState>, Json(r): Json<McsCreatePost>) -> Result<Json<serde_json::Value>, ApiError> {
    let v = s.inner.deals.create(r.title, r.body.unwrap_or_default(), r.parties, r.by).await.map_err(ApiError::bad_str)?;
    Ok(Json(json!({ "ok": true, "record": v })))
}

async fn mcs_create_get(State(s): State<AppState>, Query(q): Query<McsCreateGet>) -> Result<Json<serde_json::Value>, ApiError> {
    let parties: Vec<String> = q.parties.split(',').map(|s| s.trim().to_string()).collect();
    let v = s.inner.deals.create(q.title, q.body.unwrap_or_default(), parties, q.by).await.map_err(ApiError::bad_str)?;
    Ok(Json(json!({ "ok": true, "record": v })))
}

// ---------- Admin deals ----------

async fn admin_list_deals(State(s): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({ "deals": s.inner.deals.list() }))
}

async fn admin_delete_deal(State(s): State<AppState>, AxPath(id): AxPath<String>) -> Result<Json<serde_json::Value>, ApiError> {
    s.inner.deals.delete(&id).await.map_err(ApiError::server)?;
    Ok(Json(json!({"ok":true})))
}

// ---------- Errors ----------

pub struct ApiError { status: StatusCode, msg: String }

impl ApiError {
    fn bad(msg: impl Into<String>) -> Self { Self { status: StatusCode::BAD_REQUEST, msg: msg.into() } }
    fn bad_str(e: impl std::fmt::Display) -> Self { Self { status: StatusCode::BAD_REQUEST, msg: e.to_string() } }
    fn server(e: impl std::fmt::Display) -> Self { Self { status: StatusCode::INTERNAL_SERVER_ERROR, msg: e.to_string() } }
    fn status(s: StatusCode, msg: impl Into<String>) -> Self { Self { status: s, msg: msg.into() } }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({"error": self.msg}))).into_response()
    }
}
