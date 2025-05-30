// --- local_log_server/src/p2p/behaviour.rs ---
use libp2p::{
    autonat,
    dcutr,
    identify,
    kad::{self, store::MemoryStore},
    request_response,
    // relay, // Only needed if server *acts* as a relay explicitly
    swarm::NetworkBehaviour,
};
use std::iter;

use super::protocol::{LogBatchRequest, LogBatchResponse, LogSyncCodec, LogSyncProtocol};

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "ServerBehaviourEvent")]
pub struct ServerBehaviour {
    pub request_response: request_response::Behaviour<LogSyncCodec>,
    pub kademlia: kad::Behaviour<MemoryStore>,
    pub identify: identify::Behaviour,
    pub dcutr: dcutr::Behaviour,
    pub autonat: autonat::Behaviour,
    // If the server should act as a public relay:
    // pub relay_server: libp2p::relay::Behaviour,
}

#[derive(Debug)]
pub enum ServerBehaviourEvent {
    RequestResponse(request_response::Event<LogBatchRequest, LogBatchResponse>),
    Kademlia(kad::Event),
    Identify(identify::Event),
    Dcutr(dcutr::Event),
    Autonat(autonat::Event),
    // RelayServer(libp2p::relay::Event), // If relay_server is enabled
}

impl From<request_response::Event<LogBatchRequest, LogBatchResponse>> for ServerBehaviourEvent {
    fn from(event: request_response::Event<LogBatchRequest, LogBatchResponse>) -> Self {
        ServerBehaviourEvent::RequestResponse(event)
    }
}
impl From<kad::Event> for ServerBehaviourEvent {
    fn from(event: kad::Event) -> Self {
        ServerBehaviourEvent::Kademlia(event)
    }
}
impl From<identify::Event> for ServerBehaviourEvent {
    fn from(event: identify::Event) -> Self {
        ServerBehaviourEvent::Identify(event)
    }
}
impl From<dcutr::Event> for ServerBehaviourEvent {
    fn from(event: dcutr::Event) -> Self {
        ServerBehaviourEvent::Dcutr(event)
    }
}
impl From<autonat::Event> for ServerBehaviourEvent {
    fn from(event: autonat::Event) -> Self {
        ServerBehaviourEvent::Autonat(event)
    }
}
// impl From<libp2p::relay::Event> for ServerBehaviourEvent { // If relay_server is enabled
//     fn from(event: libp2p::relay::Event) -> Self {
//         ServerBehaviourEvent::RelayServer(event)
//     }
// }

impl ServerBehaviour {
    pub fn new(
        local_peer_id: libp2p::PeerId,
        identify_config: identify::Config,
        kad_config: kad::Config, // Pass Kademlia config
        // relay_server_config: Option<libp2p::relay::Config>, // If acting as relay
        autonat_config: autonat::Config,
    ) -> Self {
        // Kademlia
        let store = MemoryStore::new(local_peer_id);
        let kademlia = kad::Behaviour::with_config(local_peer_id, store, kad_config);

        // Request-Response
        let rr_protocols = iter::once((
            LogSyncProtocol::default(),
            request_response::ProtocolSupport::Full,
        ));
        let rr_cfg = request_response::Config::default(); // Configure timeouts etc. if needed
        let request_response =
            request_response::Behaviour::<LogSyncCodec>::new(rr_protocols, rr_cfg);

        // Identify
        let identify = identify::Behaviour::new(identify_config);

        // DCUtR
        let dcutr = dcutr::Behaviour::new(local_peer_id);

        // AutoNAT
        let autonat = autonat::Behaviour::new(local_peer_id, autonat_config);

        // Relay Server (optional)
        // let relay_server = relay_server_config
        //     .map(|config| libp2p::relay::Behaviour::new(local_peer_id, config))
        //     .unwrap_or_else(|| { /* dummy or error if mandatory */ panic!("Relay config needed") });
        // For now, not acting as a public relay server by default.

        ServerBehaviour {
            request_response,
            kademlia,
            identify,
            dcutr,
            autonat,
            // relay_server, // If enabled
        }
    }
}
