//lib.rs
// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::{fs, thread};
use tauri::{Emitter, Manager};
//use tokio::sync::mpsc;
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tauri_plugin_http::init as http_init;
use tauri_plugin_opener::init as opener_init;
use tauri_plugin_shell::init as shell_init;
use tauri::Wry;
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

// Function to load API key from .env file
fn load_api_key() -> Result<String> {
    // First try to load from environment variable
    if let Ok(key) = env::var("OPENAI_API_KEY") {
        if !key.is_empty() {
            return Ok(key);
        }
    }

    // Then try to load from .env file
    let env_path = get_env_file_path()?;
    if env_path.exists() {
        let file = File::open(env_path)?;
        let reader = BufReader::new(file);

        for line in reader.lines() {
            let line = line?;
            if line.starts_with("OPENAI_API_KEY=") {
                let key = line.trim_start_matches("OPENAI_API_KEY=").to_string();
                if !key.is_empty() {
                    return Ok(key);
                }
            }
        }
    }

    Ok(String::new())
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

// Function to get the temp directory for storing audio files
fn get_temp_dir() -> Result<PathBuf> {
    let temp_dir = std::env::temp_dir().join("reportblitz");

    if !temp_dir.exists() {
        fs::create_dir_all(&temp_dir)?;
    }

    Ok(temp_dir)
}

// Internal functio// Here's the fixed version for the audio callbacks, using a simple counter instead of thread ID
fn record_audio_internal(app_handle: tauri::AppHandle) -> Result<()> {
    let state = app_handle.state::<AppState>();
    let is_recording = state.is_recording.clone();

    // This thread will handle audio recording and stream management
    // but won't try to move the Stream between threads
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

            println!(
                "Using input device: {}",
                match device.name() {
                    Ok(name) => name,
                    Err(_) => "Unknown Device".to_string(),
                }
            );

            // Get supported configs
            let supported_configs = match device.supported_input_configs() {
                Ok(configs) => configs,
                Err(e) => {
                    eprintln!("Error getting input configs: {}", e);
                    let _ = app_handle.emit("error", format!("Microphone error: {}", e));
                    return;
                }
            };

            println!("Available input configurations:");
            for config in supported_configs {
                println!("  {:?}", config);
            }

            // Select a configuration
            let supported_config = match device.supported_input_configs() {
                Ok(configs) => {
                    match configs
                        .filter(|c| c.channels() == 1)
                        .max_by_key(|c| c.max_sample_rate().0)
                    {
                        Some(config) => config.with_max_sample_rate(),
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

            // Create a buffer to collect samples, staying in this thread
            let all_samples: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
            let all_samples_clone = all_samples.clone();

            // Create a counter for throttling log messages
            let callback_counter = Arc::new(Mutex::new(0));

            // Error callback
            let err_fn = |err| {
                eprintln!("Stream error: {:?}", err);
            };

            // Create and start the stream based on sample format
            let stream = match sample_format {
                SampleFormat::F32 => {
                    let samples = all_samples.clone();
                    let is_rec = is_recording.clone();
                    let counter = callback_counter.clone();
                    let callback = move |data: &[f32], _: &_| {
                        if is_rec.load(Ordering::SeqCst) {
                            // Throttle logging - only log every 100 callbacks
                            let should_log = {
                                let mut count = counter.lock().unwrap();
                                *count += 1;
                                if *count >= 100 {
                                    *count = 0;
                                    true
                                } else {
                                    false
                                }
                            };

                            if should_log && !data.is_empty() {
                                println!("Audio F32 callback: {} samples", data.len());
                            }

                            if !data.is_empty() {
                                let mut buffer = samples.lock().unwrap();
                                buffer.extend_from_slice(data);
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
                    let counter = callback_counter.clone();
                    let callback = move |data: &[i16], _: &_| {
                        if is_rec.load(Ordering::SeqCst) {
                            // Throttle logging - only log every 100 callbacks
                            let should_log = {
                                let mut count = counter.lock().unwrap();
                                *count += 1;
                                if *count >= 100 {
                                    *count = 0;
                                    true
                                } else {
                                    false
                                }
                            };

                            if should_log && !data.is_empty() {
                                println!("Audio I16 callback: {} samples", data.len());
                            }

                            if !data.is_empty() {
                                let data_vec: Vec<f32> =
                                    data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                                let mut buffer = samples.lock().unwrap();
                                buffer.extend(data_vec);
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
                    let counter = callback_counter.clone();
                    let callback = move |data: &[u16], _: &_| {
                        if is_rec.load(Ordering::SeqCst) {
                            // Throttle logging - only log every 100 callbacks
                            let should_log = {
                                let mut count = counter.lock().unwrap();
                                *count += 1;
                                if *count >= 100 {
                                    *count = 0;
                                    true
                                } else {
                                    false
                                }
                            };

                            if should_log && !data.is_empty() {
                                println!("Audio U16 callback: {} samples", data.len());
                            }

                            if !data.is_empty() {
                                let data_vec: Vec<f32> = data
                                    .iter()
                                    .map(|&s| (s as f32 / u16::MAX as f32) * 2.0 - 1.0)
                                    .collect();
                                let mut buffer = samples.lock().unwrap();
                                buffer.extend(data_vec);
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
            // IMPORTANT: Keep checking is_recording while the stream is active
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

            // Stream will be dropped when it goes out of scope at the end of this block
            // We don't need to explicitly drop it

            if samples.is_empty() {
                println!("No samples were collected, skipping file write and transcription.");
                return;
            }

            // The important part: We're staying in the same thread, so we can
            // directly process the samples without thread safety issues

            let temp_dir = match get_temp_dir() {
                Ok(dir) => dir,
                Err(e) => {
                    eprintln!("Failed to get temp dir: {}", e);
                    return;
                }
            };

            let wav_path = temp_dir.join("recording.wav");

            if let Err(e) = write_wav_file(&wav_path, &samples, sample_rate) {
                eprintln!("Error writing WAV file: {}", e);
                return;
            }

            println!("WAV file written successfully to {:?}", wav_path);
            match transcribe_audio(&wav_path, &app_handle).await {
                Ok(text) => {
                    println!("Transcription succeeded: {}", text);
                    let _ = app_handle.emit("transcription", text);
                    let _ = fs::remove_file(&wav_path);
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

// Function to write a WAV file
fn write_wav_file(path: &Path, samples: &[f32], sample_rate: u32) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer = hound::WavWriter::create(path, spec)?;

    for &sample in samples {
        writer.write_sample((sample * i16::MAX as f32) as i16)?;
    }

    writer.finalize()?;
    Ok(())
}

// Function to transcribe audio using OpenAI API
async fn transcribe_audio(audio_path: &Path, app_handle: &tauri::AppHandle) -> Result<String> {
    let client = reqwest::Client::new();
    
    // Get the API key from the secure store
    let api_key = get_api_key(app_handle)?;
    
    if api_key.is_empty() {
        return Err(anyhow!("OpenAI API key not set. Please add it to .env or environment variables."));
    }

    let form = reqwest::multipart::Form::new().text("model", "whisper-1");

    // Create a file part
    let file_data = tokio::fs::read(audio_path).await?;
    let file_part = reqwest::multipart::Part::bytes(file_data)
        .file_name(
            audio_path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string(),
        )
        .mime_str("audio/wav")?;

    let form = form.part("file", file_part);

    let response = client
        .post("https://api.openai.com/v1/audio/transcriptions")
        .header("Authorization", format!("Bearer {}", api_key))
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
                if let Ok(store) = app_handle.store("api_keys.dat") {
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