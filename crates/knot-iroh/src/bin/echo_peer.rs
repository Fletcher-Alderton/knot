use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use iroh::EndpointAddr;
use knot_iroh::{ConnectedIrohTransport, IrohTransport, secret_key_from_bytes};
use knot_sync::PeerTransport;

fn find_config_key() -> Option<iroh::SecretKey> {
    let home = env::var_os("HOME").map(PathBuf::from);
    let appdata = env::var_os("LOCALAPPDATA").or_else(|| env::var_os("APPDATA")).map(PathBuf::from);

    let candidates = [
        home.as_ref().map(|h| h.join(".config/knot/install.json")),
        appdata.as_ref().map(|a| a.join("Knot/install.json")),
    ];

    for path in candidates.into_iter().flatten() {
        if let Ok(bytes) = fs::read(&path) {
            if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if let Some(key_arr) = val.get("secret_key").and_then(|v| v.as_array()) {
                    let key_bytes: Vec<u8> = key_arr
                        .iter()
                        .filter_map(|x| x.as_u64().map(|n| n as u8))
                        .collect();
                    if let Ok(key) = secret_key_from_bytes(&key_bytes) {
                        println!("Loaded existing Knot secret key from: {}", path.display());
                        return Some(key);
                    }
                }
            }
        }
    }
    None
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Knot Iroh Diagnostic Tool ===");

    let args: Vec<String> = env::args().collect();
    let maybe_target = args.get(1);

    // Bind transport
    let transport = if let Some(key) = find_config_key() {
        IrohTransport::bind_with_secret_key(key).await?
    } else {
        println!("No install.json found, creating ephemeral identity...");
        IrohTransport::bind().await?
    };

    println!("Local Endpoint ID: {}", transport.endpoint_id());
    println!("Discovering addresses and connecting to relay...");
    tokio::time::sleep(Duration::from_secs(3)).await;

    let addr = transport.endpoint().addr();
    let addr_json = serde_json::to_string(&addr)?;
    println!("\n================ DEVICE ADDRESS (COPY THIS) ================");
    println!("{}", addr_json);
    println!("============================================================\n");

    if let Some(target_str) = maybe_target {
        println!("Target address provided, attempting outbound dial to peer...");
        let target_addr: EndpointAddr = serde_json::from_str(target_str)?;
        println!("Dialing: {}", target_addr.id);

        let t_clone = transport.clone();
        tokio::spawn(async move {
            match tokio::time::timeout(
                Duration::from_secs(15),
                ConnectedIrohTransport::connect(t_clone.clone(), target_addr),
            )
            .await
            {
                Ok(Ok(mut wire)) => {
                    println!("\n>>> Outbound connection SUCCESSFUL to: {}!", wire.peer());
                    let hello = knot_sync::WireMessage::new(knot_sync::Message::Hello(knot_sync::Hello {
                        device_id: "diagnostic-client".into(),
                        endpoint_id: t_clone.endpoint_id().to_string(),
                    }));
                    println!("Sending test HELLO message...");
                    let _ = wire.send(hello).await;
                    tokio::spawn(async move {
                        while let Ok(msg) = wire.recv().await {
                            println!("Outbound wire received message: {:?}", msg);
                        }
                    });
                }
                Ok(Err(e)) => println!("\n>>> Outbound connection error: {:?}", e),
                Err(_) => println!("\n>>> Outbound connection timed out after 15s."),
            }
        });
    }

    println!("Listening for incoming peer connections (Press Ctrl+C to stop)...");
    loop {
        match transport.accept_connected().await {
            Ok(mut wire) => {
                let peer_id = wire.peer();
                println!("\n>>> INCOMING CONNECTION ACCEPTED from peer: {}!", peer_id);
                tokio::spawn(async move {
                    while let Ok(msg) = wire.recv().await {
                        println!("Received wire message from {}: {:?}", peer_id, msg);
                    }
                    println!("Peer {} disconnected.", peer_id);
                });
            }
            Err(e) => {
                println!("Accept error: {:?}", e);
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }
}
