use eframe::egui;
use libp2p::swarm::{SwarmEvent, SwarmBuilder};
use libp2p::{identify, identity, kad, PeerId, Transport, floodsub::{Floodsub, FloodsubEvent, Topic}};
use std::error::Error;
use futures::StreamExt;
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::fs;

// --- ИНТЕРФЕЙС ПРИЛОЖЕНИЯ ---
struct DamaApp {
    peer_id: String,
    target_addr: String,
    message_text: String,
    history: Vec<String>,
    tx_to_network: Sender<String>,
    rx_from_network: Receiver<String>,
}

impl DamaApp {
    fn new(_cc: &eframe::CreationContext<'_>, tx: Sender<String>, rx: Receiver<String>, id: String) -> Self {
        _cc.egui_ctx.set_visuals(egui::Visuals::dark());
        Self {
            peer_id: id,
            target_addr: String::new(),
            message_text: String::new(),
            history: vec!["[SYSTEM]: DAMA Core запущен...".to_string()],
            tx_to_network: tx,
            rx_from_network: rx,
        }
    }
}

impl eframe::App for DamaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(msg) = self.rx_from_network.try_recv() {
            self.history.push(msg);
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.visuals_mut().override_text_color = Some(egui::Color32::from_rgb(0, 255, 0));
            
            ui.horizontal(|ui| {
                ui.heading("📟 DAMA CORE | DARKNET NODE");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // --- КНОПКА ПАНИКИ ---
                    let panic_btn = ui.add(egui::Button::new(egui::RichText::new("🚨 PANIC").color(egui::Color32::BLACK)).fill(egui::Color32::RED));
                    if panic_btn.clicked() {
                        let _ = fs::remove_file("dama_vault.db"); // Стираем базу
                        std::process::exit(0); // Мгновенно выходим
                    }
                });
            });

            ui.label(format!("ID: {}", self.peer_id));
            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Target:");
                ui.text_edit_singleline(&mut self.target_addr);
                if ui.button("📡 CONNECT").clicked() {
                    let _ = self.tx_to_network.send(format!("/connect {}", self.target_addr));
                }
            });

            ui.add_space(10.0);
            egui::ScrollArea::vertical().max_height(300.0).stick_to_bottom(true).show(ui, |ui| {
                for msg in &self.history {
                    ui.label(egui::RichText::new(msg).monospace());
                }
            });

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let res = ui.text_edit_singleline(&mut self.message_text);
                if ui.button("📤 SEND").clicked() || (res.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter))) {
                    if !self.message_text.is_empty() {
                        let _ = self.tx_to_network.send(self.message_text.clone());
                        self.history.push(format!("> Вы: {}", self.message_text));
                        self.message_text.clear();
                    }
                }
            });
        });
        ctx.request_repaint();
    }
}

#[derive(libp2p::swarm::NetworkBehaviour)]
struct DamaBehaviour {
    floodsub: Floodsub,
    kademlia: kad::Behaviour<kad::record::store::MemoryStore>,
    identify: identify::Behaviour,
}

fn main() -> Result<(), Box<dyn Error>> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    
    // ПРАВКА ТИПОВ: Явно указываем <String>, чтобы Rust не гадал
    let (tx_to_gui, rx_from_net): (Sender<String>, Receiver<String>) = unbounded();
    let (tx_to_net, rx_from_gui): (Sender<String>, Receiver<String>) = unbounded();

    let id_str = local_peer_id.to_string();

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let transport = libp2p::tcp::tokio::Transport::default()
                .upgrade(libp2p::core::upgrade::Version::V1)
                .authenticate(libp2p::noise::Config::new(&local_key).unwrap())
                .multiplex(libp2p::yamux::Config::default()).boxed();

            let chat_topic = Topic::new("dama-chat");
            let mut behaviour = DamaBehaviour {
                floodsub: Floodsub::new(local_peer_id),
                kademlia: kad::Behaviour::new(local_peer_id, kad::record::store::MemoryStore::new(local_peer_id)),
                identify: identify::Behaviour::new(identify::Config::new("/dama/1.0".into(), local_key.public())),
            };
            behaviour.floodsub.subscribe(chat_topic.clone());

            let mut swarm = SwarmBuilder::with_tokio_executor(transport, behaviour, local_peer_id).build();
            swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap()).unwrap();

            loop {
                tokio::select! {
                    cmd = async { rx_from_gui.recv() } => {
                        if let Ok(line) = cmd {
                            if line.starts_with("/connect") {
                                if let Some(addr) = line.split_whitespace().last() {
                                    let _ = swarm.dial(addr.parse::<libp2p::Multiaddr>().unwrap());
                                }
                            } else {
                                swarm.behaviour_mut().floodsub.publish(chat_topic.clone(), line.as_bytes());
                            }
                        }
                    }
                    event = swarm.select_next_some() => match event {
                        SwarmEvent::NewListenAddr { address, .. } => {
                            let _ = tx_to_gui.send(format!("📍 Адрес: {}", address));
                        }
                        SwarmEvent::Behaviour(DamaBehaviourEvent::Floodsub(FloodsubEvent::Message(msg))) => {
                            let text = String::from_utf8_lossy(&msg.data);
                            let _ = tx_to_gui.send(format!("📩 Друг: {}", text));
                        }
                        _ => {}
                    }
                }
            }
        });
    });

    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "DAMA Core",
        native_options,
        Box::new(|cc| Box::new(DamaApp::new(cc, tx_to_net, rx_from_net, id_str))),
    ).map_err(|e| e.to_string().into())
}
