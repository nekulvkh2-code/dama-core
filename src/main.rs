use libp2p::{
    core::upgrade,
    identify,
    identity,
    kad::{record::store::MemoryStore, Kademlia, KademliaEvent},
    noise, tcp, yamux,
    swarm::{NetworkBehaviour, SwarmEvent, SwarmBuilder},
    PeerId, Multiaddr,
};
use std::error::Error;
use futures::StreamExt;
use serde::{Serialize, Deserialize};

// --- СТРУКТУРА СООБЩЕНИЯ (DAG логика) ---
#[derive(Serialize, Deserialize, Debug)]
struct MessageNode {
    text: String,
    parent_hash: String, // Ссылка на предыдущее сообщение
    sender: String,
}

// --- ПОВЕДЕНИЕ УЗЛА (Kademlia для поиска + Identify для обмена ID) ---
#[derive(NetworkBehaviour)]
struct DamaBehaviour {
    kademlia: Kademlia<MemoryStore>,
    identify: identify::Behaviour,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // 1. Генерация ключей
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    println!("--- DAMA CORE: АКТИВАЦИЯ ---");
    println!("Ваш ID: {}", local_peer_id);

    // 2. Настройка Kademlia (хранение маршрутов в памяти)
    let store = MemoryStore::new(local_peer_id);
    let kademlia = Kademlia::new(local_peer_id, store);
    let identify = identify::Behaviour::new(identify::Config::new(
        "/dama/1.0.0".into(),
        local_key.public(),
    ));

    let behaviour = DamaBehaviour { kademlia, identify };

    // 3. Сборка Swarm (Транспорт + Поведение)
    let mut swarm = SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|_| behaviour)?
        .build();

    // 4. Запуск прослушивания
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    println!("Поиск соседей запущен...");

    // 5. Цикл обработки событий
    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => {
                println!("Узел слушает на: {}", address);
            }
            // Когда Kademlia находит кого-то
            SwarmEvent::Behaviour(DamaBehaviourEvent::Kademlia(KademliaEvent::RoutingUpdated { peer, .. })) => {
                println!("Обнаружен новый узел в сети: {}", peer);
            }
            _ => {}
        }
    }
}
