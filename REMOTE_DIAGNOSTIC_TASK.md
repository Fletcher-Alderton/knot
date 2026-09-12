# Remote Laptop Diagnostic Task

Instructions for AI agent on other laptop:

## 1. Environment & Firewall Check
Run these checks and write results to `diagnostic_report.md`:
- Check OS and IP:
  - macOS/Linux: `hostname && uname -a && ifconfig` (or `ip a`)
- Check firewall status:
  - macOS: `/usr/libexec/ApplicationFirewall/socketfilterfw --getglobalstate`
  - Linux: `sudo ufw status` or `sudo iptables -L -n -v` (if applicable)

## 2. Pull & Build Diagnostic Tool
```bash
git pull origin main
cargo check -p knot-iroh --bin knot-peer-echo
```

## 3. Run Echo Peer & Extract Address
Run the diagnostic binary:
```bash
EXIT_AFTER_ADDR=1 cargo run -p knot-iroh --bin knot-peer-echo
```
Inspect the output JSON under `DEVICE ADDRESS (COPY THIS)`.
Does `addrs` contain a `"Relay"` entry (e.g. `{"Relay":"https://aps1-1.relay.n0.iroh.link./"}`)?
If not, why did relay connection / STUN fail? Check internet connectivity to `https://aps1-1.relay.n0.iroh.link./`.

## 4. Test Bidirectional Dial
Run `knot-peer-echo` attempting to dial this laptop's relay endpoint:
```bash
cargo run -p knot-iroh --bin knot-peer-echo -- '{"id":"a9ec6045065487a42a9ee79c9db9dcabc85af1fc52837496baa08d4b120b23e8","addrs":[{"Relay":"https://aps1-1.relay.n0.iroh.link./"}]}'
```
Wait 20 seconds. Record whether output says:
- `>>> Outbound connection SUCCESSFUL to: a9ec60450...` OR
- `>>> Outbound connection error / timed out`

## 5. Output Deliverable
Write findings to `diagnostic_report.md` at repo root with:
- OS & network interface IP(s)
- Firewall status
- Printed Device Address JSON
- Outbound connection result to `a9ec60450...`
- Commit and push `diagnostic_report.md` back to git so both sides can inspect.
