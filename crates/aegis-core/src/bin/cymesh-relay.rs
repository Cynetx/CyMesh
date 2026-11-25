use std::path::PathBuf;
use std::time::Duration;

use aegis_core::models::CoreEvent;
use aegis_core::AegisNode;

struct RelayConfig {
    data_dir: PathBuf,
    port: u16,
    pin: Option<String>,
    peers: Vec<String>,
}

fn print_help() {
    println!(
        "cymesh-relay - Zero-Server Blind Mesh Relay & DHT Bridge\n\n\
         USAGE:\n  \
           cymesh-relay [OPTIONS]\n\n\
         OPTIONS:\n  \
           --port <PORT>        TCP/UDP mesh port to listen on (default: 47833)\n  \
           --data-dir <PATH>    Encrypted relay vault directory (default: ./cymesh-relay-data)\n  \
           --peer <TARGET>      Bootstrap peer IP:Port, cymesh:// link, or CYMESH-SEED (repeatable)\n  \
           --pin <PIN>          Vault unlock passphrase (or CYMESH_RELAY_PIN env)\n  \
           -h, --help           Show this help message"
    );
}

fn parse_args() -> RelayConfig {
    let mut cfg = RelayConfig {
        data_dir: PathBuf::from("./cymesh-relay-data"),
        port: 47833,
        pin: std::env::var("CYMESH_RELAY_PIN").ok(),
        peers: Vec::new(),
    };

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            "--port" => {
                let val = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --port");
                    std::process::exit(2);
                });
                cfg.port = val.parse().unwrap_or_else(|_| {
                    eprintln!("Invalid port: {val}");
                    std::process::exit(2);
                });
            }
            "--data-dir" => {
                let val = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --data-dir");
                    std::process::exit(2);
                });
                cfg.data_dir = PathBuf::from(val);
            }
            "--peer" => {
                let val = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --peer");
                    std::process::exit(2);
                });
                cfg.peers.push(val);
            }
            "--pin" => {
                let val = args.next().unwrap_or_else(|| {
                    eprintln!("Missing value for --pin");
                    std::process::exit(2);
                });
                cfg.pin = Some(val);
            }
            other => {
                eprintln!("Unknown argument: {other}");
                print_help();
                std::process::exit(2);
            }
        }
    }
    cfg
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = parse_args();
    let pin = cfg
        .pin
        .as_deref()
        .filter(|pin| pin.chars().count() >= 4 && !pin.trim().is_empty())
        .ok_or("Set CYMESH_RELAY_PIN or pass --pin with a vault passphrase of at least four characters")?;
    std::fs::create_dir_all(&cfg.data_dir)?;

    let node = AegisNode::new_default(cfg.data_dir.clone());
    if !node.is_initialized() {
        let created = node
            .create_identity_and_vault(
                pin,
                "Cymesh Relay".to_string(),
                "relay".to_string(),
                "Volunteer Blind Mesh Relay & DHT Bridge".to_string(),
                "Relay Node".to_string(),
                "#00F0FF".to_string(),
                None,
            )
            .await?;
        println!("Initialized new relay vault. Peer ID: {}", created.profile.peer_id);
    } else {
        node.unlock_vault(pin).await?;
        println!("Unlocked existing relay vault at {}", cfg.data_dir.display());
    }

    let mut settings = node.get_settings().await.unwrap_or_default();
    let needs_rebind = settings.listen_port != cfg.port;
    settings.stealth_mode = true;
    settings.mesh_relay_enabled = true;
    settings.dht_enabled = true;
    settings.hole_punch_enabled = true;
    settings.onion_routing_enabled = false;
    settings.listen_port = cfg.port;
    node.update_settings(settings).await?;

    if needs_rebind {
        node.lock_vault().await;
        node.unlock_vault(pin).await?;
    }

    for peer in &cfg.peers {
        if let Err(err) = node.add_bootstrap_peer(peer).await {
            eprintln!("Warning: could not add bootstrap peer '{peer}': {err}");
        }
    }

    let tel = node.get_network_telemetry().await?;
    println!("============================================================");
    println!("Cymesh Blind Relay Node ONLINE");
    println!("Listening TCP/UDP Port : {}", tel.node_listening_port);
    println!("Local IPs              : {:?}", tel.local_ips);
    println!("Relay Stealth Mode     : {} (hidden from user search)", tel.stealth_mode);
    println!("Share Invite URI       : {}", tel.share_uri);
    println!("============================================================");
    println!("Clients can connect by adding <SERVER_PUBLIC_IP>:{} in Cymesh Settings -> Add Bridge.", tel.node_listening_port);

    let mut events = node.subscribe();
    let mut status_tick = tokio::time::interval(Duration::from_secs(60));
    status_tick.tick().await;

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!("\nShutting down Cymesh Relay cleanly...");
                node.lock_vault().await;
                break;
            }
            _ = status_tick.tick() => {
                if let Ok(t) = node.get_network_telemetry().await {
                    println!(
                        "[status] active_peers={} dht_nodes={} dht_records={} relayed_pkts={} nat_ext={:?}",
                        t.active_direct_peers,
                        t.dht_known_nodes,
                        t.dht_stored_records,
                        t.mesh_relayed_packets,
                        t.observed_external_addr
                    );
                }
            }
            evt = events.recv() => {
                match evt {
                    Ok(CoreEvent::PeerUpdated(p)) => {
                        println!(
                            "[peer] {} ({}) route={:?}",
                            p.account_id, p.profile.peer_id, p.route
                        );
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        eprintln!("[warn] skipped {skipped} event log lines");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    _ => {}
                }
            }
        }
    }

    Ok(())
}
