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
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tauri_plugin_http::init as http_init;
use tauri_plugin_opener::init as opener_init;
use tauri_plugin_shell::init as shell_init;
use tauri_plugin_store::StoreExt;
use serde_json::json;


// App state structure
use std::sync::atomic::AtomicBool;

pub struct AppState {
    _shortcut: Arc<Mutex<String>>,
    _hold_shortcut: Arc<Mutex<String>>,
    is_recording: Arc<AtomicBool>,
    last_trigger: Arc<Mutex<Option<(String, Instant)>>>,
}

// Function to get the API key from the secure store
// Function to get the API key from the secure store
fn get_api_key(app_handle: &tauri::AppHandle) -> Result<String> {
    // Get the store from the app's resource table - handle the Result
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
        },
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
    api_key: String,
}

#[derive(Deserialize)]
struct TranscriptionResponse {
    text: String,
}

// Command to get the current shortcut configuration
#[tauri::command]
fn get_shortcut_config(state: tauri::State<AppState>) -> ShortcutConfig {
    ShortcutConfig {
        _shortcut: state._shortcut.lock().unwrap().clone(),
        _hold_shortcut: state._hold_shortcut.lock().unwrap().clone(),
        api_key: "".to_string(), // Don't expose API key to frontend
    }
}

// Command to update the shortcut configuration
#[tauri::command]
fn update_shortcut_config(
    _shortcut: String,
    app_handle: tauri::AppHandle,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    let new_shortcut = "super+KeyG".to_string();
    *state._shortcut.lock().unwrap() = new_shortcut.clone();
    app_handle
        .emit("shortcut-updated", &new_shortcut)
        .map_err(|e| e.to_string())?;
    update_global_shortcut(&app_handle, &new_shortcut, "super+KeyK").map_err(|e| e.to_string())?;
    Ok(())
}

// Function to get the path to the .env file in the Resources directory
fn get_env_file_path() -> Result<PathBuf> {
    // Get the executable path
    let current_exe = std::env::current_exe()?;
    let exe_dir = current_exe.parent().ok_or_else(|| anyhow!("Failed to get executable directory"))?;
    
    // For macOS, the Resources directory is at ../Resources relative to the executable
    #[cfg(target_os = "macos")]
    {
        if let Some(resources_dir) = exe_dir
            .parent() // Go up from MacOS
            .and_then(|p| p.parent()) // Go up from Contents
            .map(|p| p.join("Resources")) // Go to Resources
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
fn update_global_shortcut(
    app_handle: &tauri::AppHandle,
    toggle_shortcut: &str,
    hold_shortcut: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "Attempting to register shortcuts: toggle={}, hold={}",
        toggle_shortcut, hold_shortcut
    );
    app_handle.global_shortcut().unregister_all()?;
    app_handle.global_shortcut().register(toggle_shortcut)?;
    println!(
        "Toggle shortcut {} registered successfully",
        toggle_shortcut
    );
    app_handle.global_shortcut().register(hold_shortcut)?;
    println!("Hold shortcut {} registered successfully", hold_shortcut);
    Ok(())
}

use std::time::{Duration, Instant};

use std::sync::atomic::Ordering;

// Then modify your handle_shortcut function
fn handle_shortcut(app_handle: &tauri::AppHandle, shortcut: &Shortcut, state: ShortcutState) {
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

    // For toggle shortcut, only respond to Pressed state (ignore Released)
    if shortcut_str == toggle_shortcut && state == ShortcutState::Pressed {
        println!("Toggle shortcut matched!");

        // Check if we're already recording - if so, stop
        if app_state.is_recording.load(Ordering::SeqCst) {
            println!("Already recording, stopping");
            app_state.is_recording.store(false, Ordering::SeqCst);
            let _ = app_handle.emit("recording-status", false);
        } else {
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
        println!("Hold shortcut matched! State: {:?}", state);

        if state == ShortcutState::Pressed {
            println!("Hold shortcut pressed");
            let is_recording = app_state.is_recording.load(Ordering::SeqCst);
            if !is_recording {
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
fn record_audio_internal(app_handle: tauri::AppHandle) -> Result<()> {
    let state = app_handle.state::<AppState>();
    let is_recording = state.is_recording.clone();

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
            let target_sample_rate = 24000; // 16kHz is sufficient for speech recognition
            
            // Find a suitable configuration with reasonable sample rate
            let supported_config = match device.supported_input_configs() {
                Ok(configs) => {
                    let mut best_config = None;
                    
                    for config_range in configs.filter(|c| c.channels() == 1) {
                        let min_rate = config_range.min_sample_rate().0;
                        let max_rate = config_range.max_sample_rate().0;
                        
                        // Select the config that can support our target rate
                        if min_rate <= target_sample_rate && max_rate >= target_sample_rate {
                            best_config = Some(config_range.with_sample_rate(cpal::SampleRate(target_sample_rate)));
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
            let all_samples: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(capacity)));
            let all_samples_clone = all_samples.clone();

            // Create batching for samples to reduce mutex contention
            let batch_size = sample_rate as usize / 4; // 0.25 seconds worth of samples
            
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
                                batch.extend(data.iter()
                                    .map(|&s| (s as f32 / u16::MAX as f32) * 2.0 - 1.0));
                                
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
                thread::sleep(std::time::Duration::from_millis(100));
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
            match transcribe_audio_data(&wav_data, &app_handle).await {
                Ok(text) => {
                    println!("Transcription succeeded: {}", text);
                    let _ = app_handle.emit("transcription", text);
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
fn record_audio(app_handle: tauri::AppHandle) -> Result<(), String> {
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
async fn transcribe_audio_data(wav_data: &[u8], app_handle: &tauri::AppHandle) -> Result<String> {
    let client = reqwest::Client::new();
    
    // Get the API key from the secure store
    let api_key = get_api_key(app_handle)?;
    
    if api_key.is_empty() {
        return Err(anyhow!("OpenAI API key not set. Please add it to .env or environment variables."));
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
    Ok(transcription.text)
}

// Fix the setup function in run() to handle the Result from store()
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Fixed shortcuts
    let fixed_toggle_shortcut = "super+KeyG".to_string();
    let fixed_hold_shortcut = "super+KeyK".to_string();
    
    let app = tauri::Builder::default()
        .plugin(http_init())
        .plugin(shell_init())
        .plugin(opener_init())
        .plugin(tauri_plugin_store::Builder::default().build())  // Add store plugin
        .setup(move |app| {
            #[cfg(desktop)]
            {
                let app_handle = app.handle();
                
                // Initialize the store and preload the API key - handle the Result
                if let Ok(_store) = app_handle.store("api_keys.dat") {
                    println!("Store initialized successfully");
                } else {
                    println!("Failed to initialize store, will use only env file");
                }
                
                // Try to get API key from environment or .env
                if let Ok(api_key) = get_api_key(&app_handle) {
                    if !api_key.is_empty() {
                        println!("API key initialized successfully");
                    } else {
                        println!("No API key found. Please add it to the environment or .env file.");
                    }
                }
                
                app.manage(AppState {
                    _shortcut: Arc::new(Mutex::new(fixed_toggle_shortcut.clone())),
                    _hold_shortcut: Arc::new(Mutex::new(fixed_hold_shortcut.clone())),
                    is_recording: Arc::new(AtomicBool::new(false)),
                    last_trigger: Arc::new(Mutex::new(None)),
                });
                
                let handle_clone = app_handle.clone();
                app_handle.plugin(
                    tauri_plugin_global_shortcut::Builder::new()
                        .with_handler(move |_app, shortcut, event| {
                            handle_shortcut(&handle_clone, shortcut, event.state());
                        })
                        .build(),
                )?;
                if let Err(e) = update_global_shortcut(&app_handle, &fixed_toggle_shortcut, &fixed_hold_shortcut) {
                    eprintln!("Failed to register global shortcuts: {}", e);
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_shortcut_config,
            update_shortcut_config,
            record_audio
        ])
        .build(tauri::generate_context!())?;
        
    app.run(|_, _| {});
    Ok(())
}