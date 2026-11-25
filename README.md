<div align="center">
  <img src="cymesh-cyber-logo.png" width="144" alt="CyMesh logo">
  <h1>CyMesh</h1>
  <p><strong>Encrypted peer-to-peer messaging for Windows</strong></p>
  <p>An open-source project by the <strong>Cynet security team</strong>.</p>
  <p>
    <a href="README.fa.md">فارسی</a> ·
    <a href="https://github.com/Cynetx/CyMesh/actions/workflows/release-windows.yml">Windows builds</a> ·
    <a href="#relay-builder">Relay builder</a> ·
    <a href="CONTRIBUTING.md">Contribute</a>
  </p>
  <p>
    <a href="https://github.com/Cynetx/CyMesh/actions/workflows/release-windows.yml"><img src="https://github.com/Cynetx/CyMesh/actions/workflows/release-windows.yml/badge.svg?branch=main" alt="Windows build status"></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-00a88f" alt="MIT license"></a>
  </p>
</div>

Create an identity on your device, share an invite with a friend, and start a conversation. CyMesh can discover peers on a local network and use a bridge you choose when you need to reach someone on another network. Account creation needs no phone number or email.

## Try it with a friend

1. **Get the Windows app.** Open a successful [Windows build](https://github.com/Cynetx/CyMesh/actions/workflows/release-windows.yml) and look for **Cymesh-Windows-x64** under **Artifacts**. GitHub requires sign-in for artifact downloads. If an artifact is unavailable, [build from source](#build-from-source).
2. **Create your identity.** Set a PIN to lock the local vault and store the 12 recovery words somewhere private. The recovery phrase restores your identity. Chat history stays in the local vault.
3. **Exchange an invite.** On the same local network, allow peer discovery and open **Find People** to share an invite link or QR code. You can also search for `@handle#TAG` through reachable peers. To verify your contact, compare the safety code through a channel you already trust.

**Connecting across networks?** In **Settings**, expand **Advanced Network & Blackout Bridge Kit**, then choose **Add Bridge** to enter a trusted bridge address. You can also import a `CYMESH-SEED` bundle there. CyMesh ships with an empty bridge list. Delivery depends on whether a peer or relay is reachable.

## What you can do

| Feature | What it does |
| --- | --- |
| **Chat in English or Persian** | Switch between left-to-right and right-to-left layouts. Reply, react, edit messages, and set a message expiry timer. |
| **Reach peers directly or through a relay** | Discover peers on your LAN or through the distributed directory. A reachable relay can hold encrypted packets for later delivery. |
| **Keep identity and chat data on your device** | Open your encrypted local vault with your PIN. Use the recovery phrase to restore your identity on a new installation. |
| **See how you’re connected** | Check the connection route, mesh contacts and queued packets in the app. Test a relay you specify from Settings. |

## Build from source

Install [stable Rust](https://rustup.rs/), [Node.js 24 with npm](https://nodejs.org/), and the [Tauri prerequisites for your platform](https://v2.tauri.app/start/prerequisites/). On Windows, these include the Visual Studio C++ Build Tools with **Desktop development with C++** and the WebView2 Runtime.

Clone and set up the project from a terminal:

```sh
git clone https://github.com/Cynetx/CyMesh.git
cd CyMesh
npm ci
npm --prefix ui ci
npm run dev
```

Run the Rust tests and build the frontend:

```sh
cargo test --locked -p aegis-core
npm --prefix ui run build
```

On Windows, build the x64 desktop app and NSIS installer:

```sh
npm run build -- --bundles nsis -- --locked
```

The executable is `target/release/Cymesh.exe`. The installer is under `target/release/bundle/nsis/`.

<details>
<summary>Build the portable Windows ZIP on Linux</summary>

Install MinGW-w64, `zip`, Rust and Node.js with npm, then run:

```sh
rustup target add x86_64-pc-windows-gnu
./scripts/build-windows.sh
```

The script writes `dist-windows/Cymesh-Windows-x64-Portable.zip`. Windows needs the WebView2 Runtime to run the portable app.

</details>

## Run your own relay

A relay can give your group a reachable entry point across networks. Build and run it on Linux from the project root in Bash:

```bash
cargo build --locked --release -p aegis-core --bin cymesh-relay
read -r -s -p 'Relay vault passphrase: ' relay_pin
printf '\n'
CYMESH_RELAY_PIN="$relay_pin" ./target/release/cymesh-relay --port 47833
unset relay_pin
```

Use a dedicated, unprivileged account and keep the vault passphrase for restarts. From each client, add the relay’s reachable IP and port under **Add Bridge**. The default mesh port is **47833 over TCP and UDP**. LAN discovery uses **UDP 47832** by default. Run `cymesh-relay --help` for its data directory and bootstrap options.

<a name="relay-builder"></a>

### Build a Windows client preconfigured for your relay

The standalone builder makes a separate Windows edition for your group. It fills the initial bridge setting when a user creates or recovers an identity in that edition. Users can remove the bridge in Settings. **Contribute Volunteer Relay** inside the app remains the way to register and announce a relay to the existing mesh.

On your build machine, run from the source checkout:

```sh
npm run relay-builder
```

Enter the IPv4 address that clients can reach and the relay's TCP/UDP port (default `47833`). The builder prints the relay start commands for your Linux server and the exact `npm run relay-builder -- build <profile-id>` command to run on your build machine. Set up the relay and permit its port on the network before testing the client. The relay vault passphrase is entered on the server when starting the relay.

- **Windows** uses the [Windows build prerequisites](#build-from-source) and makes an x64 NSIS installer.
- **Linux** uses the MinGW-w64, `zip`, and Rust target prerequisites in the [portable build instructions](#build-from-source) and makes a Windows x64 portable ZIP.

The output is in `.cymesh-builder/<profile-id>/windows/`. Install the NSIS package or unpack the portable ZIP and open `Cymesh.exe`; the first unlocked session can contact the configured relay immediately. Each relay profile has its own vault and shortcuts. The relay address is readable from the resulting app. Share the build accordingly and use the relay's network controls to decide who can reach it.

## Architecture and privacy

| Part | Responsibility |
| --- | --- |
| [React UI](ui/src/) | Chat, pairing, settings and Persian/English translations |
| [Tauri desktop app](src-tauri/) | Windows integration and the bridge between the UI and Rust |
| [Rust core](crates/aegis-core/) | Identity, encrypted storage, peer discovery, transport and relay |

CyMesh implements signed profiles, Noise transport, Double Ratchet message encryption and optional multi-hop onion routing. Peers and relays can observe connection metadata such as IP addresses. A relay, NAT traversal or the offline queue can help when a peer is temporarily unreachable. A network outage can still prevent delivery. The project has automated tests for local multi-peer scenarios.

The client checks local network adapters and proxy settings. The optional country check is off by default. When enabled, third-party HTTPS services receive the connection’s source IP to look up its country. Invite links and seed bundles can contain network addresses, so share them with their intended recipients.

## Help build CyMesh

Try a conversation between two devices, report what worked or where it got stuck, or help polish the Persian and English interfaces. [Open an issue](https://github.com/Cynetx/CyMesh/issues) · [Contributing guide](CONTRIBUTING.md) · [Private security reporting](SECURITY.md)

**Cynet community** · [Website](https://cynetx.ir/) · [Forum](https://forum.cynetx.ir/) · [Telegram channel](https://t.me/cynetx) · [Telegram group](https://t.me/cynetgap) · [GitHub](https://github.com/Cynetx)

CyMesh is available under the [MIT License](LICENSE).
