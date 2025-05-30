#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod config_models;
mod errors;
mod generator_logic;

use app_state::GeneratorAppState;
use eframe::{App, Frame, egui};
use rfd::FileDialog;
use std::io::Write;

impl App for GeneratorAppState {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.style_mut().spacing.item_spacing = egui::vec2(8.0, 6.0);
            ui.style_mut().spacing.indent = 12.0;

            ui.heading("Remote Activity Monitor - Package Generator (P2P Mode)");
            ui.add_space(6.0);
            ui.label("This tool generates a client package for remote P2P deployment and a server package for the operator.");
            ui.label("The necessary template binaries and server assets are embedded within this generator.");
            ui.hyperlink_to("View Setup & Usage Instructions Online", "https://github.com/RIZAmohammadkhan/GuiKeyStandalone/blob/main/docs/GENERATOR_GUIDE.md");
            ui.add_space(10.0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .max_height(ui.available_height() - 30.0) // Ensure scroll area fits
                .show(ui, |ui| {

                egui::CollapsingHeader::new("🚀 Core Deployment Configuration")
                    .default_open(true)
                    .show(ui, |ui| {
                        ui.add_space(5.0);
                        ui.strong("Step 1: Configure Bootstrap Multiaddresses (for Client Package)");
                        ui.label("Comma-separated libp2p multiaddresses that clients will use to find the server or join the P2P network (e.g., public relays, or server's specific address if known and static).");

                        ui.horizontal(|ui| {
                            ui.label("Bootstrap Addresses:");
                            ui.add_sized([ui.available_width(), ui.text_style_height(&egui::TextStyle::Body)],
                                egui::TextEdit::singleline(&mut self.bootstrap_addresses_str)
                                    .hint_text("e.g., /dnsaddr/bootstrap.libp2p.io/p2p/QmNnoo..., /ip4/your.server.ip/tcp/port/p2p/YourServerPeerID"));
                        });
                        ui.add_space(10.0);

                        ui.strong("Step 2: Select Output Directory");
                        ui.label("Choose a folder where the 'ActivityMonitorClient_Package' and 'LocalLogServer_Package' will be saved.");
                        ui.horizontal(|ui| {
                            ui.label("Output Directory:");
                            ui.add_sized([ui.available_width() - 60.0, ui.text_style_height(&egui::TextStyle::Body)],
                                egui::TextEdit::singleline(&mut self.output_dir_path_str).hint_text("Path to save generated packages"));
                            if ui.button("📂 Select").on_hover_text("Choose Output Directory").clicked() {
                                if let Some(path) = FileDialog::new().pick_folder() {
                                    self.output_dir_path_str = path.to_string_lossy().into_owned();
                                }
                            }
                        });
                        ui.add_space(5.0);
                    });

                ui.add_space(8.0);

                egui::CollapsingHeader::new("📦 Local Log Server Package Configuration (Operator's Machine)")
                    .default_open(false) // Default to false as it's more advanced now
                    .show(ui, |ui| {
                    ui.add_space(5.0);
                    ui.label("Configure how the server application (in 'LocalLogServer_Package') will run on your (the operator's) machine.");
                    ui.add_space(3.0);
                    egui::Grid::new("server_config_grid")
                        .num_columns(2)
                        .spacing([10.0, 5.0])
                        .min_col_width(220.0) // Ensure labels have enough space
                        .show(ui, |ui| {

                        ui.label("Server P2P Listen Multiaddress:")
                            .on_hover_text("Libp2p multiaddress for P2P communication. Use '0' for port to pick any available. Example: /ip4/0.0.0.0/tcp/0 or /ip4/0.0.0.0/udp/0/quic-v1");
                        ui.add_sized([ui.available_width(), ui.text_style_height(&egui::TextStyle::Body)],
                            egui::TextEdit::singleline(&mut self.server_config.listen_address)
                                .hint_text("e.g., /ip4/0.0.0.0/tcp/0"));
                        ui.end_row();

                        ui.label("Server Web UI Listen Address:")
                            .on_hover_text("IP:PORT for the local web interface to view logs.");
                        ui.add_sized([ui.available_width(), ui.text_style_height(&egui::TextStyle::Body)],
                            egui::TextEdit::singleline(&mut self.server_config.web_ui_listen_address)
                                .hint_text("e.g., 0.0.0.0:8090 or 127.0.0.1:8090"));
                        ui.end_row();


                        ui.label("Server Database File Name:");
                        ui.add_sized([ui.available_width(), ui.text_style_height(&egui::TextStyle::Body)],
                            egui::TextEdit::singleline(&mut self.server_config.database_path)
                                .hint_text("e.g., activity_logs.sqlite"));
                        ui.end_row();

                        ui.label("Server Log Retention (days):")
                            .on_hover_text("0 for indefinite. How long the server keeps logs in its database.");
                        ui.add(egui::DragValue::new(&mut self.server_config.log_retention_days)
                            .speed(1.0).clamp_range(0..=3650).suffix(" days"));
                        ui.end_row();
                    });
                     ui.add_space(5.0);
                });

                ui.add_space(8.0);

                // Display generated Server Peer ID prominently if available
                ui.label("Generated Server Libp2p Peer ID (for client package):");
                let mut server_pid_display_text = self.generated_server_peer_id_display.clone();
                 ui.add_sized([ui.available_width(), ui.text_style_height(&egui::TextStyle::Body)],
                    egui::TextEdit::singleline(&mut server_pid_display_text)
                        .interactive(false)
                        .font(egui::TextStyle::Monospace)
                );
                ui.add_space(8.0);


                egui::CollapsingHeader::new("📱 Activity Monitor Client Package Configuration (Remote Machines)")
                    .default_open(true) // Keep this open by default as it's often tweaked
                    .show(ui, |ui| {
                    ui.add_space(5.0);
                    ui.label("These settings apply to the client applications (in 'ActivityMonitorClient_Package') that will be deployed remotely.");
                    ui.add_space(3.0);
                    egui::Grid::new("client_config_grid")
                        .num_columns(2)
                        .spacing([10.0, 5.0])
                        .min_col_width(220.0)
                        .show(ui, |ui| {

                        ui.label("Client Autorun Name:");
                        ui.add_sized([ui.available_width(), ui.text_style_height(&egui::TextStyle::Body)],
                            egui::TextEdit::singleline(&mut self.client_config.app_name_for_autorun));
                        ui.end_row();

                        ui.label("Client Cache Retention (days):")
                            .on_hover_text("0 for indefinite. How long client keeps unsent logs if server is unreachable.");
                        ui.add(egui::DragValue::new(&mut self.client_config.local_log_cache_retention_days)
                            .speed(1.0).clamp_range(0..=365).suffix(" days"));
                        ui.end_row();

                        ui.label("Client Sync Interval (sec):");
                        ui.add(egui::DragValue::new(&mut self.client_config.sync_interval)
                            .speed(10.0).clamp_range(10..=86400).suffix(" s"));
                        ui.end_row();

                        ui.label("Periodic Session Flush (sec):")
                            .on_hover_text("Interval to flush current app activity if no app switch occurs. 0 to disable periodic flush.");
                        ui.add(egui::DragValue::new(&mut self.client_config.processor_periodic_flush_interval_secs)
                            .speed(10.0).clamp_range(0..=7200u64).suffix(" s"));
                        ui.end_row();

                        ui.label("Max Client Log File Size (MB):")
                            .on_hover_text("Max size for client's local cache (activity_data.jsonl). 0 for no limit (not recommended).");
                        let mut max_size_u64 = self.client_config.max_log_file_size_mb.unwrap_or(0);
                        if ui.add(egui::DragValue::new(&mut max_size_u64).speed(1.0).clamp_range(0..=1024).suffix(" MB")).changed() {
                            self.client_config.max_log_file_size_mb = if max_size_u64 == 0 { None } else { Some(max_size_u64) };
                        }
                        ui.end_row();

                        ui.label("Client Internal Log Level:");
                        egui::ComboBox::from_id_source("client_log_level_combo")
                            .selected_text(self.client_config.internal_log_level.to_uppercase())
                            .width(ui.available_width())
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.client_config.internal_log_level, "trace".to_string(), "Trace");
                                ui.selectable_value(&mut self.client_config.internal_log_level, "debug".to_string(), "Debug");
                                ui.selectable_value(&mut self.client_config.internal_log_level, "info".to_string(), "Info");
                                ui.selectable_value(&mut self.client_config.internal_log_level, "warn".to_string(), "Warn");
                                ui.selectable_value(&mut self.client_config.internal_log_level, "error".to_string(), "Error");
                            });
                        ui.end_row();
                    });
                    ui.add_space(5.0);
                });

                ui.add_space(15.0);

                ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                    let generate_button = egui::Button::new("📦 Generate Deployment Packages")
                        .min_size(egui::vec2(300.0, 35.0));

                    if ui.add_enabled(!self.operation_in_progress, generate_button)
                        .on_hover_text("Generates client & server packages using embedded templates into the selected Output Directory.")
                        .clicked() {
                        self.operation_in_progress = true;
                        self.status_message = "Starting generation process...".to_string();
                        self.generated_client_id_display = "Generating...".to_string();
                        self.generated_server_peer_id_display = "Generating...".to_string();
                        self.generated_key_hex_display_snippet = "Generating...".to_string();

                        // Spawn the generation logic in a new thread to avoid blocking the UI
                        // This is a simplified example. For robust error handling back to UI,
                        // you might use channels (e.g., std::sync::mpsc) or an Arc<Mutex<Option<Result>>>.
                        // For now, we'll update status directly, but errors won't make it back to GUI text from another thread without channels.
                        // Better: Pass a clone of the Arc<Mutex<GeneratorAppState>> or specific fields.
                        // However, `self` is already `&mut GeneratorAppState` here.
                        // `perform_generation` modifies `self` directly.
                        // The blocking part is mostly I/O and crypto, which is fine for a quick operation.
                        // If it were very long, then a thread + channel would be better.

                        match generator_logic::perform_generation(self) { // `self` is `&mut GeneratorAppState`
                            Ok(_) => { /* Status is updated within perform_generation */ }
                            Err(e) => {
                                self.status_message = format!("Error: {}", e);
                                self.operation_in_progress = false; // Ensure flag is reset on error
                            }
                        }
                    }
                });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(5.0);

                if self.operation_in_progress {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(&self.status_message);
                    });
                } else {
                    ui.label("Status:");
                    let mut status_display_text = self.status_message.clone();
                    ui.add_sized(
                        [ui.available_width(), 60.0], // Fixed height for status
                        egui::TextEdit::multiline(&mut status_display_text)
                            .desired_rows(3)
                            .interactive(false)
                            .font(egui::TextStyle::Monospace)
                    );

                    if self.generated_server_peer_id_display != "N/A (will be generated)" && self.generated_server_peer_id_display != "Generating..." {
                        // Already displayed above, but could repeat here if desired.
                    }

                    if self.generated_client_id_display != "N/A" && self.generated_client_id_display != "Generating..." {
                        ui.horizontal(|ui|{
                            ui.label("Generated App Client ID:");
                            let mut client_id_text = self.generated_client_id_display.clone();
                            ui.add_sized([ui.available_width(), ui.text_style_height(&egui::TextStyle::Body)],
                                egui::TextEdit::singleline(&mut client_id_text).interactive(false).font(egui::TextStyle::Monospace));
                        });
                    }

                    if self.generated_key_hex_display_snippet != "N/A" && self.generated_key_hex_display_snippet != "Generating..." {
                         ui.horizontal(|ui|{
                            ui.label("Generated App AES Key (snippet):");
                            let mut key_snippet_text = self.generated_key_hex_display_snippet.clone(); // Already has "..."
                            ui.add_sized([ui.available_width(), ui.text_style_height(&egui::TextStyle::Body)],
                                egui::TextEdit::singleline(&mut key_snippet_text).interactive(false).font(egui::TextStyle::Monospace));
                        });
                    }
                }
            });
        });
    }
}

fn calculate_window_size() -> [f32; 2] {
    let width = 740.0; // Slightly wider for new fields
    let height = 820.0; // Slightly taller
    [width, height]
}

fn main() -> eframe::Result<()> {
    let default_panic_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        eprintln!("Generator GUI Panicked: {:?}", panic_info);
        let log_message = format!(
            "PANIC: {:?}\nTimestamp: {}\n",
            panic_info,
            chrono::Local::now().to_rfc3339()
        );
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let panic_log_path = exe_dir.join("generator_gui_panic.log");
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(panic_log_path)
                {
                    let _ = writeln!(file, "{}", log_message);
                } else {
                    eprintln!("Failed to open/create panic log file for generator GUI.");
                }
            } else {
                eprintln!("Failed to get executable directory for generator GUI panic log.");
            }
        } else {
            eprintln!("Failed to get current executable path for generator GUI panic log.");
        }
        default_panic_hook(panic_info);
    }));

    let window_size = calculate_window_size();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(window_size)
            .with_min_inner_size([700.0, 700.0])
            .with_resizable(true)
            .with_decorations(true),
        follow_system_theme: true,
        ..Default::default()
    };

    eframe::run_native(
        "Remote Activity Monitor - Package Generator (vEmbed P2P)",
        options,
        Box::new(|_cc| Box::new(GeneratorAppState::default())),
    )
}
