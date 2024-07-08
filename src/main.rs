use eframe::egui;
use log::{error, info, warn};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use thiserror::Error;
use tokio;
use tokio::sync::oneshot;
use walkdir::WalkDir;

#[derive(Debug, Error)]
pub enum SpindleSpeedUpdaterError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("No parent directory found")]
    NoParentDirectory,
    #[error("Failed to lock progress: {0}")]
    ProgressLockFailure(String),
    #[error("Invalid spindle speed: {0}")]
    InvalidSpindleSpeed(String),
    #[error("Backup failure: {0}")]
    BackupFailure(String),
    #[error("Operation cancelled: {0}")]
    CancelError(String),
}

impl From<SpindleSpeedUpdaterError> for String {
    fn from(error: SpindleSpeedUpdaterError) -> Self {
        error.to_string()
    }
}

impl From<String> for SpindleSpeedUpdaterError {
    fn from(s: String) -> Self {
        SpindleSpeedUpdaterError::InvalidSpindleSpeed(s)
    }
}

#[allow(dead_code)]
#[derive(Clone)]
struct FileInfo {
    path: PathBuf,
    last_modified: std::time::SystemTime,
}

#[allow(dead_code)]
struct MainApp {
    spindle_speed_input: String,
    validated_spindle_speed: Option<u32>,
    processing: bool,
    progress: Arc<Mutex<(usize, usize)>>,
    error_message: Option<String>,
    error_sender: Sender<SpindleSpeedUpdaterError>,
    error_receiver: Receiver<SpindleSpeedUpdaterError>,
    show_confirmation_dialog: bool,
    file_cache: HashMap<PathBuf, FileInfo>,
    cancel_sender: Option<oneshot::Sender<()>>,
    success_message: Option<String>,
    last_enter_press: Instant,
}

impl MainApp {
    fn new() -> Self {
        let (error_sender, error_receiver) = channel();

        let mut app = Self {
            spindle_speed_input: String::new(),
            validated_spindle_speed: None,
            processing: false,
            progress: Arc::new(Mutex::new((0, 0))),
            error_message: None,
            error_sender,
            error_receiver,
            show_confirmation_dialog: false,
            file_cache: HashMap::new(),
            cancel_sender: None,
            success_message: None,
            last_enter_press: Instant::now(),
        };

        info!("Initializing MainApp, updating file cache");
        if let Err(e) = app.update_file_cache() {
            error!("Failed to update file cache: {:?}", e);
        } else {
            info!("File cache updated successfully");
        }

        app
    }

    #[allow(dead_code)]
    fn update_file_cache(&mut self) -> Result<(), SpindleSpeedUpdaterError> {
        let executable_path = std::env::current_exe().map_err(SpindleSpeedUpdaterError::Io)?;
        let folder_path = executable_path
            .parent()
            .ok_or(SpindleSpeedUpdaterError::NoParentDirectory)?;

        self.file_cache.clear();

        for entry in WalkDir::new(folder_path).into_iter().filter_map(|e| e.ok()) {
            if entry.path().extension().map_or(false, |ext| ext == "tap") {
                let metadata =
                    std::fs::metadata(entry.path()).map_err(SpindleSpeedUpdaterError::Io)?;
                let file_info = FileInfo {
                    path: entry.path().to_path_buf(),
                    last_modified: metadata.modified().map_err(SpindleSpeedUpdaterError::Io)?,
                };
                self.file_cache
                    .insert(entry.path().to_path_buf(), file_info);
            }
        }

        Ok(())
    }

    #[allow(dead_code)]
    fn cancel_operation(&mut self) -> Result<(), SpindleSpeedUpdaterError> {
        if let Some(cancel_sender) = self.cancel_sender.take() {
            cancel_sender.send(()).map_err(|_| {
                SpindleSpeedUpdaterError::CancelError("Failed to send cancel signal".to_string())
            })?;
            self.processing = false;
            Ok(())
        } else {
            Err(SpindleSpeedUpdaterError::CancelError(
                "No operation in progress".to_string(),
            ))
        }
    }

    #[allow(dead_code)]
    fn show_feedback(&mut self, ui: &mut egui::Ui) {
        if self.processing {
            if let Ok(progress_data) = self.progress.lock() {
                let (processed, total) = *progress_data;
                if total > 0 {
                    let progress = processed as f32 / total as f32;
                    ui.add(egui::ProgressBar::new(progress).show_percentage());
                    ui.label(format!("Processed {} out of {} files", processed, total));
                }
            }

            if ui.button("Cancel").clicked() {
                if let Err(e) = self.cancel_operation() {
                    self.error_message = Some(format!("Failed to cancel operation: {:?}", e));
                }
            }
        }

        if let Some(error_message) = &self.error_message {
            ui.colored_label(egui::Color32::RED, error_message);
        }

        if !self.processing && ui.button("Clear Error").clicked() {
            self.error_message = None;
        }
    }

    #[allow(dead_code)]
    fn validate_spindle_speed(&mut self) -> Result<(), String> {
        const MIN_SPEED: u32 = 1;
        const MAX_SPEED: u32 = 24000;

        info!("Validating spindle speed: {}", self.spindle_speed_input);
        match self.spindle_speed_input.parse::<u32>() {
            Ok(speed) if (MIN_SPEED..=MAX_SPEED).contains(&speed) => {
                self.validated_spindle_speed = Some(speed);
                info!("Spindle speed validated: {}", speed);
                Ok(())
            }
            Ok(_) => {
                let err = format!(
                    "Spindle speed must be between {} and {} RPM",
                    MIN_SPEED, MAX_SPEED
                );
                info!("Validation failed: {}", err);
                Err(err)
            }
            Err(_) => {
                let err = "Invalid input. Please enter a valid number".to_string();
                info!("Validation failed: {}", err);
                Err(err)
            }
        }
    }

    #[allow(dead_code)]
    fn show_confirmation_dialog(&mut self, ctx: &egui::Context) {
        let mut user_choice: Option<bool> = None;
        let validated_speed = self.validated_spindle_speed.unwrap();

        egui::Window::new("Confirm Update")
            .collapsible(false)
            .resizable(false)
            .open(&mut self.show_confirmation_dialog)
            .show(ctx, |ui| {
                ui.label(format!(
                    "Are you sure you want to update the spindle speed to {} RPM in all .tap files?",
                    validated_speed
                ));
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(egui::RichText::new("Yes").strong())
                        .fill(egui::Color32::from_rgb(108, 108, 108)))
                        .clicked()
                    {
                        user_choice = Some(true);
                    }
                    if ui.button("No").clicked() {
                        user_choice = Some(false);
                    }
                });

                let now = Instant::now();
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) &&
                    now.duration_since(self.last_enter_press).as_millis() > 500
                {
                    user_choice = Some(true);
                    self.last_enter_press = now;
                }

                ui.label("Press Enter to confirm");
            });

        if let Some(choice) = user_choice {
            self.show_confirmation_dialog = false;
            if choice {
                if let Err(error) = self.start_update_process() {
                    self.error_message = Some(error.to_string());
                    error!("Failed to start spindle speed update: {:?}", error);
                } else {
                    info!("Started spindle speed update process");
                    self.error_message = None;
                }
            }
        }
    }

    fn start_update_process(&mut self) -> Result<(), SpindleSpeedUpdaterError> {
        info!("Starting update process");

        self.success_message = None;

        let speed =
            self.validated_spindle_speed
                .ok_or(SpindleSpeedUpdaterError::InvalidSpindleSpeed(
                    "No validated spindle speed".to_string(),
                ))?;
        info!("Validated speed: {}", speed);
        self.processing = true;
        let progress = Arc::clone(&self.progress);
        let error_sender = self.error_sender.clone();
        let file_cache = self.file_cache.clone();

        let (cancel_sender, cancel_receiver) = oneshot::channel();
        self.cancel_sender = Some(cancel_sender);

        tokio::spawn(async move {
            if let Err(error) =
                update_spindle_speed(speed, progress, &file_cache, cancel_receiver).await
            {
                log::error!("Error updating spindle speed: {:?}", error);
                if let Err(send_error) = error_sender.send(error) {
                    log::error!("Failed to send error to main thread: {}", send_error);
                }
            }
        });

        Ok(())
    }
}

impl eframe::App for MainApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Ok(error) = self.error_receiver.try_recv() {
            self.error_message = Some(error.to_string());
            self.processing = false;
            log::error!("Received error from background thread: {:?}", error);
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Spindle Speed Updater");

            let mut update_triggered = false;

            ui.horizontal(|ui| {
                ui.label("Enter the desired spindle speed (RPM):");
                let response = ui.text_edit_singleline(&mut self.spindle_speed_input);
                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let now = Instant::now();
                    if now.duration_since(self.last_enter_press).as_millis() > 500 {
                        update_triggered = true;
                        self.last_enter_press = now;
                    }
                }
            });

            let button_clicked = ui
                .add(
                    egui::Button::new(egui::RichText::new("Update Spindle Speeds").strong())
                        .fill(egui::Color32::from_rgb(108, 108, 108)),
                )
                .clicked();

            if (button_clicked || update_triggered)
                && !self.processing
                && !self.show_confirmation_dialog
            {
                match self.validate_spindle_speed() {
                    Ok(_) => {
                        self.show_confirmation_dialog = true;
                        self.error_message = None;
                    }
                    Err(error) => {
                        self.error_message = Some(error);
                    }
                }
            }

            // ERROR PROCESSING & PROGRESS BAR
            if self.processing {
                let progress_guard = self.progress.lock();
                match progress_guard {
                    Ok(progress_data) => {
                        let (processed, total) = *progress_data;
                        if total > 0 {
                            let progress = processed as f32 / total as f32;
                            ui.add(egui::ProgressBar::new(progress).show_percentage());
                            ui.label(format!("Processed {} of {} files", processed, total));

                            if processed == total {
                                self.processing = false;
                                let speed = self.validated_spindle_speed.unwrap();
                                self.success_message = Some(format!(
                                    "Successfully updated {} files to {} RPM.",
                                    processed, speed
                                ));
                                info!("Spindle speed update completed");
                            }
                        }
                    }
                    Err(error) => {
                        log::error!("Failed to lock progress mutex: {}", error);
                        self.error_message =
                            Some("Internal error: Failed to access progress data".to_string());
                        self.processing = false;
                    }
                }
            }

            if let Some(error_message) = &self.error_message {
                ui.colored_label(egui::Color32::RED, error_message);
            }

            if let Some(success_message) = &self.success_message {
                ui.colored_label(egui::Color32::GREEN, success_message);
            }

            if !self.processing {
                if ui.button("Clear Messages").clicked() {
                    self.error_message = None;
                    self.success_message = None;
                }
            }
        });

        if self.show_confirmation_dialog {
            self.show_confirmation_dialog(ctx);
        }

        if self.processing {
            ctx.request_repaint();
        }
    }
}

#[allow(dead_code)]
async fn update_spindle_speed(
    spindle_speed: u32,
    progress: Arc<Mutex<(usize, usize)>>,
    file_cache: &HashMap<PathBuf, FileInfo>,
    mut cancel_receiver: oneshot::Receiver<()>,
) -> Result<(), SpindleSpeedUpdaterError> {
    info!("update_spindle_speed started with speed: {}", spindle_speed);
    let total_files = file_cache.len();
    info!("Total files to process: {}", total_files);
    let mut processed_files = 0;

    {
        let mut progress_guard = progress
            .lock()
            .map_err(|e| SpindleSpeedUpdaterError::ProgressLockFailure(e.to_string()))?;
        *progress_guard = (0, total_files);
    }

    for (file_path, file_info) in file_cache {
        tokio::select! {
            _ = &mut cancel_receiver => {
                return Err(SpindleSpeedUpdaterError::CancelError("Operation cancelled by user".to_string()));
            }
            result = process_file(file_path, file_info, spindle_speed) => {
                result?;
            }
        }

        processed_files += 1;
        {
            let mut progress_guard = progress
                .lock()
                .map_err(|e| SpindleSpeedUpdaterError::ProgressLockFailure(e.to_string()))?;
            progress_guard.0 = processed_files;
        }
    }

    Ok(())
}

#[allow(dead_code)]
async fn process_file(
    file_path: &Path,
    file_info: &FileInfo,
    spindle_speed: u32,
) -> Result<(), SpindleSpeedUpdaterError> {
    let metadata = tokio::fs::metadata(file_path)
        .await
        .map_err(SpindleSpeedUpdaterError::Io)?;
    if metadata.modified().map_err(SpindleSpeedUpdaterError::Io)? != file_info.last_modified {
        warn!("File {:?} has been modified since last cached", file_path);
    }

    let updated = update_file_spindle_speed(file_path, spindle_speed)
        .await
        .map_err(SpindleSpeedUpdaterError::Io)?;

    if updated {
        info!("Updated spindle speed in file: {:?}", file_path);
    } else {
        info!("Spindle speed already correct in file: {:?}", file_path);
    }

    Ok(())
}

#[allow(dead_code)]
fn update_spindle_speed_in_content(
    content: &str,
    spindle_speed: u32,
) -> Result<String, SpindleSpeedUpdaterError> {
    let mut updated_lines = Vec::new();
    let mut found_s_command = false;

    for line in content.lines() {
        if found_s_command {
            updated_lines.push(line.to_string());
        } else if line.trim_start().starts_with('S') {
            found_s_command = true;
            updated_lines.push(format!("S{} M3", spindle_speed));
        } else {
            updated_lines.push(line.to_string());
        }
    }

    if !found_s_command {
        return Err(SpindleSpeedUpdaterError::InvalidSpindleSpeed(
            "No S command found in file".to_string(),
        ));
    }

    Ok(updated_lines.join("\n"))
}

#[allow(dead_code)]
async fn update_file_spindle_speed(file_path: &Path, spindle_speed: u32) -> io::Result<bool> {
    let content = tokio::fs::read_to_string(file_path).await?;
    let mut updated_lines = Vec::new();
    let mut found_s_command = false;
    let mut file_updated = false;

    for line in content.lines() {
        if found_s_command {
            updated_lines.push(line.to_string());
        } else if line.trim_start().starts_with('S') {
            found_s_command = true;
            let new_line = format!("S{} M3", spindle_speed);
            if line.trim() != new_line {
                updated_lines.push(new_line);
                file_updated = true;
            } else {
                updated_lines.push(line.to_string());
            }
        } else {
            updated_lines.push(line.to_string());
        }
    }

    if file_updated {
        tokio::fs::write(file_path, updated_lines.join("\n")).await?;
    }

    Ok(file_updated)
}

#[tokio::main]
async fn main() -> Result<(), eframe::Error> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    info!("Application started");

    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(320.0, 240.0)),
        ..Default::default()
    };

    eframe::run_native(
        "Spindle Speed Updater",
        options,
        Box::new(|_cc| Box::new(MainApp::new())),
    )
}
