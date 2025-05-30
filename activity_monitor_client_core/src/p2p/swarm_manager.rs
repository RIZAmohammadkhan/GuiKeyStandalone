// src/p2p/swarm_manager.rs

use std::{error::Error, sync::Arc, time::Duration};

use futures::StreamExt; // for select_next_some()
use tokio::sync::{mpsc, watch, oneshot};

use libp2p::{
    autonat,
    core::{upgrade, transport::OrTransport},
    dcutr,
    dns::tokio::Transport as DnsTransport,
    identity::Keypair,
    identify::{Behaviour as IdentifyBehaviour, Config as IdentifyConfig},
    kad::{store::MemoryStore, Behaviour as KademliaBehaviour},
    noise::Config as NoiseConfig,
    relay::client as relay_client,
    request_response::{Behaviour as ReqRespBehaviour, Config as ReqRespConfig, ProtocolSupport},
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp::tokio::Transport as TcpTransport,
    yamux::Config as YamuxConfig,
    Multiaddr, PeerId, Transport,
};
use libp2p::SwarmBuilder; // <-- correct location of SwarmBuilder

use crate::{
    app_config::Settings,
    errors::AppError,
    p2p::{
        behaviour::{ClientBehaviour, ClientBehaviourEvent},
        protocol::{LogBatchRequest, LogBatchResponse, LogSyncCodec, LogSyncProtocol},
    },
};

/// Commands sent _into_ the SwarmManager.
#[derive(Debug)]
pub enum SwarmCommand {
    DialPeer { peer: PeerId, addr: Multiaddr },
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
) -> Result<(), Box<dyn Error>> {
    // 1) Identity
    let id_keys = Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(id_keys.public());
    tracing::info!("SwarmManager: PeerId = {:?}", local_peer_id);

    // 2) Transport
    let tcp = TcpTransport::default();
    let dns_tcp = DnsTransport::system(tcp)?;              
    let (relay_transport, relay_behaviour) = relay_client::new(local_peer_id);

    let noise_config = NoiseConfig::new(&id_keys)?;       
    let transport = OrTransport::new(relay_transport, dns_tcp)
        .upgrade(upgrade::Version::V1)
        .authenticate(noise_config)
        .multiplex(YamuxConfig::default())
        .boxed();

    // 3) Behaviours
    let identify_cfg = IdentifyConfig::new("/logsync/1.0.0".into(), id_keys.public());
    let identify_behaviour = IdentifyBehaviour::new(identify_cfg.clone());

    let kademlia_behaviour = KademliaBehaviour::new(local_peer_id, MemoryStore::new(local_peer_id));

    let rr_protocols = std::iter::once((LogSyncProtocol::default(), ProtocolSupport::Full));
    let rr_cfg = ReqRespConfig::default().with_request_timeout(Duration::from_secs(30));
    let request_response_behaviour = ReqRespBehaviour::new(rr_protocols, rr_cfg);

    let dcutr_behaviour = dcutr::Behaviour::new(local_peer_id);
    let autonat_behaviour = autonat::Behaviour::new(local_peer_id, Default::default());

    let behaviour = ClientBehaviour::new(
        local_peer_id,
        relay_behaviour,
        identify_behaviour,
        kademlia_behaviour,
        request_response_behaviour,
        dcutr_behaviour,
        autonat_behaviour,
    );

    // 4) Swarm
    let mut swarm = SwarmBuilder::with_tokio_executor(transport, behaviour, local_peer_id)
        .idle_connection_timeout(Duration::from_secs(60))
        .build();

    for addr_str in &settings.listen_addresses {
        let addr: Multiaddr = addr_str.parse()?;
        swarm.listen_on(addr)?;
    }

    // 5) Loop
    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    tracing::info!("Shutting down swarm");
                    break;
                }
            }
            cmd = cmd_rx.recv() => match cmd {
                Some(SwarmCommand::DialPeer { peer, addr }) => {
                    tracing::info!("DialPeer: {:?} @ {:?}", peer, addr);
                    let _ = swarm.dial_addr(addr);
                }
                Some(SwarmCommand::SendLogBatch { target_peer_id, request, responder }) => {
                    tracing::info!("SendLogBatch to {:?}", target_peer_id);
                    // TODO: integrate with request_response behaviour
                    let _ = responder.send(Err(AppError::Internal("Unimplemented".into())));
                }
                None => { break; }
            },
            event = swarm.select_next_some() => match event {
                SwarmEvent::Behaviour(ClientBehaviourEvent::Identify(e))       => tracing::debug!("Identify: {:?}", e),
                SwarmEvent::Behaviour(ClientBehaviourEvent::Kademlia(e))       => tracing::debug!("Kademlia: {:?}", e),
                SwarmEvent::Behaviour(ClientBehaviourEvent::RequestResponse(e))=> tracing::debug!("ReqResp: {:?}", e),
                SwarmEvent::Behaviour(ClientBehaviourEvent::RelayClient(e))   => tracing::debug!("Relay: {:?}", e),
                SwarmEvent::Behaviour(ClientBehaviourEvent::Dcutr(e))         => tracing::debug!("DCUtR: {:?}", e),
                SwarmEvent::Behaviour(ClientBehaviourEvent::Autonat(e))       => tracing::debug!("AutoNAT: {:?}", e),
                SwarmEvent::NewListenAddr { address, .. }                     => tracing::info!("Listening on {:?}", address),
                _                                                              => {}
            },
        }
    }

    Ok(())
}
