// src/p2p/swarm_manager.rs

use std::{collections::HashMap, error::Error, sync::Arc, time::Duration};

use futures::StreamExt; // for select_next_some()
use tokio::sync::{mpsc, watch, oneshot};

use libp2p::{
    core::{upgrade, transport::OrTransport},
    dns::tokio::Transport as DnsTransport,
    identity::Keypair,
    identify::Config as IdentifyConfig,
    relay::client as relay_client,
    swarm::SwarmEvent,
    tcp::tokio::Transport as TcpTransport,
    Multiaddr, PeerId, Transport, Swarm,
};
// Explicit noise and yamux imports for libp2p 0.55 direct usage
use libp2p::noise;
use libp2p::yamux;

use crate::{
    app_config::Settings,
    errors::AppError,
    p2p::{
        behaviour::{ClientBehaviour, ClientBehaviourEvent},
        protocol::{LogBatchRequest, LogBatchResponse},
    },
};

/// Commands sent _into_ the SwarmManager.
#[derive(Debug)]
pub enum SwarmCommand {
    DialPeer { peer: PeerId, addr: Multiaddr }, // Mostly for testing or explicit connection management
    SendLogBatch {
        target_peer_id: PeerId,
        request: LogBatchRequest,
        responder: oneshot::Sender<Result<LogBatchResponse, AppError>>,
    },
}

/// Drive the P2P subsystem. Called from `main.rs`.
pub async fn run_swarm_manager(
    settings: Arc<Settings>,
    mut cmd_rx: mpsc::Receiver<SwarmCommand>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<(), Box<dyn Error>> { // Using Box<dyn Error> for broader error compatibility
    // 1) Identity
    // For a real application, you'd likely load/save this keypair to maintain a stable PeerId.
    let id_keys = Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(id_keys.public());
    tracing::info!("SwarmManager: Local PeerId = {:?}", local_peer_id);

    // 2) Transport
    let tcp_transport_config = libp2p::tcp::Config::default().nodelay(true);
    let tcp_transport = TcpTransport::new(tcp_transport_config);
    let dns_tcp_transport = DnsTransport::system(tcp_transport)?; // Remove .await

    let (relay_client_transport, relay_client_behaviour) =
        relay_client::new(local_peer_id);

    // Noise keys derived from the identity keypair for encryption
    let noise_config = noise::Config::new(&id_keys).expect("Signing noise static keypair failed");

    let transport = OrTransport::new(relay_client_transport, dns_tcp_transport)
        .upgrade(upgrade::Version::V1Lazy)
        .authenticate(noise_config)
        .multiplex(yamux::Config::default())
        .timeout(Duration::from_secs(20))
        .boxed();

    // 3) Create the main Network Behaviour
    // Identify protocol configuration
    let identify_config = IdentifyConfig::new(
        // Protocol version string should be unique to your application
        format!("/guikey_standalone-client/0.1.0/{}", settings.client_id),
        id_keys.public().clone(),
    )
    .with_agent_version(format!("activity-monitor-client-core/{}", env!("CARGO_PKG_VERSION")));

    // ClientBehaviour struct encapsulates Kademlia, Request-Response, Identify, Relay, DCUtR, AutoNAT
    let behaviour = ClientBehaviour::new(
        local_peer_id,
        identify_config,        // Pass the identify config
        relay_client_behaviour, // Pass the relay client part of the behaviour
    );

    // 4) Swarm - Use the new constructor approach
    let mut swarm = Swarm::new(
        transport,
        behaviour,
        local_peer_id,
        libp2p::swarm::Config::with_tokio_executor()
            .with_idle_connection_timeout(Duration::from_secs(5 * 60)),
    );

    // Add configured bootstrap nodes to Kademlia's routing table
    for addr in &settings.bootstrap_addresses {
        if let Some(peer_id) = addr.iter().last().and_then(|proto| match proto {
            libp2p::multiaddr::Protocol::P2p(peer_id) => Some(peer_id), // The hash is already a PeerId
            _ => None,
        }) {
            tracing::info!("SwarmManager: Adding bootstrap node to Kademlia: {} @ {}", peer_id, addr);
            swarm.behaviour_mut().kademlia.add_address(&peer_id, addr.clone());
        } else {
            tracing::warn!("SwarmManager: Could not parse PeerId from bootstrap address: {}. It might not be used effectively by Kademlia.", addr);
        }
    }

    // Initiate Kademlia bootstrap if bootstrap nodes are configured
    if !settings.bootstrap_addresses.is_empty() {
        match swarm.behaviour_mut().kademlia.bootstrap() {
            Ok(id) => tracing::info!("SwarmManager: Kademlia bootstrap process initiated with query ID: {:?}", id),
            Err(e) => tracing::warn!("SwarmManager: Kademlia bootstrap failed to start: {:?}", e),
        }
    } else {
        tracing::info!("SwarmManager: No bootstrap addresses configured for Kademlia. Peer discovery may be limited.");
    }

    // Kademlia will attempt to find the server's addresses using its PeerId.
    let server_target_peer_id = settings.server_peer_id;
    tracing::info!("SwarmManager: Kademlia will attempt to find and connect to server PeerId: {}", server_target_peer_id);
    swarm.behaviour_mut().kademlia.get_closest_peers(server_target_peer_id);

    // Store pending outbound request responders
    let mut pending_outbound_log_requests: HashMap<
        libp2p::request_response::OutboundRequestId,
        oneshot::Sender<Result<LogBatchResponse, AppError>>
    > = HashMap::new();

    // 5) Event Loop
    tracing::info!("SwarmManager: Entering main event loop...");
    loop {
        tokio::select! {
            biased; // Prioritize shutdown over other events

            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() { // Check if the new value is true
                    tracing::info!("SwarmManager: Shutdown signal received. Exiting event loop.");
                    break;
                }
            }

            Some(command) = cmd_rx.recv() => {
                match command {
                    SwarmCommand::DialPeer { peer, addr } => {
                        tracing::info!("SwarmManager: CMD DialPeer for {} @ {}", peer, addr);
                        // Add address to Kademlia so it's aware of it
                        swarm.behaviour_mut().kademlia.add_address(&peer, addr.clone());
                        // Attempt to dial the peer
                        if let Err(e) = swarm.dial(peer) {
                             tracing::warn!("SwarmManager: Error initiating dial to {}: {:?}", peer, e);
                        }
                    }
                    SwarmCommand::SendLogBatch { target_peer_id, request, responder } => {
                        tracing::debug!("SwarmManager: CMD SendLogBatch for PeerId: {}", target_peer_id);

                        // Ensure the target is the configured server.
                        // Could be made more flexible if multiple servers were supported.
                        if target_peer_id != server_target_peer_id {
                            tracing::error!(
                                "SwarmManager: Attempt to send log batch to non-configured server PeerId {}. Configured server: {}",
                                target_peer_id, server_target_peer_id
                            );
                            let _ = responder.send(Err(AppError::P2pOperation(
                                "Target peer is not the configured server.".to_string()
                            )));
                            continue; // Skip this command
                        }

                        // The request-response behaviour will handle dialing if not connected,
                        // provided it knows an address for the peer (from Kademlia or Identify).
                        let request_id = swarm.behaviour_mut().request_response.send_request(&target_peer_id, request);
                        tracing::info!("SwarmManager: Sent log batch request (ID: {:?}) to server {}", request_id, target_peer_id);
                        pending_outbound_log_requests.insert(request_id, responder);
                    }
                }
            }

            event = swarm.select_next_some() => {
                // Log all swarm events at trace level for detailed debugging if needed
                // tracing::trace!("SwarmManager: SwarmEvent: {:?}", event);

                match event {
                    SwarmEvent::Behaviour(behaviour_event) => {
                        match behaviour_event {
                            ClientBehaviourEvent::Identify(identify_event) => {
                                if let libp2p::identify::Event::Received { peer_id, info, .. } = identify_event {
                                    tracing::info!("SwarmManager: EVT Identify::Received from: {} with agent: '{}', protocols: {:?}, listen_addrs: {:?}",
                                        peer_id, info.agent_version, info.protocols, info.listen_addrs);
                                    for addr in info.listen_addrs {
                                        swarm.behaviour_mut().kademlia.add_address(&peer_id, addr);
                                    }
                                    if peer_id == server_target_peer_id {
                                        tracing::info!("SwarmManager: Identified target server {}. P2P connection likely established or ready.", peer_id);
                                    }
                                } else if let libp2p::identify::Event::Sent { peer_id, .. } = identify_event {
                                    tracing::debug!("SwarmManager: EVT Identify::Sent to: {}", peer_id);
                                } else if let libp2p::identify::Event::Error { peer_id, error, .. } = identify_event {
                                    tracing::warn!("SwarmManager: EVT Identify::Error with peer {}: {:?}", peer_id, error);
                                } // Other Identify events (Push, Pushed) can be logged if needed
                            }
                            ClientBehaviourEvent::Kademlia(kad_event) => {
                                // Kademlia events can be very verbose. Log selectively.
                                match &kad_event {
                                    libp2p::kad::Event::OutboundQueryProgressed { result, step, .. } => {
                                        if step.last { // Log final results or significant steps
                                            match result {
                                                libp2p::kad::QueryResult::GetClosestPeers(Ok(ok)) => {
                                                    tracing::info!("SwarmManager: EVT Kademlia GetClosestPeers for key {:?} found {} peers.", ok.key, ok.peers.len());
                                                    // Check if server was found in its own lookup - compare PeerIds from PeerInfo
                                                    let peer_ids: Vec<_> = ok.peers.iter().map(|info| info.peer_id).collect();
                                                    if ok.key.as_ref() as &[u8] == server_target_peer_id.to_bytes().as_slice() && !peer_ids.contains(&server_target_peer_id)  {
                                                        tracing::warn!("SwarmManager: Kademlia lookup for server PeerID {} did not return the server itself among closest peers.", server_target_peer_id);
                                                    }
                                                }
                                                
                                                libp2p::kad::QueryResult::GetClosestPeers(Err(err)) => {
                                                    tracing::warn!("SwarmManager: EVT Kademlia GetClosestPeers query failed: {:?}", err);
                                                }
                                                libp2p::kad::QueryResult::Bootstrap(Ok(res)) if res.num_remaining == 0 => {
                                                    tracing::info!("SwarmManager: EVT Kademlia bootstrap COMPLETED. Peer: {}, Num remaining: 0", res.peer);
                                                }
                                                libp2p::kad::QueryResult::Bootstrap(Err(err)) => {
                                                    tracing::warn!("SwarmManager: EVT Kademlia bootstrap query failed: {:?}", err);
                                                }
                                                _ => { tracing::debug!("SwarmManager: EVT Kademlia query progressed (final step): {:?}", result); }
                                            }
                                        }
                                    }
                                    libp2p::kad::Event::RoutingUpdated { peer, addresses, .. } => {
                                        tracing::debug!("SwarmManager: EVT Kademlia routing updated for peer: {}, {} addrs", peer, addresses.len());
                                    }
                                    _ => { tracing::trace!("SwarmManager: EVT Kademlia: {:?}", kad_event); }
                                }
                            }
                            ClientBehaviourEvent::RequestResponse(rr_event) => {
                                match rr_event {
                                    libp2p::request_response::Event::Message { peer, message,.. } => {
                                        match message {
                                            libp2p::request_response::Message::Request { .. } => {
                                                // Client role typically doesn't handle incoming requests in this app
                                                tracing::warn!("SwarmManager: EVT RR: Received unexpected Request from peer {}. Ignoring.", peer);
                                            }
                                            libp2p::request_response::Message::Response { request_id, response } => {
                                                tracing::info!("SwarmManager: EVT RR: Received Response (ID: {:?}) from peer {}: status '{}', msg '{}'",
                                                    request_id, peer, response.status, response.message);
                                                if let Some(responder) = pending_outbound_log_requests.remove(&request_id) {
                                                    let _ = responder.send(Ok(response));
                                                } else {
                                                    tracing::warn!("SwarmManager: EVT RR: Received Response for unknown/timed_out request_id: {:?}", request_id);
                                                }
                                            }
                                        }
                                    }
                                    libp2p::request_response::Event::OutboundFailure { peer, request_id, error, .. } => {
                                        tracing::error!("SwarmManager: EVT RR: OutboundFailure for request_id {:?} to peer {}: {:?}", request_id, peer, error);
                                        if let Some(responder) = pending_outbound_log_requests.remove(&request_id) {
                                            let app_err = match error {
                                                libp2p::request_response::OutboundFailure::Timeout => AppError::P2pOperation(format!("Request to {} timed out", peer)),
                                                libp2p::request_response::OutboundFailure::ConnectionClosed => AppError::P2pOperation(format!("Connection to {} closed", peer)),
                                                libp2p::request_response::OutboundFailure::DialFailure => AppError::P2pOperation(format!("Dial to {} failed", peer)),
                                                libp2p::request_response::OutboundFailure::UnsupportedProtocols => AppError::P2pOperation(format!("Peer {} does not support the protocol", peer)),
                                                _ => AppError::P2pOperation(format!("Request-response outbound failure to {}: {:?}", peer, error)),
                                            };
                                            let _ = responder.send(Err(app_err));
                                        }
                                    }
                                        libp2p::request_response::Event::InboundFailure { peer, request_id, error, .. } => {
                                            // Client role typically doesn't have inbound requests it's waiting for.
                                            tracing::error!("SwarmManager: EVT RR: InboundFailure from peer {} for request {:?}: {:?}. This is unexpected for a client.", peer, request_id, error);
                                        }
                                     _ => {tracing::trace!("SwarmManager: EVT RR: Other event: {:?}", rr_event);}
                                }
                            }
                            ClientBehaviourEvent::RelayClient(relay_event) => {
                                tracing::debug!("SwarmManager: EVT RelayClient: {:?}", relay_event);
                                // Specific relay events can be logged here if needed
                            }
                            ClientBehaviourEvent::Dcutr(dcutr_event) => {
                                tracing::debug!("SwarmManager: EVT DCUtR: {:?}", dcutr_event);
                                // Specific DCUtR events like initiation/completion can be logged
                            }
                        ClientBehaviourEvent::Autonat(autonat_event) => {
                            if let libp2p::autonat::Event::StatusChanged { old, new } = autonat_event {
                                tracing::info!("SwarmManager: EVT AutoNAT status changed from {:?} to: {:?}", old, new);
                            } else {
                                tracing::debug!("SwarmManager: EVT AutoNAT: {:?}", autonat_event);
                            }
                        }
                        }
                    }
                    SwarmEvent::NewListenAddr { address, .. } => {
                        tracing::info!("SwarmManager: EVT Client listening on local address: {}", address);
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, endpoint, established_in, .. } => {
                        tracing::info!(
                            "SwarmManager: EVT Connection established with peer: {} via {:?} in {:?}",
                            peer_id, endpoint.get_remote_address(), established_in
                        );
                        if peer_id == server_target_peer_id {
                            tracing::info!("SwarmManager: Successfully connected to target server {}.", peer_id);
                        }
                    }
                    SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                        tracing::info!("SwarmManager: EVT Connection closed with peer: {}. Cause: {:?}", peer_id, cause.map(|c|c.to_string()));
                    }
                    SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                        tracing::warn!("SwarmManager: EVT Outgoing connection error to peer {:?}: {}", peer_id, error);
                    }
                    SwarmEvent::IncomingConnection { local_addr, send_back_addr, .. } => {
                        tracing::debug!("SwarmManager: EVT Incoming connection attempt from {} to {}", send_back_addr, local_addr);
                    }
                    SwarmEvent::IncomingConnectionError { local_addr, send_back_addr, error, .. } => {
                        tracing::warn!("SwarmManager: EVT Incoming connection error from {} to {}: {}", send_back_addr, local_addr, error);
                    }
                    SwarmEvent::Dialing { peer_id, .. } => {
                        tracing::debug!("SwarmManager: EVT Dialing peer {:?}", peer_id);
                    }
                    // Other SwarmEvents can be logged with trace! if needed
                    _ => { tracing::trace!("SwarmManager: EVT Other SwarmEvent: {:?}", event); }
                }
            }
        }
    }
    Ok(())
}