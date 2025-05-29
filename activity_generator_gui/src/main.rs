#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // Hide console on release

mod app_state;
mod config_models;
mod errors;
mod generator_logic;

use eframe::{egui, App, Frame};
use rfd::FileDialog; // For file and folder dialogs
use app_state::GeneratorAppState;
use std::io::Write; // For panic hook writeln!

impl App for GeneratorAppState {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            // More compact spacing for better fit
            ui.style_mut().spacing.item_spacing = egui::vec2(6.0, 4.0);
            ui.style_mut().spacing.indent = 12.0;
            
            // More compact heading
            ui.heading("Activity Monitor Suite - Package Generator");
            ui.add_space(6.0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .max_height(ui.available_height() - 20.0)
                .show(ui, |ui| {
                
                // --- Input Paths ---
                egui::CollapsingHeader::new("📁 Required Paths & Output")
                    .default_open(true)
                    .show(ui, |ui| {
                    egui::Grid::new("paths_grid")
                        .num_columns(2)
                        .spacing([8.0, 3.0])
                        .show(ui, |ui| {
                        
                        ui.label("Client Template (.exe):")
                            .on_hover_text("Path to the pre-compiled activity_monitor_client_template.exe");
                        ui.horizontal(|ui| {
                            ui.add_sized([ui.available_width() - 55.0, 18.0], 
                                egui::TextEdit::singleline(&mut self.client_template_exe_path_str));
                            if ui.small_button("📂").clicked() {
                                if let Some(path) = FileDialog::new().add_filter("Executable", &["exe"]).pick_file() {
                                    self.client_template_exe_path_str = path.to_string_lossy().into_owned();
                                }
                            }
                        });
                        ui.end_row();

                        ui.label("Server Template (.exe):")
                            .on_hover_text("Path to the pre-compiled local_log_server_template.exe");
                        ui.horizontal(|ui| {
                            ui.add_sized([ui.available_width() - 55.0, 18.0], 
                                egui::TextEdit::singleline(&mut self.server_template_exe_path_str));
                            if ui.small_button("📂").clicked() {
                                if let Some(path) = FileDialog::new().add_filter("Executable", &["exe"]).pick_file() {
                                    self.server_template_exe_path_str = path.to_string_lossy().into_owned();
                                }
                            }
                        });
                        ui.end_row();

                        ui.label("Output Directory:")
                            .on_hover_text("Folder where the generated package will be saved.");
                        ui.horizontal(|ui|{
                            ui.add_sized([ui.available_width() - 55.0, 18.0], 
                                egui::TextEdit::singleline(&mut self.output_dir_path_str));
                            if ui.small_button("📂").clicked() {
                                if let Some(path) = FileDialog::new().pick_folder() {
                                    self.output_dir_path_str = path.to_string_lossy().into_owned();
                                }
                            }
                        });
                        ui.end_row();
                    });
                });
                
                ui.add_space(4.0);

                // --- Server Configuration ---
                egui::CollapsingHeader::new("🖥️ Local Log Server Configuration")
                    .default_open(false)
                    .show(ui, |ui| {
                    egui::Grid::new("server_config_grid")
                        .num_columns(2)
                        .spacing([8.0, 3.0])
                        .show(ui, |ui| {
                        
                        ui.label("Listen Address:");
                        if ui.add_sized([ui.available_width(), 18.0],
                            egui::TextEdit::singleline(&mut self.server_config.listen_address)
                                .hint_text("e.g., 127.0.0.1:8090")).changed() {
                            self.synchronize_dependent_configs(); // Update client's server_url
                        }
                        ui.end_row();

                        ui.label("Database File Name:");
                        ui.add_sized([ui.available_width(), 18.0],
                            egui::TextEdit::singleline(&mut self.server_config.database_path)
                                .hint_text("e.g., activity_logs.sqlite"));
                        ui.end_row();
                        
                        ui.label("Log Retention (days):");
                        ui.add(egui::DragValue::new(&mut self.server_config.log_retention_days)
                            .speed(1.0).clamp_range(1..=3650));
                        ui.end_row();
                    });
                });
                
                ui.add_space(4.0);

                // --- Client Configuration ---
                egui::CollapsingHeader::new("📱 Activity Monitor Client Configuration")
                    .default_open(false)
                    .show(ui, |ui| {
                    egui::Grid::new("client_config_grid")
                        .num_columns(2)
                        .spacing([8.0, 3.0])
                        .show(ui, |ui| {
                        
                        ui.label("Autorun Name:");
                        ui.add_sized([ui.available_width(), 18.0],
                            egui::TextEdit::singleline(&mut self.client_config.app_name_for_autorun));
                        ui.end_row();

                        ui.label("Cache Retention (days):")
                            .on_hover_text("0 for indefinite retention of unsent logs on client.");
                        ui.add(egui::DragValue::new(&mut self.client_config.local_log_cache_retention_days)
                            .speed(1.0).clamp_range(0..=365));
                        ui.end_row();

                        ui.label("Sync Interval (seconds):");
                        ui.add(egui::DragValue::new(&mut self.client_config.sync_interval)
                            .speed(10.0).clamp_range(10..=86400));
                        ui.end_row();

                        ui.label("Flush Interval (seconds):");
                        ui.add(egui::DragValue::new(&mut self.client_config.processor_periodic_flush_interval_secs)
                            .speed(10.0).clamp_range(10..=3600));
                        ui.end_row();

                        ui.label("Internal Log Level:");
                        let current_log_level = self.client_config.internal_log_level.clone();
                        egui::ComboBox::from_id_source("client_log_level_combo")
                            .selected_text(current_log_level)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.client_config.internal_log_level, "trace".to_string(), "Trace");
                                ui.selectable_value(&mut self.client_config.internal_log_level, "debug".to_string(), "Debug");
                                ui.selectable_value(&mut self.client_config.internal_log_level, "info".to_string(), "Info");
                                ui.selectable_value(&mut self.client_config.internal_log_level, "warn".to_string(), "Warn");
                                ui.selectable_value(&mut self.client_config.internal_log_level, "error".to_string(), "Error");
                            });
                        ui.end_row();
                    });
                });
                
                ui.add_space(8.0);

                // --- Generation Button ---
                ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                    let generate_button = egui::Button::new("🚀 Generate Monitoring Package")
                        .min_size(egui::vec2(220.0, 28.0));
                    
                    if ui.add_enabled(!self.operation_in_progress, generate_button)
                        .on_hover_text("Generates client & server executables with linked configurations into the selected Output Directory.")
                        .clicked() {
                        self.operation_in_progress = true;
                        self.status_message = "Starting generation process...".to_string();
                        self.generated_client_id_display = "Generating...".to_string();
                        self.generated_key_hex_display_snippet = "Generating...".to_string();
                        
                        match generator_logic::perform_generation(self) {
                            Ok(_) => {
                                // Status message is updated inside perform_generation upon success
                            }
                            Err(e) => {
                                self.status_message = format!("Error: {}", e);
                                self.operation_in_progress = false;
                            }
                        }
                    }
                });

                if self.operation_in_progress {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(&self.status_message);
                    });
                }
                
                ui.add_space(6.0);
                ui.separator();

                // --- Status and Results ---
                if !self.operation_in_progress {
                    ui.label("Status:");
                    let mut status_display_text = self.status_message.clone();
                    ui.add(
                        egui::TextEdit::multiline(&mut status_display_text)
                            .desired_rows(3) // Reduced from 4
                            .desired_width(f32::INFINITY)
                            .interactive(false)
                    );

                    if !self.generated_client_id_display.is_empty() && 
                       self.generated_client_id_display != "N/A" && 
                       self.generated_client_id_display != "Generating..." {
                        ui.horizontal(|ui|{
                            ui.label("Client ID:");
                            let mut client_id_text = self.generated_client_id_display.clone();
                            ui.add_sized([ui.available_width(), 18.0],
                                egui::TextEdit::singleline(&mut client_id_text).interactive(false));
                        });
                    }
                    
                    if !self.generated_key_hex_display_snippet.is_empty() && 
                       self.generated_key_hex_display_snippet != "N/A" &&
                       self.generated_key_hex_display_snippet != "Generating..." {
                         ui.horizontal(|ui|{
                            ui.label("Key (snippet):");
                            let mut key_snippet_text = format!("{}...", self.generated_key_hex_display_snippet);
                            ui.add_sized([ui.available_width(), 18.0],
                                egui::TextEdit::singleline(&mut key_snippet_text).interactive(false));
                        });
                    }
                }
            }); // End ScrollArea
        });
    }
}

// Helper function to calculate appropriate window size
fn calculate_window_size() -> [f32; 2] {
    // Conservative defaults that work on most screens (even 1366x768)
    let width = 620.0;  // Reduced from original 700.0
    let height = 680.0; // Reduced from original 850.0
    [width, height]
}

fn main() -> eframe::Result<()> {
    // Setup a panic hook to log panics to a file for easier debugging by users
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
                     eprintln!("Failed to open/create panic log file.");
                }
            } else {
                eprintln!("Failed to get executable directory for panic log.");
            }
        } else {
             eprintln!("Failed to get current executable path for panic log.");
        }
        default_panic_hook(panic_info);
    }));
    
    let window_size = calculate_window_size();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(window_size)
            .with_min_inner_size([500.0, 550.0]) // Smaller minimum size
            .with_max_inner_size([800.0, 900.0]) // Maximum size constraint
            .with_resizable(true)
            .with_position([50.0, 50.0]), // Position away from screen edges
        follow_system_theme: true,
        ..Default::default()
    };
    
    eframe::run_native(
        "Activity Monitor Suite - Package Generator",
        options,
        Box::new(|_cc| Box::new(GeneratorAppState::default())),
    )
}