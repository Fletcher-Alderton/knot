//! Live diagnostic host, or a one-shot acknowledged relay-only client.
use knot_iroh::{DIAGNOSTIC_ALPN, IrohTransport, diagnose_remote_peer_json, secret_key_from_bytes};
use std::{env, fs, path::PathBuf, time::Duration};

fn find_config_key() -> Option<iroh::SecretKey> {
    let home = env::var_os("HOME").map(PathBuf::from);
    let appdata = env::var_os("LOCALAPPDATA")
        .or_else(|| env::var_os("APPDATA"))
        .map(PathBuf::from);
    let candidates = [
        env::var_os("KNOT_CONFIG_PATH").map(PathBuf::from),
        home.as_ref().map(|h| h.join(".config/knot/install.json")),
        appdata.as_ref().map(|a| a.join("Knot/install.json")),
    ];
    for path in candidates.into_iter().flatten() {
        let Ok(bytes) = fs::read(&path) else { continue };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        let Some(array) = value.get("secret_key").and_then(|v| v.as_array()) else {
            continue;
        };
        let key_bytes: Option<Vec<u8>> = array
            .iter()
            .map(|v| v.as_u64().and_then(|n| u8::try_from(n).ok()))
            .collect();
        if let Some(bytes) = key_bytes
            && let Ok(key) = secret_key_from_bytes(&bytes)
        {
            return Some(key);
        }
    }
    None
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let target = match args.as_slice() {
        [] => None,
        [flag] if flag == "--help" || flag == "-h" => {
            println!(
                "Usage: knot-peer-echo [--dial ADDRESS_JSON]\n\nWithout arguments, serves knot-diagnostic/1 until Ctrl+C.\n--dial validates a live peer with an acknowledged relay-only HELLO and exits.\nIdentities are ephemeral. KNOT_USE_PERSISTED_IDENTITY opts the host into an existing app identity; never run both concurrently.\nEXIT_AFTER_ADDR prints an address then closes it; that address is NOT a live test target."
            );
            return Ok(());
        }
        [flag, address] if flag == "--dial" => Some(address),
        [address] if !address.starts_with('-') => Some(address),
        _ => return Err("use knot-peer-echo --help for usage".into()),
    };
    if let Some(address) = target {
        let result = diagnose_remote_peer_json(address, Duration::from_secs(30)).await?;
        println!(
            "{}",
            serde_json::json!({
                "peer": result.peer.to_string(),
                "hello_acknowledged": result.hello_acknowledged,
                "relay_only_requested": result.relay_only_requested,
                "path": result.path,
            })
        );
        return Ok(());
    }

    // Diagnostic host has no sync ALPN and cannot access or authorize a board.
    let mut builder =
        iroh::Endpoint::builder(iroh::endpoint::presets::N0).alpns(vec![DIAGNOSTIC_ALPN.to_vec()]);
    if env::var_os("KNOT_USE_PERSISTED_IDENTITY").is_some() {
        let key = find_config_key().ok_or("persisted identity requested but no valid key found")?;
        eprintln!("Warning: stop the Knot app before reusing its persisted identity.");
        builder = builder.secret_key(key);
    }
    let endpoint = tokio::time::timeout(Duration::from_secs(10), builder.bind()).await??;
    let host = IrohTransport::from_endpoint(endpoint);
    if tokio::time::timeout(Duration::from_secs(30), host.endpoint().online())
        .await
        .is_err()
    {
        host.endpoint().close().await;
        return Err("relay readiness timed out".into());
    }
    println!("=== Knot diagnostic host (keep this process running) ===");
    println!("{}", serde_json::to_string(&host.endpoint().addr())?);
    if env::var_os("EXIT_AFTER_ADDR").is_some() {
        eprintln!("EXIT_AFTER_ADDR: closing endpoint; printed address will not remain reachable.");
    } else {
        eprintln!("Serving acknowledged diagnostics; Ctrl+C stops host.");
        let result = tokio::signal::ctrl_c().await;
        host.endpoint().close().await;
        result?;
        return Ok(());
    }
    host.endpoint().close().await;
    Ok(())
}
