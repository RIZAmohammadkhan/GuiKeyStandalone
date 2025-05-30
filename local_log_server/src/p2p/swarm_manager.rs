// --- local_log_server/src/p2p/swarm_manager.rs ---
use std::{error::Error, sync::Arc, time::Duration, str::FromStr};
use futures::StreamExt;
use tokio::sync::watch;

use libp2p::{
    core::{upgrade, transport::OrTransport},
    dns::tokio::Transport as DnsTransport,
    identity::{Keypair, ed25519::SecretKey},
    identify::Config as IdentifyConfig,
    kad::{Config as KademliaConfig, store::MemoryStore},
    // relay, // For relay server functionality if enabled
    request_response::ResponseChannel, // Import ResponseChannel
    swarm::SwarmEvent,
    tcp::tokio::Transport as TcpTransport,
    Multiaddr, PeerId, Transport, Swarm,
};
use libp2p::noise;
use libp2p::yamux;

use crate::{
    app_config::ServerSettings,
    application::log_service::LogService,
    errors::ServerError, // Using ServerError for some internal logic reporting
    p2p::{
        behaviour::{ServerBehaviour, ServerBehaviourEvent},
        protocol::{LogBatchResponse, LogSyncCodec, LogSyncProtocol}, // LogSyncCodec and Protocol not directly used in this file's logic but good for context
    },
};


pub async fn run_server_swarm_manager(
    settings: Arc<ServerSettings>,
    log_service: LogService,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<(), Box<dyn Error + Send + Sync>> { // Ensure error type is Send + Sync for tokio::spawn
    
    // 1. Identity
    let secret_key = SecretKey::try_from_bytes(settings.server_identity_key_seed)
        .map_err(|e| Box::new(ServerError::Config(format!("Invalid server identity seed: {}", e))) as Box<dyn Error + Send + Sync>)?;
    let local_key = Keypair::from(libp2p::identity::ed25519::Keypair::from(secret_key));
    let local_peer_id = PeerId::from(local_key.public());
    tracing::info!("Server P2P: Local PeerId = {}", local_peer_id);

    // 2. Transport
    let tcp_transport = TcpTransport::new(libp2p::tcp::Config::default().nodelay(true));
    let dns_tcp_transport = DnsTransport::system(tcp_transport)?;

    let transport = dns_tcp_transport
        .upgrade(upgrade::Version::V1Lazy)
        .authenticate(noise::Config::new(&local_key)?)
        .multiplex(yamux::Config::default())
        .timeout(Duration::from_secs(20))
        .boxed();

    // 3. Create the main Network Behaviour
    let identify_config = IdentifyConfig::new(
        format!("/guikey_standalone-server/0.1.0/{}", local_peer_id),
        local_key.public().clone(),
    )
    .with_agent_version(format!("local-log-server/{}", env!("CARGO_PKG_VERSION")));
    
    let mut kad_config = KademliaConfig::default();
    let autonat_config = libp2p::autonat::Config {
        boot_delay: Duration::from_secs(10),
        refresh_interval: Duration::from_secs(5 * 60),
        ..Default::default()
    };

    let behaviour = ServerBehaviour::new(
        local_peer_id,
        identify_config,
        kad_config,
        autonat_config,
    );

    // 4. Swarm
    let mut swarm = Swarm::new(
        transport,
        behaviour,
        local_peer_id,
        libp2p::swarm::Config::with_tokio_executor()
            .with_idle_connection_timeout(Duration::from_secs(10 * 60)),
    );

    // 5. Listen on configured P2P multiaddress
    swarm.listen_on(settings.p2p_listen_address.clone())?;
    tracing::info!("Server P2P: Attempting to listen on {}", settings.p2p_listen_address);

    tracing::info!("Server P2P: Swarm manager entering main event loop...");
    loop {
        tokio::select! {
            biased; 

            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    tracing::info!("Server P2P: Shutdown signal received. Exiting event loop.");
                    break;
                }
            }

            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::Behaviour(behaviour_event) => {
                        match behaviour_event {
                            ServerBehaviourEvent::Identify(identify_event) => {
                                if let libp2p::identify::Event::Received { peer_id, info, .. } = identify_event {
                                    tracing::info!("Server P2P: Identify Received from: {} ({}), listen_addrs: {:?}",
                                        peer_id, info.agent_version, info.listen_addrs);
                                    for addr in info.listen_addrs {
                                        swarm.behaviour_mut().kademlia.add_address(&peer_id, addr);
                                    }
                                }
                            }
                            ServerBehaviourEvent::Kademlia(kad_event) => {
                                if let libp2p::kad::Event::RoutingUpdated{peer, ..} = kad_event {
                                    tracing::debug!("Server P2P: Kademlia routing updated for peer {}", peer);
                                }
                            }
                            ServerBehaviourEvent::RequestResponse(rr_event) => {
                                match rr_event {
                                    libp2p::request_response::Event::Message { peer, message, .. } => {
                                        if let libp2p::request_response::Message::Request { request, channel, .. } = message {
                                            tracing::info!(
                                                "Server P2P: Received LogBatchRequest from Peer {} (App Client ID: {}), payload size: {}",
                                                peer, request.app_client_id, request.encrypted_log_payload.len()
                                            );
                                            
                                            let log_service_clone = log_service.clone();
                                            // ** CORRECTED PART START **
                                            // We pass the `channel` (ResponseChannel) to the spawned task.
                                            // The `swarm.behaviour_mut().request_response` is NOT moved.
                                            // Instead, we'll need a way to send the response using the swarm
                                            // after the async block. A channel back to the swarm manager loop
                                            // or direct use of `swarm.behaviour_mut().request_response.send_response()`
                                            // if the async block can be avoided or structured differently.
                                            // For simplicity here, we will use a temporary sender to the swarm itself
                                            // if we absolutely must spawn a long-running task.
                                            // However, LogService::ingest_log_batch is already async.
                                            //
                                            // Let's try to keep it simpler:
                                            // The `channel` is a `ResponseChannel<LogBatchResponse>`.
                                            // We need to call `swarm.behaviour_mut().request_response.send_response(channel, response)`
                                            //
                                            // The challenge is `swarm` is mutably borrowed by `select_next_some()`.
                                            // To avoid this, we need to handle the response sending *outside* the
                                            // `tokio::spawn` if possible, or use a command pattern to send the response.
                                            //
                                            // Simpler approach for now: Process the request, then send response.
                                            // If `ingest_log_batch` is truly long, this would block the swarm loop.
                                            // `LogService::ingest_log_batch` involves `web::block` which is for CPU-bound tasks,
                                            // so it *should* be okay to await it here as it offloads.

                                            // Store the channel to send the response later
                                            let response_channel: ResponseChannel<LogBatchResponse> = channel;

                                            // Perform the ingestion (which is async and uses web::block for CPU work)
                                            match log_service_clone.ingest_log_batch(&request.app_client_id, request.encrypted_log_payload).await {
                                                Ok(processed_count) => {
                                                    let response = LogBatchResponse {
                                                        status: "success".to_string(),
                                                        message: format!("Processed {} log events.", processed_count),
                                                        events_processed: processed_count,
                                                    };
                                                    if swarm.behaviour_mut().request_response.send_response(response_channel, response).is_err() {
                                                        tracing::error!("Server P2P: Failed to send success response to peer {}", peer);
                                                    } else {
                                                        tracing::info!("Server P2P: Sent success response ({} events) to peer {}", processed_count, peer);
                                                    }
                                                }
                                                Err(e) => {
                                                    tracing::error!("Server P2P: Error processing log batch from {}: {}", peer, e);
                                                    let response = LogBatchResponse {
                                                        status: "error".to_string(),
                                                        message: format!("Server error processing batch: {}", e),
                                                        events_processed: 0,
                                                    };
                                                    if swarm.behaviour_mut().request_response.send_response(response_channel, response).is_err() {
                                                        tracing::error!("Server P2P: Failed to send error response to peer {}", peer);
                                                    } else {
                                                        tracing::warn!("Server P2P: Sent error response to peer {}: {}", peer, e);
                                                    }
                                                }
                                            }
                                            // ** CORRECTED PART END **
                                        } else if let libp2p::request_response::Message::Response { .. } = message {
                                            tracing::warn!("Server P2P: Received unexpected Response from peer {}. Server should not be sending requests of this type.", peer);
                                        }
                                    }
                                    libp2p::request_response::Event::OutboundFailure { peer, request_id, error, .. } => {
                                        tracing::warn!("Server P2P: OutboundFailure for request_id {:?} to peer {}: {:?} (unexpected for server).", request_id, peer, error);
                                    }
                                    libp2p::request_response::Event::InboundFailure { peer, request_id, error, .. } => {
                                        tracing::error!("Server P2P: InboundFailure processing request {:?} from peer {}: {:?}", request_id, peer, error);
                                    }
                                    _ => {} // Other RR events
                                }
                            }
                            ServerBehaviourEvent::Dcutr(dcutr_event) => {
                                tracing::debug!("Server P2P: DCUtR event: {:?}", dcutr_event);
                            }
                            ServerBehaviourEvent::Autonat(autonat_event) => {
                                if let libp2p::autonat::Event::StatusChanged { old, new } = autonat_event {
                                    tracing::info!("Server P2P: AutoNAT status changed from {:?} to: {:?}", old, new);
                                } else {
                                    tracing::debug!("Server P2P: AutoNAT event: {:?}", autonat_event);
                                }
                            }
                        }
                    }
                    SwarmEvent::NewListenAddr { address, .. } => {
                        tracing::info!("Server P2P: Locally listening on: {}", address);
                    }
                    SwarmEvent::ExternalAddrConfirmed { address } => {
                         tracing::info!("Server P2P: External address confirmed by provider: {}", address);
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                        tracing::info!("Server P2P: Connection established with peer: {} via {:?}", peer_id, endpoint.get_remote_address());
                    }
                    SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                        tracing::info!("Server P2P: Connection with peer {} closed. Cause: {:?}", peer_id, cause.map(|c|c.to_string()));
                    }
                    SwarmEvent::IncomingConnectionError { local_addr, send_back_addr, error, .. } => {
                        tracing::warn!("Server P2P: Incoming connection error from {} to {}: {}", send_back_addr, local_addr, error);
                    }
                    _ => { /* Other SwarmEvents can be logged at trace level */ }
                }
            }
        }
    }
    Ok(())
}