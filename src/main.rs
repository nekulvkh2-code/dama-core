use eframe::egui;
use libp2p::swarm::{SwarmEvent, SwarmBuilder};
use libp2p::{identify, identity, kad, PeerId, Transport, floodsub::{Floodsub, FloodsubEvent, Topic}};
use std::error::Error;
use futures::StreamExt;
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::fs;
use chrono::Local;
use rfd::FileDialog;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
enum DamaPacket {
    Text(String),
    File { name: String, data: String },
    SetAddress(String), // Новый тип пакета для обновления адреса в UI
}

struct Contact { name: String, addr: String }

struct DamaApp {
    peer_id: String,
    my_address: String,
    new_contact_name: String,
    new_contact_addr: String,
    contacts: Vec<Contact>,
    selected_contact: Option<usize>,
    message_text: String,
    history: Vec<String>,
    tx_to_network: Sender<DamaPacket>,
    rx_from_network: Receiver<(String, DamaPacket)>,
}

impl DamaApp {
    fn new(_cc: &eframe::CreationContext<'_>, tx: Sender<DamaPacket>, rx: Receiver<(String, DamaPacket)>, id: String) -> Self {
        _cc.egui_ctx.set_visuals(egui::Visuals::dark());
        Self {
            peer_id: id,
            my_address: "Ожидание сети...".to_string(),
            new_contact_name: String::new(),
            new_contact_addr: String::new(),
            contacts: Vec::new(),
            selected_contact: None,
            message_text: String::new(),
            history: vec!["[SYSTEM]: Узел запущен.".to_string()],
            tx_to_network: tx,
            rx_from_network: rx,
        }
    }
}

impl eframe::App for DamaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok((sender, packet)) = self.rx_from_network.try_recv() {
            let time = Local::now().format("%H:%M");
            match packet {
                DamaPacket::Text(t) => self.history.push(format!("[{}] {}: {}", time, sender, t)),
                DamaPacket::File { name, .. } => self.history.push(format!("[{}] {} отправил файл: {}", time, sender, name)),
                DamaPacket::SetAddress(addr) => self.my_address = addr, // Обновляем адрес в UI
            }
        }

        // --- ВЕРХНЯЯ ИНФО-ПАНЕЛЬ ---
        egui::TopBottomPanel::top("top_info").show(ctx, |ui| {
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                ui.heading(egui::RichText::new("📟 DAMA CORE").color(egui::Color32::from_rgb(0, 255, 0)));
                ui.separator();
                
                // Отображение PeerID
                ui.label("ID:");
                ui.code(&self.peer_id[..10]);
                if ui.button("📋").on_hover_text("Копировать ID").clicked() {
                    ui.output_mut(|o| o.copied_text = self.peer_id.clone());
                }

                ui.separator();

                // Отображение Адреса
                ui.label("Адрес:");
                ui.code(&self.my_address);
                if ui.button("📋").on_hover_text("Копировать адрес").clicked() {
                    ui.output_mut(|o| o.copied_text = self.my_address.clone());
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new("🚨 PANIC").fill(egui::Color32::RED)).clicked() {
                        let _ = fs::remove_file("dama_vault.db");
                        std::process::exit(0);
                    }
                });
            });
            ui.add_space(5.0);
        });

        // --- ЛЕВАЯ ПАНЕЛЬ (КОНТАКТЫ) ---
        egui::SidePanel::left("contacts_side").resizable(true).default_width(180.0).show(ctx, |ui| {
            ui.heading("👥 Контакты");
            ui.separator();
            for (i, contact) in self.contacts.iter().enumerate() {
                if ui.selectable_label(self.selected_contact == Some(i), &contact.name).clicked() {
                    self.selected_contact = Some(i);
                    self.new_contact_addr = contact.addr.clone();
                }
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                if ui.button("➕ Добавить").clicked() {
                    self.contacts.push(Contact { name: self.new_contact_name.clone(), addr: self.new_contact_addr.clone() });
                    self.new_contact_name.clear();
                }
                ui.add(egui::TextEdit::singleline(&mut self.new_contact_addr).hint_text("Адрес /ip4/..."));
                ui.add(egui::TextEdit::singleline(&mut self.new_contact_name).hint_text("Имя"));
            });
        });

        // --- ЦЕНТРАЛЬНАЯ ПАНЕЛЬ (ЧАТ) ---
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height(ui.available_height() - 40.0).stick_to_bottom(true).show(ui, |ui| {
                for line in &self.history {
                    ui.label(egui::RichText::new(line).monospace().color(egui::Color32::from_rgb(0, 200, 0)));
                }
            });

            ui.horizontal(|ui| {
                if ui.button("📎").clicked() {
                    if let Some(path) = FileDialog::new().pick_file() {
                        if let Ok(bytes) = fs::read(&path) {
                            let packet = DamaPacket::File { name: path.file_name().unwrap().to_string_lossy().into(), data: base64::encode(bytes) };
                            let _ = self.tx_to_network.send(packet);
                            self.history.push(format!("Вы отправили файл: {}", path.display()));
                        }
                    }
                }
                let res = ui.text_edit_singleline(&mut self.message_text);
                if ui.button("📤 SEND").clicked() || (res.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter))) {
                    if !self.message_text.is_empty() {
                        let input = self.message_text.trim().to_string();
                        if input.starts_with("/ip4/") {
                            let _ = self.tx_to_network.send(DamaPacket::Text(format!("/connect {}", input)));
                        } else {
                            let _ = self.tx_to_network.send(DamaPacket::Text(input.clone()));
                            self.history.push(format!("Вы: {}", input));
                        }
                        self.message_text.clear();
                    }
                }
                if ui.button("📡 CONNECT").clicked() {
                    let _ = self.tx_to_network.send(DamaPacket::Text(format!("/connect {}", self.new_contact_addr)));
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
    let (tx_to_gui, rx_from_net) = unbounded::<(String, DamaPacket)>();
    let (tx_to_net, rx_from_gui) = unbounded::<DamaPacket>();
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
                    packet = async { rx_from_gui.recv() } => {
                        if let Ok(p) = packet {
                            if let DamaPacket::Text(t) = &p {
                                if t.starts_with("/connect ") {
                                    let addr = t.replace("/connect ", "");
                                    let _ = swarm.dial(addr.parse::<libp2p::Multiaddr>().unwrap());
                                    continue;
                                }
                            }
                            let bytes = serde_json::to_vec(&p).unwrap();
                            swarm.behaviour_mut().floodsub.publish(chat_topic.clone(), bytes);
                        }
                    }
                    event = swarm.select_next_some() => match event {
                        SwarmEvent::NewListenAddr { address, .. } => {
                            // Отправляем адрес в UI через специальный пакет
                            let _ = tx_to_gui.send(("Система".into(), DamaPacket::SetAddress(address.to_string())));
                        }
                        SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                            let _ = tx_to_gui.send(("Система".into(), DamaPacket::Text(format!("🤝 СОЕДИНЕНИЕ: {}", peer_id))));
                            swarm.behaviour_mut().floodsub.add_node_to_partial_view(peer_id);
                        }
                        SwarmEvent::Behaviour(DamaBehaviourEvent::Floodsub(FloodsubEvent::Message(msg))) => {
                            if let Ok(p) = serde_json::from_slice::<DamaPacket>(&msg.data) {
                                let _ = tx_to_gui.send((msg.source.to_string(), p));
                            }
                        }
                        _ => {}
                    }
                }
            }
        });
    });

    eframe::run_native("DAMA Core", eframe::NativeOptions::default(), Box::new(|cc| Box::new(DamaApp::new(cc, tx_to_net, rx_from_net, id_str))))
        .map_err(|e| e.to_string().into())
}

