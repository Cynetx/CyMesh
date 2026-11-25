# Cymesh

Cymesh is a Windows-first peer-to-peer desktop messenger from the **Cynet security team**. Its React and TypeScript interface supports English and Persian with LTR and RTL layouts. The desktop app uses Tauri and a Rust messaging core. The source is available under the [MIT license](LICENSE).

## فارسی

**Cymesh** پیام‌رسان همتابه‌همتای تیم امنیتی **ساینت (Cynet)** است. برنامه رابط فارسی و انگلیسی دارد و برای ویندوز طراحی شده است. روی شبکهٔ محلی از کشف همتا استفاده می‌کند. برای پیدا کردن همتا در شبکه‌ای دیگر، آدرس یک پل مورد اعتماد یا بستهٔ `CYMESH-SEED` را از تنظیمات وارد کنید. نسخهٔ عمومی با فهرست پلِ خالی شروع می‌شود.

برای ساخت روی ویندوز، Rust پایدار، Node.js 24 با npm، ابزارهای C++ ویژوال‌استودیو و WebView2 را نصب کنید. دستورهای بخش ساخت و اجرا را از ریشهٔ پروژه اجرا کنید. نصب‌کنندهٔ NSIS در `target/release/bundle/nsis/` ساخته می‌شود.

## Build and run | ساخت و اجرا

Install [stable Rust](https://rustup.rs/), [Node.js 24 with npm](https://nodejs.org/), and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/). On Windows, use the Microsoft C++ Build Tools with the **Desktop development with C++** workload and install the WebView2 Runtime if needed.

From the repository root:

```sh
npm ci
npm --prefix ui ci
npm run dev
```

Use the following commands to test the Rust core, type-check and build the frontend, and produce the Windows NSIS installer:

```sh
cargo test --locked -p aegis-core
npm --prefix ui run build
npm run build -- --bundles nsis -- --locked
```

Build the NSIS installer on Windows. The executable is created at `target/release/Cymesh.exe`, and the installer is under `target/release/bundle/nsis/`. GitHub Actions runs the core tests and builds downloadable Windows artifacts for each successful run.

For a portable Windows ZIP from Linux, install MinGW-w64, `zip`, Node.js with npm and stable Rust. Then run:

```sh
rustup target add x86_64-pc-windows-gnu
./scripts/build-windows.sh
```

The resulting ZIP is written to `dist-windows/`. Windows still needs the WebView2 Runtime to run the portable application.

## Connect peers and configure a relay

1. Create a local identity. Store the 12 recovery words privately. The phrase restores the identity, while chat history lives in the local vault.
2. To pair on a LAN, place both peers on a network that permits peer discovery and exchange invite links or use discovery. Across networks, add a reachable, trusted bridge under **Settings → Advanced Network → Add Bridge**, import a seed bundle, or connect to a known peer address.
3. To verify a contact, compare the safety code using a trusted channel.

The public build has an empty bridge list. Cross-network connectivity depends on reachable peers or relays, network conditions, and firewall and NAT behavior. Local discovery uses UDP 47832 by default. The mesh normally uses port 47833 for TCP and UDP. A network outage can prevent peers from reaching one another.

To build and start the optional relay from Linux in Bash:

```bash
cargo build --locked --release -p aegis-core --bin cymesh-relay
read -r -s -p 'Relay vault passphrase: ' relay_pin
printf '\n'
CYMESH_RELAY_PIN="$relay_pin" ./target/release/cymesh-relay --port 47833
unset relay_pin
```

Choose a strong, unique passphrase and keep it for future restarts. Run the relay under a dedicated, unprivileged account and add its reachable IP and port in clients that should use it. `--help` lists the storage path and peer options. Share relay invitations and seed bundles only with intended recipients since they may include network addresses.

## Design and privacy

- `ui/` contains the React UI, translations and Tauri bridge. `src-tauri/` contains desktop configuration and command handlers. `crates/aegis-core/` contains identity, encryption, local storage, peer discovery and the relay CLI.
- The source implements signed profiles, Noise transport, Double Ratchet message encryption, an encrypted local vault, and optional relay, offline delivery and onion routing paths. Peers and relays can observe connection information. The included tests exercise local scenarios. A security audit is a separate activity.
- The local VPN/proxy check uses local interface and environment information. The optional Iran IP check, disabled by default, contacts third-party HTTPS services that receive the source IP.
- The public desktop build uses a separate application identifier for its vault. Start with a new test vault when verifying changes.

## Cynet community | جامعهٔ ساینت

- [Website · سایت](https://cynetx.ir/)
- [Forum · انجمن](https://forum.cynetx.ir/)
- [Telegram channel · کانال تلگرام](https://t.me/cynetx)
- [Telegram group · گروه تلگرام](https://t.me/cynetgap)
- [GitHub · گیت‌هاب](https://github.com/Cynetx)

See [CONTRIBUTING.md](CONTRIBUTING.md) for development contributions and [SECURITY.md](SECURITY.md) for private reporting.
