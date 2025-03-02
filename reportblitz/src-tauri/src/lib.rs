//lib.rs
// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use tauri::{Emitter, Manager};
//use tokio::sync::mpsc;
use serde_json::json;
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tauri_plugin_http::init as http_init;
use tauri_plugin_opener::init as opener_init;
use tauri_plugin_shell::init as shell_init;
use tauri_plugin_store::StoreExt;
// Use these v2 imports instead
use tauri::menu::{Menu, MenuEvent, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::Wry;
use tauri::{AppHandle, Runtime};
use enigo::{Enigo, KeyboardControllable};
use std::time::{Duration, Instant};

// App state structure
use std::sync::atomic::AtomicBool;

pub struct AppState {
    _shortcut: Arc<Mutex<String>>,
    _hold_shortcut: Arc<Mutex<String>>,
    _cancel_shortcut: Arc<Mutex<String>>, // New field for cancel shortcut
    is_recording: Arc<AtomicBool>,
    is_cancelled: Arc<AtomicBool>, // Track if recording was cancelled
    last_trigger: Arc<Mutex<Option<(String, Instant)>>>,
    strict_text_field_mode: Arc<AtomicBool>,
}

#[tauri::command]
fn toggle_strict_text_field_mode(
    app_handle: AppHandle<Wry>,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    let current = state.strict_text_field_mode.load(Ordering::SeqCst);
    let new_value = !current;
    state.strict_text_field_mode.store(new_value, Ordering::SeqCst);
    
    // Save the setting
    let store = match app_handle.store("settings.dat") {
        Ok(store) => store,
        Err(e) => return Err(format!("Failed to access settings store: {}", e)),
    };
    
    store.set("strict_text_field_mode", json!(new_value));
    if let Err(e) = store.save() {
        return Err(format!("Failed to save settings: {}", e));
    }
    
    Ok(new_value)
}

// Add this command to get the current mode
#[tauri::command]
fn get_strict_text_field_mode(
    state: tauri::State<'_, AppState>,
) -> bool {
    state.strict_text_field_mode.load(Ordering::SeqCst)
}

// Function to get the API key from the secure store
// Function to get the API key from the secure store
// Update the function signature to use the generic type
fn get_api_key<R: Runtime>(app_handle: &AppHandle<R>) -> Result<String> {
    // The rest of the function remains the same
    let store = match app_handle.store("api_keys.dat") {
        Ok(store) => store,
        Err(e) => {
            println!("Error accessing store: {}", e);
            return Ok(String::new());
        }
    };

    // Try to get the API key from the store
    match store.get("openai_api_key") {
        Some(key) => {
            if let Some(key_str) = key.as_str() {
                if !key_str.is_empty() {
                    println!("API key loaded from secure store");
                    return Ok(key_str.to_string());
                }
            }
        }
        None => (),
    }

    // If not in store, check environment variable
    if let Ok(key) = env::var("OPENAI_API_KEY") {
        if !key.is_empty() {
            // Save to store for future use
            store.set("openai_api_key", json!(key));
            if let Err(e) = store.save() {
                println!("Error saving to store: {}", e);
            }
            println!("API key loaded from environment variable and saved to store");
            return Ok(key);
        }
    }

    // Final fallback: check .env file
    let env_path = get_env_file_path()?;
    if env_path.exists() {
        if let Ok(file) = File::open(env_path) {
            let reader = BufReader::new(file);

            for line_result in reader.lines() {
                if let Ok(line) = line_result {
                    if line.starts_with("OPENAI_API_KEY=") {
                        let key = line.trim_start_matches("OPENAI_API_KEY=").to_string();
                        if !key.is_empty() {
                            // Save to store for future use
                            store.set("openai_api_key", json!(key));
                            if let Err(e) = store.save() {
                                println!("Error saving to store: {}", e);
                            }
                            println!("API key loaded from .env file and saved to store");
                            return Ok(key);
                        }
                    }
                }
            }
        }
    }

    println!("No API key found");
    Ok(String::new())
}

#[derive(Serialize, Deserialize, Clone)]
struct ShortcutConfig {
    _shortcut: String,
    _hold_shortcut: String,
    _cancel_shortcut: String,
    api_key: String,
}

#[derive(Deserialize)]
struct TranscriptionResponse {
    text: String,
}

// Command to get the current shortcut configuration
#[tauri::command]
fn get_shortcut_config(state: tauri::State<'_, AppState>) -> ShortcutConfig {
    ShortcutConfig {
        _shortcut: state._shortcut.lock().unwrap().clone(),
        _hold_shortcut: state._hold_shortcut.lock().unwrap().clone(),
        _cancel_shortcut: state._cancel_shortcut.lock().unwrap().clone(),
        api_key: "".to_string(), // Don't expose API key to frontend
    }
}

// Command to update the shortcut configuration
#[tauri::command]
fn update_shortcut_config(
    toggle_shortcut: String,
    hold_shortcut: String,
    cancel_shortcut: String,
    app_handle: AppHandle<Wry>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    // Validate shortcuts
    if toggle_shortcut.is_empty() || hold_shortcut.is_empty() || cancel_shortcut.is_empty() {
        return Err("Shortcuts cannot be empty".to_string());
    }
    
    // Check for conflicts
    if toggle_shortcut == hold_shortcut || toggle_shortcut == cancel_shortcut || hold_shortcut == cancel_shortcut {
        return Err("All shortcuts must be different".to_string());
    }
    
    // Update the state
    *state._shortcut.lock().unwrap() = toggle_shortcut.clone();
    *state._hold_shortcut.lock().unwrap() = hold_shortcut.clone();
    *state._cancel_shortcut.lock().unwrap() = cancel_shortcut.clone();
    
    // Save to persistent storage
    let store = match app_handle.store("settings.dat") {
        Ok(store) => store,
        Err(e) => return Err(format!("Failed to access settings store: {}", e)),
    };
    
    store.set("toggle_shortcut", json!(toggle_shortcut));
    store.set("hold_shortcut", json!(hold_shortcut));
    store.set("cancel_shortcut", json!(cancel_shortcut));
    
    if let Err(e) = store.save() {
        return Err(format!("Failed to save settings: {}", e));
    }
    
    // Update global shortcuts
    update_global_shortcut(&app_handle, &toggle_shortcut, &hold_shortcut, &cancel_shortcut)
        .map_err(|e| format!("Failed to register shortcuts: {}", e))?;
    
    // Emit an event to inform frontend
    app_handle
        .emit("shortcuts-updated", json!({
            "toggle_shortcut": toggle_shortcut,
            "hold_shortcut": hold_shortcut,
            "cancel_shortcut": cancel_shortcut
        }))
        .map_err(|e| e.to_string())?;
    
    Ok(())
}

// Function to get the path to the .env file in the Resources directory
fn get_env_file_path() -> Result<PathBuf> {
    // Get the executable path
    let current_exe = std::env::current_exe()?;
    let exe_dir = current_exe
        .parent()
        .ok_or_else(|| anyhow!("Failed to get executable directory"))?;

    // For macOS, the Resources directory is at ../Resources relative to the executable
    #[cfg(target_os = "macos")]
    {
        if let Some(resources_dir) = exe_dir
            .parent() // Go up from MacOS
            .and_then(|p| p.parent()) // Go up from Contents
            .map(|p| p.join("Resources"))
        // Go to Resources
        {
            let env_path = resources_dir.join(".env");
            println!("Looking for .env file at: {:?}", env_path);
            if env_path.exists() {
                return Ok(env_path);
            }
        }
    }

    // For Windows/Linux, resources are typically in the same directory as the executable
    #[cfg(not(target_os = "macos"))]
    {
        let resources_dir = exe_dir.join("resources");
        if resources_dir.exists() {
            let env_path = resources_dir.join(".env");
            println!("Looking for .env file at: {:?}", env_path);
            if env_path.exists() {
                return Ok(env_path);
            }
        }
    }

    // Development path - check src-tauri directory
    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".env");
    println!("Looking for .env file at: {:?}", dev_path);
    if dev_path.exists() {
        return Ok(dev_path);
    }

    // Fallback to executable directory
    let default_path = exe_dir.join(".env");
    println!("No .env file found, using default path: {:?}", default_path);
    Ok(default_path)
}

// Function to update the global shortcut
fn update_global_shortcut<R: Runtime>(
    app_handle: &AppHandle<R>,
    toggle_shortcut: &str,
    hold_shortcut: &str,
    cancel_shortcut: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "Attempting to register shortcuts: toggle={}, hold={}, cancel={}",
        toggle_shortcut, hold_shortcut, cancel_shortcut
    );
    
    app_handle.global_shortcut().unregister_all()?;
    app_handle.global_shortcut().register(toggle_shortcut)?;
    app_handle.global_shortcut().register(hold_shortcut)?;
    app_handle.global_shortcut().register(cancel_shortcut)?;
    
    println!("All shortcuts registered successfully");
    Ok(())
}

use std::sync::atomic::Ordering;

// Then modify your handle_shortcut function
fn handle_shortcut<R: Runtime>(
    app_handle: &AppHandle<R>,
    shortcut: &Shortcut,
    state: ShortcutState,
) {
    let now = Instant::now();
    let shortcut_str = shortcut.to_string();
    let app_state = app_handle.state::<AppState>();

    // Use thread-safe debounce check
    let should_process = {
        let mut last_trigger = app_state.last_trigger.lock().unwrap();
        if let Some((last_shortcut, last_time)) = &*last_trigger {
            if last_shortcut == &shortcut_str
                && now.duration_since(*last_time) < Duration::from_millis(300)
            {
                println!("Debounced duplicate trigger for: {}", shortcut_str);
                return;
            }
        }
        *last_trigger = Some((shortcut_str.clone(), now));
        true
    };

    if !should_process {
        return;
    }

    let toggle_shortcut = app_state._shortcut.lock().unwrap().clone();
    let hold_shortcut = app_state._hold_shortcut.lock().unwrap().clone();
    let cancel_shortcut = app_state._cancel_shortcut.lock().unwrap().clone();

    // Handle cancel shortcut (only respond to Pressed state)
    if shortcut_str == cancel_shortcut && state == ShortcutState::Pressed {
        println!("Cancel shortcut triggered!");
        
        // If we're recording, cancel it
        if app_state.is_recording.load(Ordering::SeqCst) {
            println!("Cancelling current recording");
            app_state.is_cancelled.store(true, Ordering::SeqCst); // Mark as cancelled
            app_state.is_recording.store(false, Ordering::SeqCst); // Stop recording
            let _ = app_handle.emit("recording-status", false);
            let _ = app_handle.emit("recording-cancelled", true);
        }
        return;
    }

    // Original toggle shortcut handler
    if shortcut_str == toggle_shortcut && state == ShortcutState::Pressed {
        println!("Toggle shortcut matched!");

        // Check if we're already recording - if so, stop
        if app_state.is_recording.load(Ordering::SeqCst) {
            println!("Already recording, stopping");
            app_state.is_recording.store(false, Ordering::SeqCst);
            let _ = app_handle.emit("recording-status", false);
        } else {
            // Reset cancelled state
            app_state.is_cancelled.store(false, Ordering::SeqCst);
            
            // If not recording, start
            println!("Not recording, starting");
            app_state.is_recording.store(true, Ordering::SeqCst);
            let _ = app_handle.emit("recording-status", true);

            let app_handle_clone = app_handle.clone();
            thread::spawn(move || {
                if let Err(e) = record_audio_internal(app_handle_clone) {
                    eprintln!("Error recording audio: {}", e);
                }
            });
        }
    } else if shortcut_str == hold_shortcut {
        // Original hold shortcut handler
        println!("Hold shortcut matched! State: {:?}", state);

        if state == ShortcutState::Pressed {
            println!("Hold shortcut pressed");
            let is_recording = app_state.is_recording.load(Ordering::SeqCst);
            if !is_recording {
                // Reset cancelled state
                app_state.is_cancelled.store(false, Ordering::SeqCst);
                
                app_state.is_recording.store(true, Ordering::SeqCst);
                let _ = app_handle.emit("recording-status", true);
                println!("Recording started");
                let app_handle_clone = app_handle.clone();
                thread::spawn(move || {
                    if let Err(e) = record_audio_internal(app_handle_clone) {
                        eprintln!("Error recording audio: {}", e);
                    }
                });
            }
        } else if state == ShortcutState::Released {
            println!("Hold shortcut released");
            let is_recording = app_state.is_recording.load(Ordering::SeqCst);
            if is_recording {
                app_state.is_recording.store(false, Ordering::SeqCst);
                let _ = app_handle.emit("recording-status", false);
                println!("Recording stopped");
            }
        }
    }
}


// Optimized record_audio_internal function
fn record_audio_internal<R: Runtime>(app_handle: AppHandle<R>) -> Result<()> {
    let state = app_handle.state::<AppState>();
    let is_recording = state.is_recording.clone();
    let is_cancelled = state.is_cancelled.clone();

    // Create a single thread for audio processing
    thread::spawn(move || {
        // Set up runtime inside thread for async operations
        let runtime = tokio::runtime::Runtime::new().unwrap();

        runtime.block_on(async {
            // Initialize audio
            let host = cpal::default_host();
            let device = match host.default_input_device() {
                Some(device) => device,
                None => {
                    eprintln!("No input device available");
                    let _ = app_handle.emit("error", "No microphone found");
                    return;
                }
            };

            // For optimization, we'll use a fixed configuration that's good enough for speech
            // instead of always using the maximum sample rate
            let target_sample_rate = 17000; // 16kHz is sufficient for speech recognition

            // Find a suitable configuration with reasonable sample rate
            let supported_config = match device.supported_input_configs() {
                Ok(configs) => {
                    let mut best_config = None;

                    for config_range in configs.filter(|c| c.channels() == 1) {
                        let min_rate = config_range.min_sample_rate().0;
                        let max_rate = config_range.max_sample_rate().0;

                        // Select the config that can support our target rate
                        if min_rate <= target_sample_rate && max_rate >= target_sample_rate {
                            best_config = Some(
                                config_range.with_sample_rate(cpal::SampleRate(target_sample_rate)),
                            );
                            break;
                        }
                    }

                    // If we didn't find a config that supports our exact target,
                    // just choose one with the closest sample rate
                    if best_config.is_none() {
                        best_config = match device.supported_input_configs() {
                            Ok(configs) => {
                                configs
                                    .filter(|c| c.channels() == 1)
                                    .min_by_key(|c| {
                                        let rate = if c.max_sample_rate().0 < target_sample_rate {
                                            c.max_sample_rate().0
                                        } else {
                                            c.min_sample_rate().0
                                        };
                                        (target_sample_rate as i32 - rate as i32).abs()
                                    })
                                    .map(|c| {
                                        // Choose the closest available sample rate
                                        if c.min_sample_rate().0 > target_sample_rate {
                                            c.with_sample_rate(c.min_sample_rate())
                                        } else if c.max_sample_rate().0 < target_sample_rate {
                                            c.with_sample_rate(c.max_sample_rate())
                                        } else {
                                            c.with_sample_rate(cpal::SampleRate(target_sample_rate))
                                        }
                                    })
                            }
                            Err(_) => None,
                        };
                    }

                    match best_config {
                        Some(config) => config,
                        None => {
                            eprintln!("No suitable input config found");
                            let _ = app_handle.emit("error", "Microphone configuration error");
                            return;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error getting supported configs: {}", e);
                    let _ = app_handle.emit("error", format!("Microphone error: {}", e));
                    return;
                }
            };

            println!("Selected input config: {:?}", supported_config);

            let config = supported_config.config();
            let sample_format = supported_config.sample_format();
            let sample_rate = config.sample_rate.0;

            // Use a more efficient buffer approach to reduce mutex contention
            // Pre-allocate with a reasonable size based on typical recording duration
            let capacity = sample_rate as usize * 60; // 1 minute of audio at our sample rate
            let all_samples: Arc<Mutex<Vec<f32>>> =
                Arc::new(Mutex::new(Vec::with_capacity(capacity)));
            let all_samples_clone = all_samples.clone();

            // Create batching for samples to reduce mutex contention
            let batch_size = sample_rate as usize / 10; // 0.25 seconds worth of samples

            // Error callback
            let err_fn = |err| {
                eprintln!("Stream error: {:?}", err);
            };

            // Only log once every 2 seconds
            let log_interval = std::time::Duration::from_secs(2);
            let _last_log = std::time::Instant::now();

            // Create and start the stream based on sample format
            let stream = match sample_format {
                SampleFormat::F32 => {
                    let samples = all_samples.clone();
                    let is_rec = is_recording.clone();
                    let mut batch: Vec<f32> = Vec::with_capacity(batch_size);
                    let last_log_time = Arc::new(Mutex::new(std::time::Instant::now()));

                    let callback = move |data: &[f32], _: &_| {
                        if is_rec.load(Ordering::SeqCst) {
                            if !data.is_empty() {
                                // Collect samples to batch before locking mutex
                                batch.extend_from_slice(data);

                                // Only lock the mutex and push when we have a full batch
                                if batch.len() >= batch_size {
                                    let now = std::time::Instant::now();
                                    let mut last_log = last_log_time.lock().unwrap();
                                    if now.duration_since(*last_log) >= log_interval {
                                        println!("Audio batch collected: {} samples", batch.len());
                                        *last_log = now;
                                    }

                                    let mut buffer = samples.lock().unwrap();
                                    buffer.append(&mut batch);

                                    // Reset the batch with the same capacity
                                    batch = Vec::with_capacity(batch_size);
                                }
                            }
                        }
                    };

                    match device.build_input_stream(&config, callback, err_fn, None) {
                        Ok(stream) => stream,
                        Err(e) => {
                            eprintln!("Failed to build input stream: {}", e);
                            let _ = app_handle.emit("error", format!("Microphone error: {}", e));
                            return;
                        }
                    }
                }
                SampleFormat::I16 => {
                    let samples = all_samples.clone();
                    let is_rec = is_recording.clone();
                    let mut batch: Vec<f32> = Vec::with_capacity(batch_size);
                    let last_log_time = Arc::new(Mutex::new(std::time::Instant::now()));

                    let callback = move |data: &[i16], _: &_| {
                        if is_rec.load(Ordering::SeqCst) {
                            if !data.is_empty() {
                                // Convert and collect samples in batch
                                batch.extend(data.iter().map(|&s| s as f32 / i16::MAX as f32));

                                // Only lock the mutex when batch is full
                                if batch.len() >= batch_size {
                                    let now = std::time::Instant::now();
                                    let mut last_log = last_log_time.lock().unwrap();
                                    if now.duration_since(*last_log) >= log_interval {
                                        println!("Audio batch collected: {} samples", batch.len());
                                        *last_log = now;
                                    }

                                    let mut buffer = samples.lock().unwrap();
                                    buffer.append(&mut batch);

                                    // Reset the batch
                                    batch = Vec::with_capacity(batch_size);
                                }
                            }
                        }
                    };

                    match device.build_input_stream(&config, callback, err_fn, None) {
                        Ok(stream) => stream,
                        Err(e) => {
                            eprintln!("Failed to build input stream: {}", e);
                            let _ = app_handle.emit("error", format!("Microphone error: {}", e));
                            return;
                        }
                    }
                }
                SampleFormat::U16 => {
                    let samples = all_samples.clone();
                    let is_rec = is_recording.clone();
                    let mut batch: Vec<f32> = Vec::with_capacity(batch_size);
                    let last_log_time = Arc::new(Mutex::new(std::time::Instant::now()));

                    let callback = move |data: &[u16], _: &_| {
                        if is_rec.load(Ordering::SeqCst) {
                            if !data.is_empty() {
                                // Convert and collect samples in batch
                                batch.extend(
                                    data.iter()
                                        .map(|&s| (s as f32 / u16::MAX as f32) * 2.0 - 1.0),
                                );

                                // Only lock the mutex when batch is full
                                if batch.len() >= batch_size {
                                    let now = std::time::Instant::now();
                                    let mut last_log = last_log_time.lock().unwrap();
                                    if now.duration_since(*last_log) >= log_interval {
                                        println!("Audio batch collected: {} samples", batch.len());
                                        *last_log = now;
                                    }

                                    let mut buffer = samples.lock().unwrap();
                                    buffer.append(&mut batch);

                                    // Reset the batch
                                    batch = Vec::with_capacity(batch_size);
                                }
                            }
                        }
                    };

                    match device.build_input_stream(&config, callback, err_fn, None) {
                        Ok(stream) => stream,
                        Err(e) => {
                            eprintln!("Failed to build input stream: {}", e);
                            let _ = app_handle.emit("error", format!("Microphone error: {}", e));
                            return;
                        }
                    }
                }
                _ => {
                    eprintln!("Unsupported sample format");
                    let _ = app_handle.emit("error", "Unsupported audio format");
                    return;
                }
            };

            println!("Audio stream built successfully, playing...");
            if let Err(e) = stream.play() {
                eprintln!("Failed to play stream: {}", e);
                let _ = app_handle.emit("error", format!("Microphone error: {}", e));
                return;
            }

            println!("Audio processing thread started");

            // Loop until recording is stopped
            while is_recording.load(Ordering::SeqCst) {
                thread::sleep(std::time::Duration::from_millis(10));
            }

            // Check if recording was cancelled
            if is_cancelled.load(Ordering::SeqCst) {
                println!("Recording was cancelled, skipping transcription");
                return;
            }

            println!("Recording stopped, processing samples...");

            // Take ownership of the collected samples
            let samples = {
                let mut buffer = all_samples_clone.lock().unwrap();
                std::mem::take(&mut *buffer)
            };

            println!("Total samples collected: {}", samples.len());

            if samples.is_empty() {
                println!("No samples were collected, skipping file write and transcription.");
                return;
            }

            // Check again if recording was cancelled
            if is_cancelled.load(Ordering::SeqCst) {
                println!("Recording was cancelled, skipping transcription");
                return;
            }

            // Memory-based WAV creation
            let spec = hound::WavSpec {
                channels: 1,
                sample_rate,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };

            // Create WAV file in memory
            let mut cursor = std::io::Cursor::new(Vec::new());
            let mut writer = match hound::WavWriter::new(&mut cursor, spec) {
                Ok(writer) => writer,
                Err(e) => {
                    eprintln!("Error creating WAV writer: {}", e);
                    return;
                }
            };

            // Write samples to memory buffer
            for &sample in &samples {
                if let Err(e) = writer.write_sample((sample * i16::MAX as f32) as i16) {
                    eprintln!("Error writing sample: {}", e);
                    return;
                }
            }

            // Finalize the WAV data in memory - STORE THE RESULT OF FINALIZE
            let finalize_result = writer.finalize();
            if let Err(e) = finalize_result {
                eprintln!("Error finalizing WAV writer: {}", e);
                return;
            }

            // Get the WAV data
            let wav_data = cursor.into_inner();
            println!("WAV data created in memory, size: {} bytes", wav_data.len());

            // Free up memory we don't need anymore
            drop(samples);

            // Use the in-memory data for transcription directly
            if is_cancelled.load(Ordering::SeqCst) {
                println!("Recording was cancelled, skipping transcription");
                return;
            }
            
            match transcribe_audio_data(&wav_data, &app_handle).await {
                Ok(text) => {
                    // Skip typing if cancelled after transcription completed
                    if !is_cancelled.load(Ordering::SeqCst) {
                        println!("Transcription succeeded: {}", text);
                        //let _ = app_handle.emit("transcription", text);
                    } else {
                        println!("Transcription completed but cancelled, not typing text");
                    }
                }
                Err(e) => {
                    eprintln!("Transcription error: {}", e);
                    let _ = app_handle.emit("error", format!("Transcription error: {}", e));
                }
            }
        });
    });

    Ok(())
}

// Command to record audio
#[tauri::command]
fn record_audio(app_handle: AppHandle<Wry>) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    let is_recording = state.is_recording.clone(); // Clone the Arc to use in this function

    if is_recording.load(Ordering::SeqCst) {
        is_recording.store(false, Ordering::SeqCst);
        app_handle
            .emit("recording-status", false)
            .map_err(|e| e.to_string())?;
        return Ok(());
    }

    is_recording.store(true, Ordering::SeqCst);
    app_handle
        .emit("recording-status", true)
        .map_err(|e| e.to_string())?;

    let app_handle_clone = app_handle.clone();

    thread::spawn(move || {
        if let Err(e) = record_audio_internal(app_handle_clone) {
            eprintln!("Error recording audio: {}", e);
        }
    });

    Ok(())
}

// Add this function right after transcribe_audio_with_data or replace that function
async fn transcribe_audio_data<R: Runtime>(
    wav_data: &[u8],
    app_handle: &AppHandle<R>,
) -> Result<String> {
    let client = reqwest::Client::new();

    // Get the API key from the secure store
    let api_key = get_api_key(app_handle)?;

    if api_key.is_empty() {
        return Err(anyhow!(
            "OpenAI API key not set. Please add it to .env or environment variables."
        ));
    }

    let form = reqwest::multipart::Form::new().text("model", "whisper-1");

    // Create a file part from the memory buffer
    let file_part = reqwest::multipart::Part::bytes(wav_data.to_vec())
        .file_name("recording.wav")
        .mime_str("audio/wav")?;

    let form = form.part("file", file_part);

    // Set a reasonable timeout for the request
    let response = client
        .post("https://api.openai.com/v1/audio/transcriptions")
        .header("Authorization", format!("Bearer {}", api_key))
        .timeout(std::time::Duration::from_secs(30)) // 30 second timeout
        .multipart(form)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow!(
            "API request failed with status {}: {}",
            response.status(),
            response.text().await?
        ));
    }

    let transcription: TranscriptionResponse = response.json().await?;
    
    // Send the text to UI
    let _ = app_handle.emit("transcription", &transcription.text);
    
    // Type the text at the cursor position
    let text_to_type = transcription.text.clone();
    let app_handle_clone = app_handle.clone();
    thread::spawn(move || {
        // Wait a moment before starting to type
        thread::sleep(Duration::from_millis(0));
        
        if let Err(e) = type_text_at_cursor(&text_to_type, &app_handle_clone) {
            eprintln!("Error typing text: {}", e);
        } else {
            println!("Successfully typed text at cursor");
        }
    });
    
    Ok(transcription.text)
}

// Define event IDs for menu items
const TRAY_RECORD_AUDIO: &str = "tray-record-audio";
const TRAY_SHOW_WINDOW: &str = "tray-show-window";
const TRAY_HIDE_WINDOW: &str = "tray-hide-window";
const TRAY_QUIT: &str = "tray-quit";
const TRAY_SEPARATOR: &str = "tray-separator";

// Create a function to set up the tray menu for Tauri v2
fn create_tray_menu<R: Runtime>(app_handle: &AppHandle<R>) -> Menu<R> {
    // Create menu items - note we pass app_handle to build()
    let record = MenuItemBuilder::with_id(TRAY_RECORD_AUDIO, "Record Audio")
        .build(app_handle)
        .unwrap();
    let show = MenuItemBuilder::with_id(TRAY_SHOW_WINDOW, "Show Window")
        .build(app_handle)
        .unwrap();
    let hide = MenuItemBuilder::with_id(TRAY_HIDE_WINDOW, "Hide Window")
        .build(app_handle)
        .unwrap();

    // Create a separator - in v2 we use a MenuItemBuilder with an empty label
    let separator = MenuItemBuilder::with_id(TRAY_SEPARATOR, "")
        .build(app_handle)
        .unwrap();

    let quit = MenuItemBuilder::with_id(TRAY_QUIT, "Quit")
        .build(app_handle)
        .unwrap();

    // Fixed: Pass app_handle as first argument and borrow the array
    Menu::with_items(app_handle, &[&record, &show, &hide, &separator, &quit]).unwrap()
}

// Set up the tray icon
fn setup_system_tray(app_handle: &AppHandle<Wry>) -> Result<(), Box<dyn std::error::Error>> {
    // Create the tray menu
    let tray_menu = create_tray_menu(app_handle);

    // Build the tray icon
    TrayIconBuilder::new()
        .menu(&tray_menu)
        .icon_as_template(true)
        .build(app_handle)?;

    // Set up the menu event handler using on_menu_event
    app_handle.on_menu_event(move |app, event| {
        handle_tray_event(app, event);
    });

    Ok(())
}

// Handle menu events
fn handle_tray_event(app: &AppHandle<Wry>, event: MenuEvent) {
    match event.id.as_ref() {
        TRAY_QUIT => {
            std::process::exit(0);
        }
        TRAY_RECORD_AUDIO => {
            // Call the record function
            if let Err(e) = record_audio(app.clone()) {
                eprintln!("Error starting recording from tray: {}", e);
            }
        }
        TRAY_SHOW_WINDOW => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        TRAY_HIDE_WINDOW => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.hide();
            }
        }
        _ => {}
    }
}


fn type_text_at_cursor(text: &str, app_handle: &AppHandle<impl Runtime>) -> Result<()> {
    // Get AppState to check if strict mode is enabled
    let app_state = app_handle.state::<AppState>();
    let strict_mode = app_state.strict_text_field_mode.load(Ordering::SeqCst);
    
    // If strict mode is enabled, try to detect if we're in a text field
    if strict_mode && !is_likely_text_field() {
        println!("Strict mode enabled and no text field detected, skipping text insertion");
        
        // Notify the user
        let _ = app_handle.emit("text-field-detection", json!({
            "detected": false,
            "message": "No text field detected. Move your cursor to a text field and try again."
        }));
        
        return Ok(());
    }
    
    // Create a new Enigo instance for keyboard control
    let mut enigo = Enigo::new();
    
    // Small delay to ensure the application is ready
    //thread::sleep(Duration::from_millis(200));
    
    // Type the text character by character with a small delay between each
    for character in text.chars() {
        // Convert each character to a string before typing
        enigo.key_sequence(&character.to_string());
        
        // Small delay to prevent overwhelming the target application
        // Different systems might need different delays
        thread::sleep(Duration::from_millis(0));
    }
    
    Ok(())
}

fn is_likely_text_field() -> bool {
    // Platform-specific detection when possible
    #[cfg(target_os = "macos")]
    {
        // On macOS, try to use AppleScript for more accurate detection
        if let Ok(output) = std::process::Command::new("osascript")
            .arg("-e")
            .arg(r#"
                tell application "System Events"
                    set frontApp to name of first application process whose frontmost is true
                    set frontAppPath to path of application file of first application process whose frontmost is true
                    return frontApp & ":" & frontAppPath
                end tell
            "#)
            .output()
        {
            let output_str = String::from_utf8_lossy(&output.stdout).to_lowercase();
            
            // Check for common text editors and applications
            let common_text_apps = [
                "textedit", "notes", "pages", "word", "sublime", 
                "vscode", "visual studio code", "textmate", "terminal",
                "iterm", "chrome", "safari", "firefox", "slack", "discord",
                "outlook", "mail", "evernote", "notion", "google docs"
            ];
            
            // Check if any of these apps are in the output
            if common_text_apps.iter().any(|&app| output_str.contains(app)) {
                return true;
            }
            
            // If we get output but didn't match known apps, try one more check
            // This attempts to determine if the focused element can accept text input
            if let Ok(element_info) = std::process::Command::new("osascript")
                .arg("-e")
                .arg(r#"
                    tell application "System Events"
                        set focusedElement to focused object of first application process whose frontmost is true
                        set elementRole to role of focusedElement
                        return elementRole
                    end tell
                "#)
                .output()
            {
                let element_role = String::from_utf8_lossy(&element_info.stdout).to_lowercase();
                
                // Common roles that can accept text
                let text_roles = [
                    "text field", "text area", "editor", "document", "sheet",
                    "textfield", "textarea"
                ];
                
                if text_roles.iter().any(|&role| element_role.contains(role)) {
                    return true;
                }
            }
        }
    }
    
    #[cfg(target_os = "windows")]
    {
        // On Windows, we could use the Windows API to check the focused control
        // This is a simplified version - in a full implementation you'd use winapi
        // to call GetForegroundWindow and related APIs
        
        // For now, we'll use a basic approach with PowerShell
        if let Ok(output) = std::process::Command::new("powershell")
            .arg("-Command")
            .arg("Get-Process | Where-Object {$_.MainWindowHandle -ne 0 -and $_.MainWindowTitle -ne ''} | Select-Object -First 1 | Select-Object -ExpandProperty ProcessName")
            .output()
        {
            let app_name = String::from_utf8_lossy(&output.stdout).to_lowercase();
            let common_text_apps = [
                "notepad", "word", "excel", "outlook", "code", "sublime_text",
                "chrome", "firefox", "edge", "teams", "slack", "discord",
                "powershell", "cmd", "windowsterminal", "putty", "terminal"
            ];
            
            if common_text_apps.iter().any(|&app| app_name.contains(app)) {
                return true;
            }
        }
    }
    
    #[cfg(target_os = "linux")]
    {
        // On Linux, we could use tools like xdotool or gdbus to check active windows
        if let Ok(output) = std::process::Command::new("sh")
            .arg("-c")
            .arg("xdotool getwindowfocus getwindowname 2>/dev/null || echo ''")
            .output()
        {
            let window_name = String::from_utf8_lossy(&output.stdout).to_lowercase();
            let common_text_apps = [
                "terminal", "gnome-terminal", "konsole", "xterm", "gedit",
                "kate", "libreoffice", "firefox", "chrome", "chromium",
                "code", "sublime", "atom", "emacs", "vim", "discord",
                "slack", "telegram", "document", "editor", "text"
            ];
            
            if common_text_apps.iter().any(|&app| window_name.contains(app)) {
                return true;
            }
        }
    }
    
    // If we couldn't detect with platform-specific methods, fall back to true
    // to not block transcription - users can enable strict mode if needed
    true
}


// Add a function to load shortcuts from storage
fn load_shortcuts_from_storage<R: Runtime>(app_handle: &AppHandle<R>) -> (String, String, String) {
    // Default shortcuts
    #[cfg(target_os = "windows")]
    let default_toggle = "ctrl+KeyG".to_string();
    #[cfg(not(target_os = "windows"))]
    let default_toggle = "super+KeyG".to_string();

    #[cfg(target_os = "windows")]
    let default_hold = "ctrl+KeyK".to_string();
    #[cfg(not(target_os = "windows"))]
    let default_hold = "super+KeyK".to_string();

    #[cfg(target_os = "windows")]
    let default_cancel = "ctrl+KeyC".to_string();
    #[cfg(not(target_os = "windows"))]
    let default_cancel = "super+KeyC".to_string();
    
    // Try to get from store
    match app_handle.store("settings.dat") {
        Ok(store) => {
            let toggle = store.get("toggle_shortcut")
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or(default_toggle.clone());
                
            let hold = store.get("hold_shortcut")
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or(default_hold.clone());
                
            let cancel = store.get("cancel_shortcut")
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or(default_cancel.clone());
                
            (toggle, hold, cancel)
        }
        Err(_) => (default_toggle, default_hold, default_cancel),
    }
}

// Update the run function to load settings
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let app = tauri::Builder::default()
        .plugin(http_init())
        .plugin(shell_init())
        .plugin(opener_init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .setup(move |app| {
            #[cfg(desktop)]
            {
                let app_handle = app.handle();

                // Initialize the store and preload the API key
                if let Ok(_store) = app_handle.store("api_keys.dat") {
                    println!("Store initialized successfully");
                } else {
                    println!("Failed to initialize store, will use only env file");
                }
                
                // Initialize settings store
                let _ = app_handle.store("settings.dat");

                // Try to get API key from environment or .env
                if let Ok(api_key) = get_api_key(&app_handle) {
                    if !api_key.is_empty() {
                        println!("API key initialized successfully");
                    } else {
                        println!(
                            "No API key found. Please add it to the environment or .env file."
                        );
                    }
                }

                // Load shortcuts from storage
                let (toggle_shortcut, hold_shortcut, cancel_shortcut) = load_shortcuts_from_storage(&app_handle);
                println!(
                    "Loaded shortcuts - toggle: {}, hold: {}, cancel: {}",
                    toggle_shortcut, hold_shortcut, cancel_shortcut
                );

                // Set up system tray
                if let Err(e) = setup_system_tray(&app_handle) {
                    eprintln!("Failed to set up system tray: {}", e);
                }

                app.manage(AppState {
                    _shortcut: Arc::new(Mutex::new(toggle_shortcut.clone())),
                    _hold_shortcut: Arc::new(Mutex::new(hold_shortcut.clone())),
                    _cancel_shortcut: Arc::new(Mutex::new(cancel_shortcut.clone())),
                    is_recording: Arc::new(AtomicBool::new(false)),
                    is_cancelled: Arc::new(AtomicBool::new(false)), // Track cancellation state
                    last_trigger: Arc::new(Mutex::new(None)),
                    strict_text_field_mode: Arc::new(AtomicBool::new(false)),
                });

                let handle_clone = app_handle.clone();
                app_handle.plugin(
                    tauri_plugin_global_shortcut::Builder::new()
                        .with_handler(move |_app, shortcut, event| {
                            handle_shortcut(&handle_clone, shortcut, event.state());
                        })
                        .build(),
                )?;
                
                // Register shortcuts from storage
                if let Err(e) = update_global_shortcut(
                    &app_handle,
                    &toggle_shortcut,
                    &hold_shortcut,
                    &cancel_shortcut
                ) {
                    eprintln!("Failed to register global shortcuts: {}", e);
                }

                // Configure window to hide instead of close
                if let Some(window) = app_handle.get_webview_window("main") {
                    // Use a new variable to avoid moving the window
                    let window_clone = window.clone();
                    window.on_window_event(move |event| {
                        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                            // Hide window instead of closing
                            let _ = window_clone.hide();
                            api.prevent_close();
                        }
                    });
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_shortcut_config,
            update_shortcut_config,
            record_audio,
            toggle_strict_text_field_mode,
            get_strict_text_field_mode
        ])
        .build(tauri::generate_context!())?;

    app.run(|_, _| {});
    Ok(())
}