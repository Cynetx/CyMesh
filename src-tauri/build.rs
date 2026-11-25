use std::net::SocketAddrV4;

fn main() {
    println!("cargo:rerun-if-changed=icons/icon.ico");
    println!("cargo:rerun-if-changed=icons/icon.png");
    println!("cargo:rerun-if-changed=tauri.conf.json");
    println!("cargo:rerun-if-env-changed=CYMESH_BUILDER_RELAY");
    println!("cargo:rerun-if-env-changed=CYMESH_BUILDER_PROFILE");

    match (
        std::env::var("CYMESH_BUILDER_RELAY").ok(),
        std::env::var("CYMESH_BUILDER_PROFILE").ok(),
    ) {
        (Some(relay), Some(profile)) => {
            let relay: SocketAddrV4 = relay.parse().expect("Expected a relay IPv4 address and port");
            assert!(
                relay.port() != 0
                    && !relay.ip().is_unspecified()
                    && !relay.ip().is_multicast()
                    && !relay.ip().is_broadcast(),
                "Expected a reachable unicast relay address and a nonzero port"
            );
            assert!(
                profile.strip_prefix("relay-").is_some_and(|id| {
                    id.len() == 12
                        && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                }),
                "Invalid relay builder profile ID"
            );
            assert!(
                std::env::var("TAURI_CONFIG").is_ok(),
                "Run the relay builder to configure the app identifier"
            );
            println!("cargo:rustc-env=CYMESH_CLIENT_RELAY={relay}");
            println!("cargo:rustc-env=CYMESH_CLIENT_PROFILE={profile}");
        }
        (None, None) => {}
        _ => panic!("Run the relay builder to set both the relay address and profile ID"),
    }

    tauri_build::build();
}
