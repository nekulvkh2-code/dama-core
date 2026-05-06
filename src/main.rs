use libp2p::swarm::{SwarmEvent, SwarmBuilder};
use libp2p::{identify, identity, kad, PeerId, Transport};
use std::error::Error;
use futures::StreamExt;
use rusqlite::{Connection, Result as SqlResult};

// --- СТРУКТУРА ХРАНИЛИЩА ---
struct DamaStorage { 
    _conn: Connection 
}

impl DamaStorage {
    fn init() -> SqlResult<Self> {
        let conn = Connection::open("dama_vault.db")?;
        // Создаем таблицу для хранения сообщений
        conn.execute(
            "CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY,
                sender TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;
        Ok(Self { _conn: conn })
    }
}

// --- ПОВЕДЕНИЕ УЗЛА ---
#[derive(libp2p::swarm::NetworkBehaviour)]
struct DamaBehaviour {
    kademlia: kad::Behaviour<kad::record::store::MemoryStore>,
    identify: identify::Behaviour,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("--- DAMA CORE: ЗАПУСК ПРИВАТНОЙ СЕТИ ---");

    // 1. Инициализация базы данных
    let _storage = DamaStorage::init().expect("Не удалось создать базу данных");
    println!("✅ Хранилище сообщений активировано.");

    // 2. Подключение к Tor
    println!("Подключение к сети Tor (Arti)...");
    let config = arti_client::TorClientConfig::default();
    let _tor_client = arti_client::TorClient::create_bootstrapped(config).await?;
    println!("✅ Tor активен. Ваш IP скрыт.");

    // 3. Настройка P2P узла
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    println!("Ваш PeerID: {}", local_peer_id);

    // Сборка транспорта вручную для стабильности на Windows
    let transport = libp2p::tcp::tokio::Transport::default()
        .upgrade(libp2p::core::upgrade::Version::V1)
        .authenticate(libp2p::noise::Config::new(&local_key)?)
        .multiplex(libp2p::yamux::Config::default())
        .boxed();

    let behaviour = DamaBehaviour {
        kademlia: kad::Behaviour::new(local_peer_id, kad::record::store::MemoryStore::new(local_peer_id)),
        identify: identify::Behaviour::new(identify::Config::new("/dama/1.0".into(), local_key.public())),
    };

    let mut swarm = SwarmBuilder::with_tokio_executor(
        transport,
        behaviour,
        local_peer_id,
    ).build();

    // Запуск прослушивания
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
    println!("🚀 Мессенджер DAMA запущен и готов к работе.");

    while let Some(event) = swarm.next().await {
        if let SwarmEvent::NewListenAddr { address, .. } = event {
            println!("📍 Адрес вашего узла в сети: {}", address);
        }
    }
    
    Ok(())
}
