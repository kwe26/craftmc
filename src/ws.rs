use crate::state::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use serde_json::json;
use std::time::Duration;

pub async fn console_ws(
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> Response {
    // Authentication is enforced by the auth middleware (cookie-based) before this runs.
    ws.on_upgrade(move |socket| handle_socket(state, socket))
}

async fn handle_socket(state: AppState, mut socket: WebSocket) {
    // 1. Send a snapshot first (last ~500 lines so we don't hang the browser).
    let snap = state.inner.console.snapshot(500);
    let init = json!({ "type": "snapshot", "lines": snap });
    if socket.send(Message::Text(init.to_string())).await.is_err() { return; }

    let mut rx = state.inner.console.subscribe();
    // Throttling: batch up to 80 lines or 80 ms before flushing.
    let mut batch: Vec<crate::console::ConsoleLine> = Vec::with_capacity(80);
    let mut interval = tokio::time::interval(Duration::from_millis(80));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            biased;
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(t))) => {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
                            if v.get("type").and_then(|x| x.as_str()) == Some("cmd") {
                                if let Some(cmd) = v.get("cmd").and_then(|x| x.as_str()) {
                                    if let Err(e) = state.inner.server.send_command(cmd).await {
                                        let _ = socket.send(Message::Text(json!({"type":"error","msg":e.to_string()}).to_string())).await;
                                    }
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Ping(p))) => { let _ = socket.send(Message::Pong(p)).await; }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
            line = rx.recv() => {
                match line {
                    Ok(l) => {
                        batch.push(l);
                        if batch.len() >= 80 {
                            if !flush(&mut socket, &mut batch).await { break; }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        let _ = socket.send(Message::Text(json!({"type":"lagged","skipped":n}).to_string())).await;
                    }
                    Err(_) => break,
                }
            }
            _ = interval.tick() => {
                if !batch.is_empty() {
                    if !flush(&mut socket, &mut batch).await { break; }
                }
            }
        }
    }
}

async fn flush(socket: &mut WebSocket, batch: &mut Vec<crate::console::ConsoleLine>) -> bool {
    let payload = json!({ "type": "lines", "lines": batch });
    let ok = socket.send(Message::Text(payload.to_string())).await.is_ok();
    batch.clear();
    ok
}

pub async fn tasks_ws(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| handle_tasks(state, socket))
}

async fn handle_tasks(state: AppState, mut socket: WebSocket) {
    let task_snap = state.inner.tasks.list();
    let player_snap = state.inner.players.list();
    if socket.send(Message::Text(json!({"type":"snapshot","tasks":task_snap, "players": player_snap}).to_string())).await.is_err() { return; }
    let mut task_rx = state.inner.tasks.subscribe();
    let mut player_rx = state.inner.players.subscribe();
    let mut metrics_tick = tokio::time::interval(Duration::from_millis(2000));
    metrics_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            t = task_rx.recv() => {
                match t {
                    Ok(t) => { if socket.send(Message::Text(json!({"type":"task","task":t}).to_string())).await.is_err() { break; } }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
            p = player_rx.recv() => {
                match p {
                    Ok(p) => { if socket.send(Message::Text(json!({"type":"players","players":p}).to_string())).await.is_err() { break; } }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
            _ = metrics_tick.tick() => {
                if let Some(m) = state.inner.metrics.latest() {
                    let info = state.inner.server.info();
                    let payload = json!({"type":"metrics","metrics": m, "server": info});
                    if socket.send(Message::Text(payload.to_string())).await.is_err() { break; }
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Ping(p))) => { let _ = socket.send(Message::Pong(p)).await; }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }
}
