//! Outbound protocol adapter. One socket, one outstanding heartbeat, no command handler.
#[cfg(test)]
#[path = "network_tests.rs"]
mod tests;
use crate::{
    config::Configuration,
    lifecycle::{NoticeMemory, Participation, Phase, diagnostic_text},
};
use bootleg_core::{
    api::{ApiError, ErrorResponse},
    protocol::*,
    *,
};
use futures_util::{SinkExt, StreamExt};
use std::time::{Duration, Instant};
use tokio::{
    net::TcpStream,
    sync::watch,
    time::{sleep, timeout},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async_with_config,
    tungstenite::{self, Message, client::IntoClientRequest, protocol::WebSocketConfig},
};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
enum Failure {
    Transport,
    Server(ApiError),
    Stop,
}
fn report(error: &ApiError) {
    eprintln!(
        "smuggler: {:?}: {} (request {})",
        error.level,
        diagnostic_text(error.message.as_str()),
        diagnostic_text(error.request_id.as_str())
    );
}
fn registration(c: &Configuration) -> AgentFrame {
    AgentFrame::Register {
        protocol_version: PROTOCOL_VERSION,
        project_id: c.project.clone(),
        container_id: c.container.clone(),
        display_name: c.name.clone(),
        build_commit: c.baseline.commit.clone(),
        build_dirty: c.baseline.dirty,
        platform: Platform {
            os: std::env::consts::OS.parse().unwrap(),
            arch: std::env::consts::ARCH.parse().unwrap(),
        },
        agent_version: env!("CARGO_PKG_VERSION").parse().unwrap(),
        runtime: Some(RuntimeMetadata {
            pod_uid: c.runtime.pod_uid.clone(),
            pod_name: c.runtime.pod_name.clone(),
            namespace: c.runtime.namespace.clone(),
            container_name: c.runtime.container_name.clone(),
            cluster_name: c.runtime.cluster_name.clone(),
            runtime_hint: None,
        }),
        application_state: ApplicationState::Running,
    }
}
async fn send(socket: &mut Socket, frame: AgentFrame) -> Result<(), Failure> {
    let text = encode_agent_frame(&frame).map_err(|_| Failure::Transport)?;
    timeout(
        Duration::from_secs(2),
        socket.send(Message::Text(text.into())),
    )
    .await
    .map_err(|_| Failure::Transport)?
    .map_err(|_| Failure::Transport)
}
async fn receive(socket: &mut Socket) -> Result<ServerFrame, Failure> {
    loop {
        match socket.next().await {
            Some(Ok(Message::Text(text))) => {
                return match decode_server_frame(&text).map_err(|_| Failure::Transport)? {
                    ServerFrame::Error { error } => Err(Failure::Server(error)),
                    other => Ok(other),
                };
            }
            Some(Ok(Message::Ping(_))) => {
                socket.flush().await.map_err(|_| Failure::Transport)?;
            }
            Some(Ok(Message::Pong(_))) => {}
            _ => return Err(Failure::Transport),
        }
    }
}
async fn session(
    socket: &mut Socket,
    c: &Configuration,
    policy: &mut Participation,
    notices: &mut NoticeMemory,
) -> Result<(), Failure> {
    let epoch = Instant::now();
    policy
        .begin(epoch.elapsed())
        .map_err(|_| Failure::Transport)?;
    send(socket, registration(c)).await?;
    let registered = timeout(
        Duration::from_secs(5).saturating_sub(epoch.elapsed()),
        receive(socket),
    )
    .await
    .map_err(|_| Failure::Transport)??;
    match registered {
        ServerFrame::Registered {
            smuggler_session_id,
            heartbeat_interval_seconds,
            lease_timeout_seconds,
            notices: received,
            ..
        } => {
            policy
                .registered(
                    smuggler_session_id,
                    epoch.elapsed(),
                    heartbeat_interval_seconds,
                    lease_timeout_seconds,
                )
                .map_err(|_| Failure::Transport)?;
            for notice in received.as_slice() {
                if notices.first_seen(notice.id.as_str()) {
                    eprintln!(
                        "smuggler: {:?}: {}",
                        notice.level,
                        diagnostic_text(notice.message.as_str())
                    );
                }
            }
        }
        _ => return Err(Failure::Transport),
    }
    loop {
        // Listen while idle too: revocation/close must not wait for the next heartbeat.
        tokio::select! {
            _ = sleep(Duration::from_secs(15)) => {},
            incoming = receive(socket) => { incoming?; return Err(Failure::Transport); },
        }
        let id = policy
            .heartbeat(epoch.elapsed())
            .map_err(|_| Failure::Transport)?;
        let sent_at = Instant::now();
        send(
            socket,
            AgentFrame::Heartbeat {
                smuggler_session_id: id,
                application_state: ApplicationState::Running,
            },
        )
        .await?;
        match timeout(
            Duration::from_secs(15).saturating_sub(sent_at.elapsed()),
            receive(socket),
        )
        .await
        .map_err(|_| Failure::Transport)??
        {
            ServerFrame::HeartbeatAck {
                smuggler_session_id,
                ..
            } => policy
                .acknowledged(&smuggler_session_id, epoch.elapsed())
                .map_err(|_| Failure::Transport)?,
            _ => return Err(Failure::Transport),
        }
    }
}
async fn connect(c: &Configuration) -> Result<Socket, Failure> {
    let mut request = c
        .endpoint
        .as_str()
        .into_client_request()
        .map_err(|_| Failure::Transport)?;
    let headers = request.headers_mut();
    let mut authorization = format!("Bearer {}", c.credential.expose_secret())
        .parse::<tungstenite::http::HeaderValue>()
        .map_err(|_| Failure::Transport)?;
    authorization.set_sensitive(true);
    headers.insert("Authorization", authorization);
    for (key, value) in [
        ("Bootleg-Client", "smuggler"),
        ("Bootleg-Client-Version", env!("CARGO_PKG_VERSION")),
        ("Bootleg-Protocol-Version", "1"),
    ] {
        headers.insert(
            tungstenite::http::HeaderName::from_bytes(key.as_bytes()).unwrap(),
            value.parse().unwrap(),
        );
    }
    let ws_config = WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME_BYTES))
        .max_frame_size(Some(MAX_FRAME_BYTES))
        .max_write_buffer_size(65536)
        .write_buffer_size(0);
    match timeout(
        Duration::from_secs(5),
        connect_async_with_config(request, Some(ws_config), false),
    )
    .await
    {
        Ok(Ok((socket, _))) => Ok(socket),
        Ok(Err(tungstenite::Error::Http(response))) => {
            if let Some(body) = response
                .body()
                .as_ref()
                .filter(|b| b.len() <= MAX_FRAME_BYTES)
                && let Ok(envelope) = serde_json::from_slice::<ErrorResponse>(body)
            {
                return Err(Failure::Server(envelope.error));
            }
            // Even without a valid envelope, never hot-loop rejected credentials.
            if matches!(response.status().as_u16(), 400 | 401 | 403 | 404) {
                eprintln!("smuggler: control plane rejected participation");
                return Err(Failure::Stop);
            }
            Err(Failure::Transport)
        }
        _ => Err(Failure::Transport),
    }
}
pub async fn run(c: Configuration, mut shutdown: watch::Receiver<bool>) {
    let mut policy = Participation::new(c.container.clone());
    let mut notices = NoticeMemory::default();
    loop {
        if *shutdown.borrow() {
            return;
        }
        let connection = tokio::select! {
            _ = shutdown.changed() => return,
            result = connect(&c) => result,
        };
        let outcome = match connection {
            Ok(mut socket) => {
                let result = tokio::select! {
                    _ = shutdown.changed() => Err(Failure::Stop),
                    result = session(&mut socket, &c, &mut policy, &mut notices) => result,
                };
                let _ = timeout(Duration::from_millis(200), socket.close(None)).await;
                result
            }
            Err(error) => Err(error),
        };
        let mut minimum = 0;
        match outcome {
            Err(Failure::Stop) => return,
            Err(Failure::Server(error)) => {
                report(&error);
                minimum = error.retry_after_seconds.unwrap_or(0);
                policy.failure(error.code);
                if policy.phase == Phase::Disabled {
                    return;
                }
            }
            _ => {
                eprintln!("smuggler: control plane unavailable; reconnecting");
                policy.disconnected();
            }
        }
        let delay = policy.retry_delay(rand::random::<f64>(), minimum);
        tokio::select! { _ = shutdown.changed() => return, _ = sleep(delay) => {} }
    }
}
