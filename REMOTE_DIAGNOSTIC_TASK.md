# Remote laptop/device relay diagnostic

The diagnostic proves bounded transport/application reachability only. Sync trust, board authorization, pairing, and board convergence remain separate.

A host must remain running and foregrounded. A suspended mobile app cannot answer diagnostics.

## Start host

```sh
cargo run -p knot-iroh --bin knot-peer-echo
```

Copy the complete endpoint JSON printed by the command. Keep the host process alive. The default identity is fresh and ephemeral.

Persisted app identity is explicit and should be used only while the Knot app is stopped:

```sh
KNOT_USE_PERSISTED_IDENTITY=1 cargo run -p knot-iroh --bin knot-peer-echo
```

`EXIT_AFTER_ADDR=1` closes the endpoint after printing. Its address is not a live test target.

## Dial from CLI

```sh
cargo run -p knot-iroh --bin knot-peer-echo -- --dial '<ENDPOINT_ADDRESS_JSON>'
```

Success returns JSON containing:

```json
{
  "hello_acknowledged": true,
  "relay_only_requested": true,
  "path": "relay"
}
```

The client parses the complete `EndpointAddr` JSON. Do not remove direct addresses manually; `diagnose_remote_peer` calls `clear_ip_transports()` so only relay transport can be selected. It rejects success unless Iroh reports the selected path as relay.

## Dial from Knot

1. Open **Settings → Sync → Device details**.
2. Copy the complete host JSON into **Remote endpoint address JSON**.
3. Tap **Diagnostic dial**.
4. Require `hello_acknowledged: true`, `relay_only_requested: true`, and `path: relay`.

The Tauri command calls:

```rust
knot_iroh::diagnose_remote_peer_json(
    &json,
    std::time::Duration::from_secs(20),
).await
```

## Protocol properties

- Dedicated ALPN: `knot-diagnostic/1`.
- Independent ephemeral client endpoint.
- Nonce-bound HELLO/ACK.
- Small bounded frames.
- One bounded client deadline covering bind and exchange.
- Bounded inbound handler.
- Endpoint cleanup on success, malformed reply, transport failure, and timeout.
- No board access, trust mutation, or sync framing changes.

## Automated tests

Deterministic local tests run normally:

```sh
cargo test -p knot-iroh
```

The real relay-network test is ignored by default:

```sh
cargo test -p knot-iroh internet_diagnostic_proves_relay_only_path \
  -- --ignored --nocapture
```

Ordinary CI does not depend on external relay availability. For a manual report, record OS, network type, firewall/VPN state, complete endpoint JSON, result JSON, and timeout/error output. Never record private identity material.
