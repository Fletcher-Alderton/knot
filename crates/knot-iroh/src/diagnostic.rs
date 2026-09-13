//! Isolated diagnostics: no sync state, board access, or trust mutation.
use super::{
    DIAGNOSTIC_ACK, DIAGNOSTIC_ALPN, DIAGNOSTIC_HELLO, DiagnosticResult, TransportError,
    write_frame,
};
use iroh::{Endpoint, EndpointAddr, endpoint::Connection};
use tokio::{
    io::AsyncReadExt,
    time::{Duration, Instant, timeout, timeout_at},
};

const MAX_FRAME: usize = 128;

async fn read_frame(recv: &mut iroh::endpoint::RecvStream) -> Result<Vec<u8>, TransportError> {
    let len = recv
        .read_u32()
        .await
        .map_err(|_| TransportError::Disconnected)? as usize;
    if len > MAX_FRAME {
        return Err(TransportError::MessageTooLarge);
    }
    let mut bytes = vec![0; len];
    recv.read_exact(&mut bytes)
        .await
        .map_err(|_| TransportError::Disconnected)?;
    Ok(bytes)
}

fn timed_out() -> TransportError {
    TransportError::Malformed("diagnostic timed out".into())
}

pub(super) async fn run(
    addr: EndpointAddr,
    limit: Duration,
) -> Result<DiagnosticResult, TransportError> {
    if limit.is_zero() {
        return Err(timed_out());
    }
    let deadline = Instant::now() + limit.min(Duration::from_secs(120));
    // No IP transports means direct connectivity cannot win a relay diagnostic.
    let endpoint = timeout_at(
        deadline,
        Endpoint::builder(iroh::endpoint::presets::N0)
            .clear_ip_transports()
            .alpns(vec![DIAGNOSTIC_ALPN.to_vec()])
            .bind(),
    )
    .await
    .map_err(|_| timed_out())?
    .map_err(|e| TransportError::Malformed(format!("diagnostic bind failed: {e}")))?;
    run_with_endpoint(endpoint, addr, deadline, true).await
}

async fn run_with_endpoint(
    endpoint: Endpoint,
    addr: EndpointAddr,
    deadline: Instant,
    relay_only: bool,
) -> Result<DiagnosticResult, TransportError> {
    let result = timeout_at(deadline, exchange(&endpoint, addr, relay_only))
        .await
        .map_err(|_| timed_out());
    // Cleanup also runs for timeout and malformed responses, not just success.
    endpoint.close().await;
    result?
}

async fn exchange(
    endpoint: &Endpoint,
    addr: EndpointAddr,
    relay_only: bool,
) -> Result<DiagnosticResult, TransportError> {
    let conn = endpoint.connect(addr, DIAGNOSTIC_ALPN).await?;
    let (mut send, mut recv) = conn
        .open_bi()
        .await
        .map_err(|_| TransportError::Disconnected)?;
    // A fresh endpoint identity supplies a fresh nonce for every diagnostic.
    let id = endpoint.id();
    let nonce = &id.as_bytes()[..16];
    let mut hello = DIAGNOSTIC_HELLO.to_vec();
    hello.extend_from_slice(nonce);
    write_frame(&mut send, &hello).await?;
    let ack = read_frame(&mut recv).await?;
    let mut expected = DIAGNOSTIC_ACK.to_vec();
    expected.extend_from_slice(nonce);
    if ack != expected {
        return Err(TransportError::Malformed("invalid diagnostic ACK".into()));
    }
    let path = conn
        .paths()
        .iter()
        .find(|p| p.is_selected())
        .map(|p| {
            if p.is_relay() {
                "relay"
            } else if p.is_ip() {
                "direct"
            } else {
                "unknown"
            }
        })
        .unwrap_or("unknown");
    if relay_only && path != "relay" {
        return Err(TransportError::Malformed(format!(
            "relay-only diagnostic has unverified route: {path}"
        )));
    }
    Ok(DiagnosticResult {
        peer: conn.remote_id(),
        hello_acknowledged: true,
        relay_only_requested: relay_only,
        path: path.into(),
    })
}

pub(super) async fn serve(conn: &Connection) {
    let result = timeout(Duration::from_secs(5), async {
        let (mut send, mut recv) = conn
            .accept_bi()
            .await
            .map_err(|_| TransportError::Disconnected)?;
        let hello = read_frame(&mut recv).await?;
        if hello.len() != DIAGNOSTIC_HELLO.len() + 16 || !hello.starts_with(DIAGNOSTIC_HELLO) {
            return Err(TransportError::Malformed("invalid diagnostic HELLO".into()));
        }
        let mut ack = DIAGNOSTIC_ACK.to_vec();
        ack.extend_from_slice(&hello[DIAGNOSTIC_HELLO.len()..]);
        write_frame(&mut send, &ack).await?;
        // Wait for receipt before releasing the final server-side connection handle.
        send.stopped()
            .await
            .map_err(|_| TransportError::Disconnected)?;
        Ok::<(), TransportError>(())
    })
    .await;
    if !matches!(result, Ok(Ok(()))) {
        conn.close(0u32.into(), b"diagnostic failure");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IrohTransport, diagnose_remote_peer_json};

    async fn local_endpoint() -> Endpoint {
        Endpoint::builder(iroh::endpoint::presets::N0DisableRelay)
            .alpns(vec![DIAGNOSTIC_ALPN.to_vec()])
            .bind()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn rejects_malformed_address_before_network() {
        for input in ["not-json", "{}", r#"{"id":"invalid"}"#] {
            let error = diagnose_remote_peer_json(input, Duration::from_secs(1))
                .await
                .unwrap_err();
            assert!(error.to_string().contains("invalid EndpointAddr JSON"));
        }
    }

    #[tokio::test]
    async fn local_diagnostic_acknowledges_nonce_without_sync_trust() {
        let host = IrohTransport::bind_local().await.unwrap();
        let client = local_endpoint().await;
        assert_ne!(host.endpoint_id(), client.id());
        let result = run_with_endpoint(
            client,
            host.endpoint().addr(),
            Instant::now() + Duration::from_secs(5),
            false,
        )
        .await
        .unwrap();
        assert_eq!(result.peer, host.endpoint_id());
        assert!(result.hello_acknowledged);
        assert!(!result.relay_only_requested);
        assert_eq!(result.path, "direct");
        assert!(
            host.inner.peers.lock().await.is_empty(),
            "diagnostics must not publish trusted sync peers"
        );
        host.endpoint().close().await;
    }

    #[tokio::test]
    async fn rejects_wrong_nonce_ack_and_closes_client() {
        let host = local_endpoint().await;
        let host_task = host.clone();
        let task = tokio::spawn(async move {
            let conn = host_task.accept().await.unwrap().await.unwrap();
            let (mut send, mut recv) = conn.accept_bi().await.unwrap();
            read_frame(&mut recv).await.unwrap();
            let mut ack = DIAGNOSTIC_ACK.to_vec();
            ack.extend_from_slice(&[0; 16]);
            write_frame(&mut send, &ack).await.unwrap();
            let _ = send.stopped().await;
            conn.closed().await;
        });
        let error = run_with_endpoint(
            local_endpoint().await,
            host.addr(),
            Instant::now() + Duration::from_secs(5),
            false,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("invalid diagnostic ACK"));
        timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap();
        host.close().await;
    }

    #[tokio::test]
    async fn stalled_peer_times_out_and_closes_client() {
        let host = local_endpoint().await;
        let host_task = host.clone();
        let task = tokio::spawn(async move {
            let conn = host_task.accept().await.unwrap().await.unwrap();
            let (_send, mut recv) = conn.accept_bi().await.unwrap();
            read_frame(&mut recv).await.unwrap();
            conn.closed().await;
        });
        let error = run_with_endpoint(
            local_endpoint().await,
            host.addr(),
            Instant::now() + Duration::from_secs(1),
            false,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("diagnostic timed out"));
        timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap();
        host.close().await;
    }

    #[tokio::test]
    async fn oversized_diagnostic_frame_is_rejected() {
        let host = IrohTransport::bind_local().await.unwrap();
        let client = local_endpoint().await;
        let conn = client
            .connect(host.endpoint().addr(), DIAGNOSTIC_ALPN)
            .await
            .unwrap();
        let (mut send, mut recv) = conn.open_bi().await.unwrap();
        send.write_all(&((MAX_FRAME + 1) as u32).to_be_bytes())
            .await
            .unwrap();
        send.finish().unwrap();
        assert!(
            timeout(Duration::from_secs(2), read_frame(&mut recv))
                .await
                .unwrap()
                .is_err()
        );
        client.close().await;
        host.endpoint().close().await;
    }

    #[tokio::test]
    #[ignore = "requires internet access to the Iroh relay network"]
    async fn internet_diagnostic_proves_relay_only_path() {
        let host = IrohTransport::bind().await.unwrap();
        timeout(Duration::from_secs(30), host.endpoint().online())
            .await
            .expect("host relay readiness");
        let addr = serde_json::to_string(&host.endpoint().addr()).unwrap();
        let result = diagnose_remote_peer_json(&addr, Duration::from_secs(30)).await;
        host.endpoint().close().await;
        let result = result.unwrap();
        assert!(result.hello_acknowledged);
        assert!(result.relay_only_requested);
        assert_eq!(result.path, "relay");
        eprintln!(
            "relay-only HELLO acknowledged by {} via {}",
            result.peer, result.path
        );
    }
}
