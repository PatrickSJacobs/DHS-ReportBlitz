// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::{fs, thread};
use tauri::{Manager, Emitter};
use tokio::sync::mpsc;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

// App state structure
struct AppState {
    shortcut: Arc<Mutex<String>>,
    is_recording: Arc<Mutex<bool>>,
    rx: Arc<Mutex<Option<mpsc::Receiver<Vec<f32>>>>>,
}

#[derive(Serialize, Deserialize, Clone)]
struct ShortcutConfig {
    shortcut: String,
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
        shortcut: state.shortcut.lock().unwrap().clone(),
        api_key: String::new(), // API key is stored elsewhere or retrieved from secure storage
    }
}

// Command to update the shortcut configuration
#[tauri::command]
fn update_shortcut_config(
    shortcut: String,
    _api_key: String,
    app_handle: tauri::AppHandle,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    // Update the state
    *state.shortcut.lock().unwrap() = shortcut.clone();
    
    // Emit event for shortcut update
    app_handle.emit("shortcut-updated", shortcut).map_err(|e| e.to_string())?;
    
    // Update the global shortcut
    update_global_shortcut(&app_handle, &shortcut).map_err(|e| e.to_string())?;
    
    Ok(())
}

// Function to update the global shortcut
fn update_global_shortcut(app_handle: &tauri::AppHandle, shortcut_str: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Unregister all existing shortcuts
    app_handle.global_shortcut().unregister_all()?;
    
    // Register the new shortcut
    let shortcut = Shortcut::from(shortcut_str);
    app_handle.global_shortcut().register(shortcut)?;
    
    Ok(())
}

// Function to handle the global shortcut
fn handle_shortcut(app_handle: &tauri::AppHandle) {
    let state = app_handle.state::<AppState>();
    let mut is_recording = state.is_recording.lock().unwrap();
    
    // Toggle recording state
    *is_recording = !*is_recording;
    
    // Emit recording status event
    let _ = app_handle.emit("recording-status", *is_recording);
    
    if *is_recording {
        // Start recording
        let app_handle_clone = app_handle.clone();
        thread::spawn(move || {
            if let Err(e) = record_audio_internal(app_handle_clone) {
                eprintln!("Error recording audio: {}", e);
            }
        });
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

// Internal function to record audio
fn record_audio_internal(app_handle: tauri::AppHandle) -> Result<()> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or_else(|| anyhow!("No input device available"))?;
    
    println!("Using input device: {}", device.name()?);
    
    let config = device.default_input_config()?;
    println!("Default input config: {:?}", config);
    
    let sample_format = config.sample_format();
    let config = cpal::StreamConfig::from(config);
    
    let (tx, rx) = mpsc::channel(1024);
    
    // Store the receiver in the app state
    {
        let state = app_handle.state::<AppState>();
        let mut rx_guard = state.rx.lock().unwrap();
        *rx_guard = Some(rx);
    }
    
    let err_fn = |err| eprintln!("An error occurred on the audio stream: {}", err);
    
    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(
            &config,
            move |data: &[f32], _: &_| {
                let data_vec = data.to_vec();
                if let Err(e) = tx.try_send(data_vec) {
                    if !matches!(e, mpsc::error::TrySendError::Full(_)) {
                        eprintln!("Failed to send audio data: {}", e);
                    }
                }
            },
            err_fn,
            None,
        )?,
        SampleFormat::I16 => device.build_input_stream(
            &config,
            move |data: &[i16], _: &_| {
                let data_vec: Vec<f32> = data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                if let Err(e) = tx.try_send(data_vec) {
                    if !matches!(e, mpsc::error::TrySendError::Full(_)) {
                        eprintln!("Failed to send audio data: {}", e);
                    }
                }
            },
            err_fn,
            None,
        )?,
        SampleFormat::U16 => device.build_input_stream(
            &config,
            move |data: &[u16], _: &_| {
                let data_vec: Vec<f32> = data.iter().map(|&s| (s as f32 / u16::MAX as f32) * 2.0 - 1.0).collect();
                if let Err(e) = tx.try_send(data_vec) {
                    if !matches!(e, mpsc::error::TrySendError::Full(_)) {
                        eprintln!("Failed to send audio data: {}", e);
                    }
                }
            },
            err_fn,
            None,
        )?,
        _ => return Err(anyhow!("Unsupported sample format")),
    };
    
    stream.play()?;
    
    // Process audio data in a separate thread
    let app_handle_clone = app_handle.clone();
    let sample_rate = config.sample_rate;
    
    thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        
        runtime.block_on(async {
            let temp_dir = get_temp_dir().unwrap();
            let wav_path = temp_dir.join("recording.wav");
            let mut all_samples = Vec::new();
            
            loop {
                let state = app_handle_clone.state::<AppState>();
                let is_recording = *state.is_recording.lock().unwrap();
                
                if !is_recording {
                    break;
                }
                
                let mut rx_guard = state.rx.lock().unwrap();
                if let Some(rx) = rx_guard.as_mut() {
                    match rx.try_recv() {
                        Ok(data) => all_samples.extend(data),
                        Err(mpsc::error::TryRecvError::Empty) => (),
                        Err(e) => {
                            eprintln!("Error receiving audio data: {}", e);
                            break;
                        }
                    }
                }
                
                thread::sleep(std::time::Duration::from_millis(10));
            }
            
            if !all_samples.is_empty() {
                // Write WAV file
                if let Err(e) = write_wav_file(&wav_path, &all_samples, sample_rate.0) {
                    eprintln!("Error writing WAV file: {}", e);
                    return;
                }
                
                // Transcribe audio
                match transcribe_audio(&wav_path).await {
                    Ok(text) => {
                        let _ = app_handle_clone.emit("transcription", text);
                    }
                    Err(e) => {
                        let _ = app_handle_clone.emit("error", format!("Transcription error: {}", e));
                    }
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
    let mut is_recording = state.is_recording.lock().unwrap();
    
    if *is_recording {
        *is_recording = false;
        app_handle.emit("recording-status", false).map_err(|e| e.to_string())?;
        return Ok(());
    }
    
    *is_recording = true;
    app_handle.emit("recording-status", true).map_err(|e| e.to_string())?;
    
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
async fn transcribe_audio(audio_path: &Path) -> Result<String> {
    let client = reqwest::Client::new();
    let api_key = std::env::var("OPENAI_API_KEY").map_err(|_| anyhow!("OPENAI_API_KEY not set"))?;
    
    let form = reqwest::multipart::Form::new()
        .text("model", "whisper-1");
    
    // Create a file part
    let file_data = tokio::fs::read(audio_path).await?;
    let file_part = reqwest::multipart::Part::bytes(file_data)
        .file_name(audio_path.file_name().unwrap().to_string_lossy().to_string())
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_global_shortcut::init())
        .manage(AppState {
            shortcut: Arc::new(Mutex::new("CommandOrControl+G".to_string())),
            is_recording: Arc::new(Mutex::new(false)),
            rx: Arc::new(Mutex::new(None)),
        })
        .invoke_handler(tauri::generate_handler![
            get_shortcut_config,
            update_shortcut_config,
            record_audio
        ])
        .setup(|app| {
            #[cfg(desktop)]
            {
                let app_handle = app.handle();
                let state = app.state::<AppState>();
                let shortcut_str = state.shortcut.lock().unwrap().clone();
                
                // Register the initial shortcut
                if let Err(e) = update_global_shortcut(&app_handle, &shortcut_str) {
                    eprintln!("Failed to register global shortcut: {}", e);
                }
                
                // Set up a handler for the shortcut
                let app_handle_clone = app_handle.clone();
                app_handle.listen_global("tauri://global-shortcut", move |_| {
                    handle_shortcut(&app_handle_clone);
                });
            }
            
            Ok(())
        })
        .build(tauri::generate_context!())?;
        
    app.run(|_, _| {});
    Ok(())
}
