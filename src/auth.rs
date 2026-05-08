use crate::state::AppState;
use argon2::{password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString}, Argon2};
use axum::{
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};
use rand::RngCore;

pub fn hash_password(pw: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default().hash_password(pw.as_bytes(), &salt).map_err(|e| anyhow::anyhow!(e))?.to_string();
    Ok(hash)
}

pub fn verify_password(pw: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(h) => Argon2::default().verify_password(pw.as_bytes(), &h).is_ok(),
        Err(_) => false,
    }
}

pub fn new_session_token() -> String {
    let mut b = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut b);
    hex(&b)
}

fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b { s.push_str(&format!("{:02x}", x)); }
    s
}

pub fn extract_session(req_headers: &axum::http::HeaderMap) -> Option<String> {
    let cookie = req_headers.get("cookie")?.to_str().ok()?;
    for part in cookie.split(';') {
        let part = part.trim();
        if let Some(v) = part.strip_prefix("cmc_session=") {
            return Some(v.to_string());
        }
    }
    None
}

pub async fn require_auth(
    State(state): State<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = req.uri().path().to_string();
    let cfg = state.config();
    let public = path.starts_with("/api/setup")
        || path == "/api/login"
        || path == "/api/status-public"
        || path == "/control"
        || path == "/login"
        || path == "/setup"
        || path == "/"
        || path.starts_with("/static/");
    if public {
        return Ok(next.run(req).await);
    }
    if !cfg.setup_complete {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let token = extract_session(req.headers());
    let ok = match token {
        Some(t) => state.inner.sessions.read().contains(&t),
        None => false,
    };
    if !ok {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(req).await)
}
