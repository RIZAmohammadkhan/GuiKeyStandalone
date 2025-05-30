// src/p2p/behaviour.rs

use libp2p::{
    autonat,
    dcutr, // Keep this for dcutr::Behaviour and dcutr::Event later
    identify,
    kad::{self, Config as KademliaConfig, store::MemoryStore}, // KademliaProtocolName not needed if using default
    request_response,
    relay::client::{
        self as relay_client_module, // Alias the module
        Behaviour as RelayClientBehaviour,
        // Event is handled in ClientBehaviourEvent
        // Transport is handled by SwarmBuilder now
    },
    swarm::NetworkBehaviour,
    PeerId,
    // StreamProtocol, // Not strictly needed if kad::PROTOCOL_NAME is directly compatible
};
use tokio::time::Duration;
use std::iter;

use super::protocol::{
    LogSyncCodec,
    LogBatchRequest,
    LogBatchResponse,
    LogSyncProtocol,
};

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "ClientBehaviourEvent")]
pub struct ClientBehaviour {
    pub request_response: request_response::Behaviour<LogSyncCodec>,
    pub kademlia:         kad::Behaviour<MemoryStore>,
    pub identify:         identify::Behaviour,
    pub relay_client:     RelayClientBehaviour,
    pub dcutr:            dcutr::Behaviour, // This is libp2p::dcutr::Behaviour
    pub autonat:          autonat::Behaviour, // This is libp2p::autonat::Behaviour
}

#[derive(Debug)]
pub enum ClientBehaviourEvent {
    RequestResponse(request_response::Event<LogBatchRequest, LogBatchResponse>),
    Kademlia(kad::Event),
    Identify(identify::Event),
    RelayClient(relay_client_module::Event), // Use aliased module for event
    Dcutr(libp2p::dcutr::Event),       // Use full path
    Autonat(libp2p::autonat::Event),   // Use full path
}

// --- From implementations ---
impl From<request_response::Event<LogBatchRequest, LogBatchResponse>> for ClientBehaviourEvent {
    fn from(e: request_response::Event<LogBatchRequest, LogBatchResponse>) -> Self { ClientBehaviourEvent::RequestResponse(e) }
}
impl From<kad::Event> for ClientBehaviourEvent {
    fn from(e: kad::Event) -> Self { ClientBehaviourEvent::Kademlia(e) }
}
impl From<identify::Event> for ClientBehaviourEvent {
    fn from(e: identify::Event) -> Self { ClientBehaviourEvent::Identify(e) }
}
impl From<relay_client_module::Event> for ClientBehaviourEvent { // Use aliased module
    fn from(e: relay_client_module::Event) -> Self { ClientBehaviourEvent::RelayClient(e) }
}
impl From<libp2p::dcutr::Event> for ClientBehaviourEvent {
    fn from(e: libp2p::dcutr::Event) -> Self { ClientBehaviourEvent::Dcutr(e) }
}
impl From<libp2p::autonat::Event> for ClientBehaviourEvent {
    fn from(e: libp2p::autonat::Event) -> Self { ClientBehaviourEvent::Autonat(e) }
}

impl ClientBehaviour {
    pub fn new(
        local_peer_id: PeerId,
        identify_config: identify::Config,
        relay_client_behaviour: RelayClientBehaviour,
    ) -> Self {
        // Kademlia
        let store = MemoryStore::new(local_peer_id);
        // KademliaConfig::default() should set the standard protocol name.
        // The error `no method named set_protocol_name` confirms this.
        // If you need to customize other Kademlia parameters, do it on kad_cfg.
        let kad_cfg = KademliaConfig::default();
        // For example: kad_cfg.set_query_timeout(Duration::from_secs(60));
        let kademlia = kad::Behaviour::with_config(local_peer_id, store, kad_cfg);

        // Request-Response
        let rr_protocols = iter::once((
            LogSyncProtocol::default(),
            request_response::ProtocolSupport::Full,
        ));
        let rr_cfg = request_response::Config::default()
            .with_request_timeout(Duration::from_secs(45));
        let request_response =
            request_response::Behaviour::<LogSyncCodec>::new(rr_protocols, rr_cfg);

        // Identify
        let identify = identify::Behaviour::new(identify_config);

        // DCUtR
        let dcutr = dcutr::Behaviour::new(local_peer_id);

        // AutoNAT
        let autonat_cfg = autonat::Config {
            boot_delay:     Duration::from_secs(15),
            retry_interval: Duration::from_secs(60),
            ..Default::default()
        };
        let autonat = autonat::Behaviour::new(local_peer_id, autonat_cfg);

        ClientBehaviour {
            request_response,
            kademlia,
            identify,
            relay_client: relay_client_behaviour,
            dcutr,
            autonat,
        }
    }
}