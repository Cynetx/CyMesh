import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { closeSync, copyFileSync, mkdirSync, openSync, readdirSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { isIPv4 } from "node:net";
import { join, relative } from "node:path";
import { createInterface } from "node:readline/promises";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const profilesDir = join(root, ".cymesh-builder");
const defaultPort = "47833";

function relayAddress(host, port) {
  if (!isIPv4(host) || ["0.0.0.0", "255.255.255.255"].includes(host) || Number(host.split(".")[0]) >= 224) {
    throw new Error("Enter the relay's reachable unicast IPv4 address.");
  }
  if (!/^\d{1,5}$/.test(port) || Number(port) < 1 || Number(port) > 65535) {
    throw new Error("Enter a port from 1 to 65535.");
  }
  return `${host}:${Number(port)}`;
}

function profileId(relay) {
  const hash = createHash("sha256").update(relay).digest("hex");
  return `relay-${hash.slice(0, 12)}`;
}

async function configure(args) {
  let [host, port = defaultPort] = args;
  if (host === undefined) {
    if (!process.stdin.isTTY) throw new Error("Run interactively or supply an IPv4 address and port.");
    const prompts = createInterface({ input: process.stdin, output: process.stdout });
    try {
      host = await prompts.question("Relay IPv4 address (آدرس سرور): ");
      port = (await prompts.question(`Relay port [${defaultPort}]: `)).trim() || defaultPort;
    } finally {
      prompts.close();
    }
  }

  const relay = relayAddress(host.trim(), port.trim());
  const relayPort = relay.split(":")[1];
  const id = profileId(relay);
  const profileDir = join(profilesDir, id);
  mkdirSync(profileDir, { recursive: true });
  writeFileSync(join(profileDir, "relay.json"), JSON.stringify({ relay }, null, 2) + "\n", { mode: 0o600 });

  console.log(`\nSaved local profile ${id} for ${relay}.`);
  console.log("\nOn the Linux relay, from a checkout of CyMesh, run in Bash:");
  console.log("  cargo build --locked --release -p aegis-core --bin cymesh-relay");
  console.log("  read -r -s -p 'Relay vault passphrase: ' relay_pin");
  console.log("  printf '\\n'");
  console.log(`  CYMESH_RELAY_PIN="$relay_pin" ./target/release/cymesh-relay --port ${relayPort}`);
  console.log("  unset relay_pin");
  console.log(`Allow client access to port ${relayPort} over TCP and UDP on the relay network.`);
  console.log("\nOn your build machine, from this checkout:");
  console.log(`  npm run relay-builder -- build ${id}`);
  console.log("\nWindows builds an NSIS installer. Linux builds a portable Windows x64 ZIP.");
  console.log(`Output: .cymesh-builder/${id}/windows/`);
  console.log("Run the setup file, or extract the portable ZIP and open Cymesh.exe.");
  console.log("The client includes the relay address. Keep the profile and installer within the intended group.");
}

function run(command, args, env) {
  const result = spawnSync(command, args, { cwd: root, env, stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${command} failed (${result.signal ?? result.status}).`);
  }
}

function build(id) {
  if (!/^relay-[a-f0-9]{12}$/.test(id ?? "")) throw new Error("Enter a profile ID created by this builder.");
  if (!["win32", "linux"].includes(process.platform)) throw new Error("Build on Windows or Linux.");

  const profileDir = join(profilesDir, id);
  const data = JSON.parse(readFileSync(join(profileDir, "relay.json"), "utf8"));
  const [host, port, extra] = String(data.relay).split(":");
  const relay = relayAddress(host, port ?? "");
  if (extra !== undefined || profileId(relay) !== id) throw new Error("Profile changed. Run the builder again to create a new profile.");

  const output = join(profileDir, "windows");
  const env = {
    ...process.env,
    CYMESH_BUILDER_RELAY: relay,
    CYMESH_BUILDER_PROFILE: id,
    CARGO_TARGET_DIR: join(profileDir, "target"),
    CYMESH_DIST_DIR: output,
  };
  const overlay = {
    productName: `Cymesh ${id}`,
    identifier: `ir.cynetx.cymesh.${id}`,
  };

  const lockPath = join(profilesDir, "build.lock");
  let lock;
  try {
    lock = openSync(lockPath, "wx", 0o600);
  } catch (error) {
    if (error.code === "EEXIST") {
      throw new Error("Another builder may be running. If it was interrupted, remove .cymesh-builder/build.lock after it stops.");
    }
    throw error;
  }

  try {
    if (process.platform === "linux") {
      run("bash", ["scripts/build-windows.sh"], { ...env, TAURI_CONFIG: JSON.stringify(overlay) });
    } else {
      const npm = process.env.npm_execpath;
      if (!npm) throw new Error("Start the builder with npm run relay-builder.");
      const configPath = join(profileDir, "tauri.json");
      writeFileSync(configPath, JSON.stringify(overlay));
      delete env.TAURI_CONFIG;
      run(process.execPath, [npm, "ci"], env);
      run(process.execPath, [npm, "--prefix", "ui", "ci"], env);
      run(process.execPath, [
        npm, "run", "build", "--", "--ci", "--bundles", "nsis", "--target", "x86_64-pc-windows-msvc",
        "--config", configPath, "--", "--locked",
      ], env);

      const bundles = join(env.CARGO_TARGET_DIR, "x86_64-pc-windows-msvc", "release", "bundle", "nsis");
      const { version } = JSON.parse(readFileSync(join(root, "src-tauri", "tauri.conf.json"), "utf8"));
      const installers = readdirSync(bundles).filter((name) => name.endsWith(`_${version}_x64-setup.exe`));
      if (installers.length !== 1) throw new Error(`Expected one NSIS installer in ${bundles}.`);
      mkdirSync(output, { recursive: true });
      copyFileSync(join(bundles, installers[0]), join(output, `Cymesh-${id}-Windows-x64-Setup.exe`));
    }
    console.log(`\nWindows output: ${relative(root, output)}`);
  } finally {
    closeSync(lock);
    unlinkSync(lockPath);
  }
}

try {
  const [command, ...args] = process.argv.slice(2);
  if (command === "build" && args.length === 1) {
    build(args[0]);
  } else if (command === "--help") {
    console.log("npm run relay-builder [-- configure <IPv4> [port]]");
    console.log("npm run relay-builder -- build <profile-id>");
  } else if (command === undefined || (command === "configure" && args.length <= 2)) {
    await configure(args);
  } else {
    throw new Error("Run npm run relay-builder -- --help for usage.");
  }
} catch (error) {
  console.error(`Relay builder: ${error.message}`);
  process.exitCode = 1;
}
