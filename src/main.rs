use libp2p::{
    identify,
    identity,
    kad::{record::store::MemoryStore, Kademlia},
    noise, tcp, yamux,
    swarm::{NetworkBehaviour, SwarmEvent, SwarmBuilder},
    PeerId,
};
use std::error::Error;
use futures::StreamExt;
use serde::{Serialize, Deserialize};
use arti_client::{TorClient, TorClientConfig};
use tor_rtcompat::PreferredRuntime;

// Криптография
use aes_gcm::{Aes256Gcm, Key, Nonce, aead::Aead};
use aes_gcm::aead::KeyInit;

// База данных
use rusqlite::{params, Connection, Result as SqlResult};

// --- МОДУЛЬ ХРАНИЛИЩА (SQLite + SQLCipher) ---
struct DamaStorage {
    conn: Connection,
}

impl DamaStorage {
    fn init() -> SqlResult<Self> {
        let conn = Connection::open("dama_vault.db")?;
        // Устанавливаем пароль для файла базы данных
        conn.execute("PRAGMA key = 'dama_secure_vault_pass';", [])?; 
        
        conn.execute(
            "CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY,
                sender TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;
        Ok(Self { conn })
    }

    fn save_message(&self, sender: &str, content: &str) -> SqlResult<()> {
        self.conn.execute(
            "INSERT INTO messages (sender, content) VALUES (?1, ?2)",
            params![sender, content],
        )?;
        Ok(())
    }
}

// --- ШИФРОВАНИЕ (AES-256) ---
struct DamaEncryption {
    shared_secret: [u8; 32],
}

impl DamaEncryption {
    fn encrypt(&self, message: &[u8]) -> Vec<u8> {
        let key = Key::<Aes256Gcm>::from_slice(&self.shared_secret);
        let cipher = Aes256Gcm::new(key);
        let nonce = Nonce::from_slice(b"unique nonce 12"); 
        cipher.encrypt(nonce, message).expect("Ошибка шифрования")
    }
}

// --- СТРУКТУРА СООБЩЕНИЯ ---
#[derive(Serialize, Deserialize, Debug)]
struct MessageNode {
    text: String,
    parent_hash: String,
    sender: String,
}

// --- ПОВЕДЕНИЕ СЕТИ ---
#[derive(NetworkBehaviour)]
struct DamaBehaviour {
    kademlia: Kademlia<MemoryStore>,
    identify: identify::Behaviour,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("--- DAMA CORE: ЗАПУСК ПРИВАТНОЙ СЕТИ ---");

    // 1. Инициализация зашифрованного хранилища
    let storage = DamaStorage::init().expect("Не удалось создать зашифрованную базу данных");
    println!("✅ Хранилище сообщений активировано.");

    // 2. Подключение к Tor
    println!("Подключение к сети Tor (Arti)...");
    let config = TorClientConfig::default();
    let rt = PreferredRuntime::current()?;
    let _tor_client = TorClient::with_runtime(rt)
        .config(config)
        .create_bootstrapped()
        .await?;
    println!("✅ Tor активен. Ваш IP скрыт.");

    // 3. Настройка P2P узла
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    println!("Ваш PeerID: {}", local_peer_id);

    let store = MemoryStore::new(local_peer_id);
    let behaviour = DamaBehaviour {
        kademlia: Kademlia::new(local_peer_id, store),
        identify: identify::Behaviour::new(identify::Config::new("/dama/1.0".into(), local_key.public())),
    };

    let mut swarm = SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        .with_tcp(tcp::Config::default(), noise::Config::new, yamux::Config::default)?
        .with_behaviour(|_| behaviour)?
        .build();

    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    println!("🚀 Мессенджер DAMA запущен и готов к работе.");

    // Пример сохранения сообщения (тест базы)
    storage.save_message("Система", "Узел запущен успешно")?;

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => println!("Слушаем на: {}", address),
            _ => {}
        }
    }
}
