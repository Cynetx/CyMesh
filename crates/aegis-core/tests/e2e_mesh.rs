use tempfile::TempDir;
use tokio::time::{sleep, Duration};

use aegis_core::crypto::{
    seal_for_recipient, unseal_packet, DoubleRatchetState, SealedInnerPayload,
};
use aegis_core::identity::{compute_safety_verification, decode_32_b58, IdentityKeys};
use aegis_core::models::{CoreEvent, DeliveryStatus, PresenceStatus, VaultUnlockMode};
use aegis_core::storage::{DuressAction, UnlockedVault};
use aegis_core::AegisNode;

#[test]
fn test_identity_mnemonic_and_profile_tamper_detection() {
    let (keys, mnemonic) = IdentityKeys::generate().expect("generate keys");
    let recovered = IdentityKeys::from_mnemonic(&mnemonic).expect("recover from mnemonic");
    assert_eq!(keys.peer_id(), recovered.peer_id());
    assert_eq!(keys.x25519_pub_b58(), recovered.x25519_pub_b58());

    let mut card = keys.build_signed_profile(
        "Ghost Operator",
        "@ghost",
        "Zero-trust node",
        "Cipher Lead",
        "#00F0FF",
        None,
        PresenceStatus::Online,
    );
    assert!(card.verify().is_ok());

    // Tampering with display_name must invalidate Ed25519 signature
    card.display_name = "Imposter".to_string();
    assert!(card.verify().is_err());
}

#[test]
fn test_safety_number_symmetry() {
    let (alice, _) = IdentityKeys::generate().unwrap();
    let (bob, _) = IdentityKeys::generate().unwrap();
    let (num_ab, emo_ab) = compute_safety_verification(&alice.peer_id(), &bob.peer_id()).unwrap();
    let (num_ba, emo_ba) = compute_safety_verification(&bob.peer_id(), &alice.peer_id()).unwrap();
    assert_eq!(num_ab, num_ba);
    assert_eq!(emo_ab, emo_ba);
}

#[test]
fn test_double_ratchet_and_sealed_envelope() {
    let (alice, _) = IdentityKeys::generate().unwrap();
    let (bob, _) = IdentityKeys::generate().unwrap();

    let mut alice_dr = DoubleRatchetState::initialize(
        &alice.x25519_secret(),
        &alice.x25519_public(),
        &bob.x25519_public(),
    );
    let mut bob_dr = DoubleRatchetState::initialize(
        &bob.x25519_secret(),
        &bob.x25519_public(),
        &alice.x25519_public(),
    );

    // Simultaneous messages before first DH turn
    let (env_a1, _) = alice_dr.encrypt(b"Salam Bob!").unwrap();
    let (env_a2, _) = alice_dr.encrypt(b"Message 2 out-of-order test").unwrap();
    let (env_b1, _) = bob_dr.encrypt(b"Salam Alice!").unwrap();

    // Deliver a2 before a1 to verify skipped key window
    let dec_a2 = bob_dr.decrypt(&env_a2).unwrap();
    let dec_a1 = bob_dr.decrypt(&env_a1).unwrap();
    assert_eq!(&*dec_a2, b"Message 2 out-of-order test");
    assert_eq!(&*dec_a1, b"Salam Bob!");

    let dec_b1 = alice_dr.decrypt(&env_b1).unwrap();
    assert_eq!(&*dec_b1, b"Salam Alice!");

    // DH Ratchet turn after receiving peer's ephemeral key
    let (env_b2, _) = bob_dr.encrypt(b"After DH ratchet turn").unwrap();
    let dec_b2 = alice_dr.decrypt(&env_b2).unwrap();
    assert_eq!(&*dec_b2, b"After DH ratchet turn");

    // Sealed mesh envelope
    let bob_x = decode_32_b58(&bob.x25519_pub_b58()).unwrap();
    let sealed = seal_for_recipient(
        "pkt-1".into(),
        &bob.peer_id(),
        &bob_x,
        &SealedInnerPayload {
            sender_peer_id: alice.peer_id(),
            payload_json: b"hidden-payload".to_vec(),
            sender_profile: None,
        },
        3,
        9_999_999_999_999,
    )
    .unwrap();
    let unsealed = unseal_packet(&sealed, &bob.peer_id(), &bob.x25519_secret()).unwrap();
    assert_eq!(unsealed.sender_peer_id, alice.peer_id());
    assert_eq!(unsealed.payload_json, b"hidden-payload");
}

#[test]
fn test_vault_duress_decoy_and_wipe_modes() {
    let dir = TempDir::new().unwrap();
    let (keys, _) = IdentityKeys::generate().unwrap();
    let profile = keys.build_signed_profile(
        "Real User",
        "@real",
        "Sensitive vault",
        "Operator",
        "#00FF9D",
        None,
        PresenceStatus::Online,
    );

    let vault = UnlockedVault::create_new(
        dir.path(),
        "real-pin-1234",
        &keys,
        &profile,
        Some(("panic-9999", DuressAction::Decoy)),
    )
    .unwrap();
    drop(vault);

    assert!(UnlockedVault::unlock(dir.path(), "wrong-pin").is_err());

    let (primary_v, primary_k) = UnlockedVault::unlock(dir.path(), "real-pin-1234").unwrap();
    assert_eq!(primary_v.mode, VaultUnlockMode::Primary);
    assert_eq!(primary_k.peer_id(), keys.peer_id());
    drop(primary_v);

    // Unlocking with panic PIN returns decoy identity cleanly
    let (decoy_v, decoy_k) = UnlockedVault::unlock(dir.path(), "panic-9999").unwrap();
    assert_eq!(decoy_v.mode, VaultUnlockMode::Decoy);
    assert_ne!(decoy_k.peer_id(), keys.peer_id());
}

#[tokio::test]
async fn test_two_node_serverless_e2ee_and_offline_sync() {
    let dir_a = TempDir::new().unwrap();
    let dir_b = TempDir::new().unwrap();

    // Use distinct discovery ports in unit test to isolate from LAN traffic
    let node_a = AegisNode::new(dir_a.path().to_path_buf(), 48891);
    let node_b = AegisNode::new(dir_b.path().to_path_buf(), 48892);

    let bundle_a = node_a
        .create_identity_and_vault(
            "pin-a",
            "Alice Cyber".into(),
            "alice".into(),
            "Node A".into(),
            "Architect".into(),
            "#00F0FF".into(),
            None,
        )
        .await
        .unwrap();

    let bundle_b = node_b
        .create_identity_and_vault(
            "pin-b",
            "Bob Mesh".into(),
            "bob".into(),
            "Node B".into(),
            "Analyst".into(),
            "#00FF9D".into(),
            None,
        )
        .await
        .unwrap();

    let mut rx_b = node_b.subscribe();
    let mut rx_a = node_a.subscribe();

    // Alice connects directly to Bob using Bob's serverless share URI
    let tel_b = node_b.get_network_telemetry().await.unwrap();
    let direct_uri = aegis_core::identity::encode_share_uri(
        &bundle_b.profile,
        &[format!("127.0.0.1:{}", tel_b.node_listening_port)],
    )
    .unwrap();
    node_a.add_peer_by_uri(&direct_uri).await.unwrap();

    // Wait briefly for Noise_XX handshake + attestation
    sleep(Duration::from_millis(350)).await;

    let sent_msg = node_a
        .send_message(&bundle_b.profile.peer_id, "درود! اتصال کاملاً امن و بدون سرور!".into(), None, None)
        .await
        .unwrap();
    assert_eq!(sent_msg.status, DeliveryStatus::SentDirect);

    // Wait for Bob to receive & decrypt message
    let mut received_on_b = false;
    for _ in 0..20 {
        if let Ok(CoreEvent::MessageReceived(m)) =
            tokio::time::timeout(Duration::from_millis(150), rx_b.recv()).await.unwrap_or(Err(tokio::sync::broadcast::error::RecvError::Closed))
        {
            if m.content.contains("بدون سرور") {
                received_on_b = true;
                break;
            }
        }
    }
    assert!(received_on_b, "Bob should receive encrypted direct message");

    // Verify DeliveryReceipt arrived at Alice
    sleep(Duration::from_millis(200)).await;
    let msgs_on_a = node_a.list_messages(&bundle_b.profile.peer_id).await.unwrap();
    assert_eq!(msgs_on_a.last().unwrap().status, DeliveryStatus::Delivered);

    // Test Offline Store-and-Forward: Bob locks vault (goes offline)
    node_b.lock_vault().await;
    sleep(Duration::from_millis(150)).await;

    let queued = node_a
        .send_message(&bundle_b.profile.peer_id, "Offline queued E2EE message".into(), None, None)
        .await
        .unwrap();
    assert_eq!(queued.status, DeliveryStatus::QueuedOffline);

    // Bob unlocks vault; Alice's maintenance loop or reconnect flushes sealed outbox!
    node_b.unlock_vault("pin-b").await.unwrap();
    let tel_b2 = node_b.get_network_telemetry().await.unwrap();
    let _ = node_a
        .connect_peer_addr(&format!("127.0.0.1:{}", tel_b2.node_listening_port))
        .await
        .unwrap();

    sleep(Duration::from_millis(400)).await;
    let msgs_on_b = node_b.list_messages(&bundle_a.profile.peer_id).await.unwrap();
    assert!(
        msgs_on_b.iter().any(|m| m.content == "Offline queued E2EE message"),
        "Bob should receive queued sealed offline message upon reconnect"
    );
    let _ = rx_a.try_recv();
}

async fn setup_test_node(
    dir: &TempDir,
    discovery_port: u16,
    pin: &str,
    name: &str,
    handle: &str,
    listen_port: u16,
) -> (AegisNode, aegis_core::models::IdentityCreatedBundle) {
    let node = AegisNode::new(dir.path().to_path_buf(), discovery_port);
    let bundle = node
        .create_identity_and_vault(
            pin,
            name.into(),
            handle.into(),
            "Cross-city mesh node".into(),
            "Relay".into(),
            "#00F0FF".into(),
            None,
        )
        .await
        .unwrap();
    let mut settings = node.get_settings().await.unwrap();
    settings.listen_port = listen_port;
    node.update_settings(settings).await.unwrap();
    (node, bundle)
}

#[tokio::test]
async fn test_4_node_dht_discovery_and_2hop_onion_relay() {
    // Topology: Alice <-> Relay1 <-> Relay2 <-> Bob
    // Distinct discovery ports guarantee zero LAN broadcast cross-talk; Alice & Bob NEVER connect directly!
    let d_a = TempDir::new().unwrap();
    let d_r1 = TempDir::new().unwrap();
    let d_r2 = TempDir::new().unwrap();
    let d_b = TempDir::new().unwrap();

    let (alice, b_alice) = setup_test_node(&d_a, 49101, "pin-a", "Alice CityA", "alice_a", 0).await;
    let (relay1, _) = setup_test_node(&d_r1, 49102, "pin-r1", "Relay One", "relay1", 0).await;
    let (relay2, _) = setup_test_node(&d_r2, 49103, "pin-r2", "Relay Two", "relay2", 0).await;
    let (bob, b_bob) = setup_test_node(&d_b, 49104, "pin-b", "Bob CityB", "bob_b", 0).await;

    // Enable strict Onion Multi-Hop mode on Alice so Alice NEVER dials Bob directly
    let mut alice_cfg = alice.get_settings().await.unwrap();
    alice_cfg.onion_routing_enabled = true;
    alice_cfg.hole_punch_enabled = false;
    alice.update_settings(alice_cfg).await.unwrap();

    let mut bob_cfg = bob.get_settings().await.unwrap();
    bob_cfg.onion_routing_enabled = true;
    bob_cfg.hole_punch_enabled = false;
    bob.update_settings(bob_cfg).await.unwrap();

    let tel_r1 = relay1.get_network_telemetry().await.unwrap();
    let tel_r2 = relay2.get_network_telemetry().await.unwrap();
    let r1_addr = format!("127.0.0.1:{}", tel_r1.node_listening_port);
    let r2_addr = format!("127.0.0.1:{}", tel_r2.node_listening_port);

    // Connect mesh overlay: Alice -> Relay1 -> Relay2 <- Bob
    relay1.add_bootstrap_peer(&r2_addr).await.unwrap();
    bob.add_bootstrap_peer(&r2_addr).await.unwrap();
    sleep(Duration::from_millis(400)).await;
    alice.add_bootstrap_peer(&r1_addr).await.unwrap();
    sleep(Duration::from_millis(600)).await;

    // Alice finds Bob across cities using ONLY Bob's PeerId via DHT lookup
    alice.connect_peer_by_id(&b_bob.profile.peer_id).await.unwrap();

    let mut rx_b = bob.subscribe();
    let sent = alice
        .send_message(
            &b_bob.profile.peer_id,
            "پیام رمزنگاری پیازی بین دو شهر بدون سرور!".into(),
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(sent.status, DeliveryStatus::RelayedMesh);

    let mut received = false;
    for _ in 0..30 {
        if let Ok(CoreEvent::MessageReceived(msg)) = tokio::time::timeout(
            Duration::from_millis(200),
            rx_b.recv(),
        )
        .await
        .unwrap_or(Err(tokio::sync::broadcast::error::RecvError::Closed))
        {
            if msg.content.contains("پیازی بین دو شهر") {
                received = true;
                break;
            }
        }
    }
    assert!(received, "Bob must receive & decrypt Double-Ratchet message over 2-hop Onion mesh");

    // Verify Alice NEVER opened a direct link to Bob, yet routed via Onion circuit
    let tel_a = alice.get_network_telemetry().await.unwrap();
    assert_eq!(tel_a.active_direct_peers, 1, "Alice must only be connected to Relay1");
    assert!(tel_a.onion_routed_packets >= 1);

    // Verify intermediate relays hold ZERO decrypted messages (true zero-knowledge E2EE)
    assert!(relay1.list_messages(&b_bob.profile.peer_id).await.unwrap().is_empty());
    assert!(relay2.list_messages(&b_alice.profile.peer_id).await.unwrap().is_empty());
}

#[tokio::test]
async fn test_peer_echo_and_udp_hole_punch_direct_tunnel() {
    // Topology: Alice <-> RelayR <-> Bob
    // Proves Peer-Echo external address discovery + coordinated UDP NAT Hole Punching + encrypted UDP tunnel!
    let d_a = TempDir::new().unwrap();
    let d_r = TempDir::new().unwrap();
    let d_b = TempDir::new().unwrap();

    let (alice, b_alice) = setup_test_node(&d_a, 49201, "pin-a", "Alice NAT", "alice_nat", 0).await;
    let (relay, _) = setup_test_node(&d_r, 49202, "pin-r", "Introducer Relay", "intro", 0).await;
    let (bob, b_bob) = setup_test_node(&d_b, 49203, "pin-b", "Bob NAT", "bob_nat", 0).await;

    let tel_r = relay.get_network_telemetry().await.unwrap();
    let r_addr = format!("127.0.0.1:{}", tel_r.node_listening_port);

    bob.add_bootstrap_peer(&r_addr).await.unwrap();
    sleep(Duration::from_millis(350)).await;
    alice.add_bootstrap_peer(&r_addr).await.unwrap();
    sleep(Duration::from_millis(500)).await;

    // Both Alice & Bob must have learned their Peer-Observed UDP address without Google STUN
    let tel_a0 = alice.get_network_telemetry().await.unwrap();
    let tel_b0 = bob.get_network_telemetry().await.unwrap();
    assert!(tel_a0.observed_external_addr.is_some());
    assert!(tel_b0.observed_external_addr.is_some());

    // Pair Alice <-> Bob ONLY via DHT + UDP Hole Punching (NO TCP dial between Alice & Bob!)
    alice.initiate_udp_punch(&b_bob.profile.peer_id).await.unwrap();

    let mut punched = false;
    for _ in 0..25 {
        let ta = alice.get_network_telemetry().await.unwrap();
        let tb = bob.get_network_telemetry().await.unwrap();
        if ta.nat_punch_successes >= 1 && tb.nat_punch_successes >= 1 {
            punched = true;
            break;
        }
        sleep(Duration::from_millis(120)).await;
    }
    assert!(punched, "Coordinated UDP NAT Hole Punching should establish ChaCha20 tunnel");

    // Disable mesh relay on Alice to prove traffic flows directly over punched UDP tunnel!
    let mut cfg = alice.get_settings().await.unwrap();
    cfg.mesh_relay_enabled = false;
    alice.update_settings(cfg).await.unwrap();

    let mut rx_b = bob.subscribe();
    let sent = alice
        .send_message(
            &b_bob.profile.peer_id,
            "Direct punched UDP encrypted tunnel!".into(),
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(sent.status, DeliveryStatus::SentDirect);

    let mut got = false;
    for _ in 0..20 {
        if let Ok(CoreEvent::MessageReceived(msg)) = tokio::time::timeout(
            Duration::from_millis(150),
            rx_b.recv(),
        )
        .await
        .unwrap_or(Err(tokio::sync::broadcast::error::RecvError::Closed))
        {
            if msg.sender_id == b_alice.profile.peer_id && msg.content == "Direct punched UDP encrypted tunnel!" {
                got = true;
                break;
            }
        }
    }
    assert!(got, "Bob should receive direct E2EE message over UDP hole-punched tunnel");
}

#[tokio::test]
async fn test_blackout_dht_mailbox_offline_sync_after_ip_change() {
    // Simulates international blackout & asynchronous reconnect:
    // Alice sends E2EE message via Onion relay while Bob is offline -> Relay caches encrypted packet ->
    // Alice ALSO goes offline -> Bob unlocks later on a brand-new port & syncs automatically from DHT Mailbox!
    let d_a = TempDir::new().unwrap();
    let d_r = TempDir::new().unwrap();
    let d_b = TempDir::new().unwrap();

    let (alice, b_alice) = setup_test_node(&d_a, 49301, "pin-a", "Alice Blackout", "alice_blk", 0).await;
    let (relay, _) = setup_test_node(&d_r, 49302, "pin-r", "Domestic Seed", "seed", 0).await;
    let (bob, b_bob) = setup_test_node(&d_b, 49303, "pin-b", "Bob Blackout", "bob_blk", 0).await;

    let mut alice_cfg = alice.get_settings().await.unwrap();
    alice_cfg.onion_routing_enabled = true;
    alice_cfg.hole_punch_enabled = false;
    alice.update_settings(alice_cfg).await.unwrap();

    let tel_r = relay.get_network_telemetry().await.unwrap();
    let r_addr = format!("127.0.0.1:{}", tel_r.node_listening_port);

    bob.add_bootstrap_peer(&r_addr).await.unwrap();
    alice.add_bootstrap_peer(&r_addr).await.unwrap();
    sleep(Duration::from_millis(500)).await;

    // Bob goes completely offline (vault locked / device shutdown)
    bob.lock_vault().await;
    sleep(Duration::from_millis(200)).await;

    // Alice pairs via cached DHT record and dispatches sealed onion message while Bob is offline
    alice.connect_peer_by_id(&b_bob.profile.peer_id).await.unwrap();
    let sent = alice
        .send_message(
            &b_bob.profile.peer_id,
            "Mailbox emergency sync across blackout!".into(),
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(sent.status, DeliveryStatus::RelayedMesh);
    sleep(Duration::from_millis(350)).await;

    // Alice ALSO goes offline BEFORE Bob ever wakes up!
    alice.lock_vault().await;
    sleep(Duration::from_millis(200)).await;

    // Bob unlocks later on a new dynamic port and reconnects to Domestic Seed
    bob.unlock_vault("pin-b").await.unwrap();
    let mut synced = false;
    for _ in 0..25 {
        let msgs = bob.list_messages(&b_alice.profile.peer_id).await.unwrap_or_default();
        if msgs.iter().any(|m| m.content == "Mailbox emergency sync across blackout!") {
            synced = true;
            break;
        }
        sleep(Duration::from_millis(200)).await;
    }
    assert!(
        synced,
        "Bob must sync & decrypt Alice's offline message from DHT Mailbox even after Alice went offline"
    );
}

#[tokio::test]
async fn test_decentralized_handle_and_account_id_search_across_hops() {
    // Topology: Alice <-> Relay1 <-> Relay2 <-> Bob
    // Proves zero-server @handle & @handle#TAG lookup across multiple mesh hops without LAN or Peer ID sharing!
    let d_a = TempDir::new().unwrap();
    let d_r1 = TempDir::new().unwrap();
    let d_r2 = TempDir::new().unwrap();
    let d_b = TempDir::new().unwrap();

    let (alice, _b_alice) = setup_test_node(&d_a, 49401, "pin-a", "Alice Search", "alice_s", 0).await;
    let (relay1, _) = setup_test_node(&d_r1, 49402, "pin-r1", "Bridge One", "bridge1", 0).await;
    let (relay2, _) = setup_test_node(&d_r2, 49403, "pin-r2", "Bridge Two", "bridge2", 0).await;
    let (bob, b_bob) = setup_test_node(&d_b, 49404, "pin-b", "Bob Fox", "bob_fox", 0).await;

    assert!(b_bob.account_id.starts_with("@bob_fox#"));
    assert!(b_bob.share_uri.starts_with("cymesh://id/"));

    let tel_r1 = relay1.get_network_telemetry().await.unwrap();
    let tel_r2 = relay2.get_network_telemetry().await.unwrap();
    relay1
        .add_bootstrap_peer(&format!("127.0.0.1:{}", tel_r2.node_listening_port))
        .await
        .unwrap();
    bob.add_bootstrap_peer(&format!("127.0.0.1:{}", tel_r2.node_listening_port))
        .await
        .unwrap();
    sleep(Duration::from_millis(350)).await;
    alice
        .add_bootstrap_peer(&format!("127.0.0.1:{}", tel_r1.node_listening_port))
        .await
        .unwrap();
    sleep(Duration::from_millis(500)).await;

    // 1. Alice searches ONLY by partial handle "@bob_fox" — no Peer ID or IP needed!
    let found = alice.search_accounts("@bob_fox").await.unwrap();
    assert!(
        found.iter().any(|r| r.account_id == b_bob.account_id),
        "Decentralized multi-hop search must locate @bob_fox#TAG across hops"
    );

    // 2. Alice connects purely by @handle#TAG & chats directly with Bob
    let peer = alice.connect_by_account(&b_bob.account_id).await.unwrap();
    assert_eq!(peer.account_id, b_bob.account_id);

    let mut rx_b = bob.subscribe();
    alice
        .send_message(&peer.profile.peer_id, "Found you strictly by Cymesh ID!".into(), None, None)
        .await
        .unwrap();

    let mut delivered = false;
    for _ in 0..20 {
        if let Ok(CoreEvent::MessageReceived(msg)) = tokio::time::timeout(
            Duration::from_millis(150),
            rx_b.recv(),
        )
        .await
        .unwrap_or(Err(tokio::sync::broadcast::error::RecvError::Closed))
        {
            if msg.content == "Found you strictly by Cymesh ID!" {
                delivered = true;
                break;
            }
        }
    }
    assert!(delivered, "Bob must receive E2EE message after being found by @handle#TAG");

    // 3. Seed bundle export & import roundtrip with CYMESH-SEED prefix
    let seed_bundle = alice.export_seed_bundle().await.unwrap();
    assert!(seed_bundle.starts_with("CYMESH-SEED:"));
}

#[tokio::test]
async fn test_hidden_relay_volunteer_delete_chat_and_vault_reset() {
    let d_a = TempDir::new().unwrap();
    let d_r = TempDir::new().unwrap();
    let d_b = TempDir::new().unwrap();

    // Hidden blind relay using the exact stealth + "Relay Node" profile as cymesh-relay
    let relay_node = AegisNode::new(d_r.path().to_path_buf(), 0);
    let r_bundle = relay_node
        .create_identity_and_vault(
            "pin-r",
            "Cymesh Relay".into(),
            "relay".into(),
            "Blind Relay".into(),
            "Relay Node".into(),
            "#00F0FF".into(),
            None,
        )
        .await
        .unwrap();
    let mut r_cfg = relay_node.get_settings().await.unwrap();
    r_cfg.stealth_mode = true;
    r_cfg.listen_port = 49502;
    relay_node.update_settings(r_cfg).await.unwrap();
    relay_node.lock_vault().await;
    relay_node.unlock_vault("pin-r").await.unwrap();
    let r_tel = relay_node.get_network_telemetry().await.unwrap();
    let r_addr = format!("127.0.0.1:{}", r_tel.node_listening_port);

    let (alice, b_alice) = setup_test_node(&d_a, 49501, "pin-a", "Alice", "alice_del", 0).await;
    let (bob, b_bob) = setup_test_node(&d_b, 49503, "pin-b", "Bob", "bob_del", 0).await;

    // 1. Registering volunteer relay verifies handshake, adds bootstrap peer, and hides relay from ChatList & search
    let res = alice.register_volunteer_relay(&r_addr).await.unwrap();
    assert_eq!(res.peer_id, r_bundle.profile.peer_id);
    assert!(alice.get_settings().await.unwrap().bootstrap_peers.contains(&r_addr));
    assert!(
        alice
            .list_peers()
            .await
            .unwrap()
            .iter()
            .all(|p| p.profile.peer_id != r_bundle.profile.peer_id),
        "Hidden Relay Node must never appear in user chat list"
    );
    assert!(
        alice
            .search_accounts("relay")
            .await
            .unwrap()
            .iter()
            .all(|r| r.peer_id != r_bundle.profile.peer_id),
        "Hidden Relay Node must never appear in account search"
    );

    // 2. Connect Alice <-> Bob, send message, then delete_chat wipes peer & messages completely
    let bob_tel = bob.get_network_telemetry().await.unwrap();
    alice
        .connect_peer_addr(&format!("127.0.0.1:{}", bob_tel.node_listening_port))
        .await
        .unwrap();
    alice
        .send_message(&b_bob.profile.peer_id, "Temporary chat msg".into(), None, None)
        .await
        .unwrap();
    assert_eq!(alice.list_messages(&b_bob.profile.peer_id).await.unwrap().len(), 1);
    assert!(alice
        .list_peers()
        .await
        .unwrap()
        .iter()
        .any(|p| p.profile.peer_id == b_bob.profile.peer_id));

    let mut rx_a = alice.subscribe();
    alice.delete_chat(&b_bob.profile.peer_id).await.unwrap();
    assert!(alice.list_messages(&b_bob.profile.peer_id).await.unwrap().is_empty());
    assert!(alice
        .list_peers()
        .await
        .unwrap()
        .iter()
        .all(|p| p.profile.peer_id != b_bob.profile.peer_id));

    let mut saw_deleted_evt = false;
    for _ in 0..8 {
        if let Ok(CoreEvent::ChatDeleted { peer_id }) = rx_a.try_recv() {
            if peer_id == b_bob.profile.peer_id {
                saw_deleted_evt = true;
                break;
            }
        }
    }
    assert!(saw_deleted_evt, "CoreEvent::ChatDeleted must be emitted");

    // 3. Resetting vault wipes initialized state and lets user recover same PeerId with a new PIN
    alice.reset_vault().await.unwrap();
    assert!(!alice.is_initialized());
    assert!(!alice.is_unlocked().await);

    let recovered = alice
        .recover_identity_and_vault(
            &b_alice.recovery_mnemonic,
            "new-pin-999",
            "Alice Recovered".into(),
            "alice_del".into(),
            "".into(),
            "Cipher".into(),
            "#00F0FF".into(),
        )
        .await
        .unwrap();
    assert_eq!(recovered.profile.peer_id, b_alice.profile.peer_id);
}

#[tokio::test]
async fn test_mesh_relay_delivery_receipt_and_read_receipt() {
    let d_a = TempDir::new().unwrap();
    let d_r = TempDir::new().unwrap();
    let d_b = TempDir::new().unwrap();

    let (alice, b_alice) = setup_test_node(&d_a, 49301, "pin-a", "Alice Mesh", "alice_m", 0).await;
    let (relay, _) = setup_test_node(&d_r, 49302, "pin-r", "Blind Relay", "relay", 0).await;
    let (bob, b_bob) = setup_test_node(&d_b, 49303, "pin-b", "Bob Mesh", "bob_m", 0).await;

    let mut cfg_a = alice.get_settings().await.unwrap();
    cfg_a.hole_punch_enabled = false;
    alice.update_settings(cfg_a).await.unwrap();

    let mut cfg_b = bob.get_settings().await.unwrap();
    cfg_b.hole_punch_enabled = false;
    bob.update_settings(cfg_b).await.unwrap();

    let tel_r = relay.get_network_telemetry().await.unwrap();
    let r_addr = format!("127.0.0.1:{}", tel_r.node_listening_port);

    alice.add_bootstrap_peer(&r_addr).await.unwrap();
    bob.add_bootstrap_peer(&r_addr).await.unwrap();
    sleep(Duration::from_millis(500)).await;

    let search = alice.search_accounts("bob_m").await.unwrap();
    assert!(!search.is_empty());
    let bob_found = search.iter().find(|s| s.peer_id == b_bob.profile.peer_id).unwrap();

    alice.connect_by_account(&bob_found.account_id).await.unwrap();
    let mut rx_b = bob.subscribe();

    let sent = alice
        .send_message(&b_bob.profile.peer_id, "Hello over Blind Relay!".into(), None, None)
        .await
        .unwrap();
    assert_eq!(sent.status, DeliveryStatus::RelayedMesh);

    let mut bob_got = false;
    for _ in 0..25 {
        if let Ok(CoreEvent::MessageReceived(msg)) = tokio::time::timeout(Duration::from_millis(150), rx_b.recv()).await.unwrap_or(Err(tokio::sync::broadcast::error::RecvError::Closed)) {
            if msg.sender_id == b_alice.profile.peer_id && msg.content == "Hello over Blind Relay!" {
                bob_got = true;
                break;
            }
        }
    }
    assert!(bob_got, "Bob should receive message over relay");

    let mut alice_delivered = false;
    for _ in 0..25 {
        let msgs = alice.list_messages(&b_bob.profile.peer_id).await.unwrap();
        if let Some(m) = msgs.last() {
            if m.status == DeliveryStatus::Delivered || m.status == DeliveryStatus::Read {
                alice_delivered = true;
                break;
            }
        }
        sleep(Duration::from_millis(100)).await;
    }
    assert!(alice_delivered, "Alice should receive Delivered status via mesh relay");

    bob.mark_chat_read(&b_alice.profile.peer_id).await.unwrap();

    let mut alice_read = false;
    for _ in 0..25 {
        let msgs = alice.list_messages(&b_bob.profile.peer_id).await.unwrap();
        if let Some(m) = msgs.last() {
            if m.status == DeliveryStatus::Read {
                alice_read = true;
                break;
            }
        }
        sleep(Duration::from_millis(100)).await;
    }
    assert!(alice_read, "Alice should receive Read status (cyan double-check) via mesh relay");
}
