<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from '@tauri-apps/api/event';

  // State variables
  let shortcut = $state("");
  let apiKey = $state("");
  let isRecording = $state(false);
  let transcriptions = $state<string[]>([]);
  let errorMessage = $state("");
  let successMessage = $state("");

  // Event listeners
  let unlistenRecordingStatus: (() => void) | null = null;
  let unlistenTranscription: (() => void) | null = null;

  onMount(async () => {
    // Get the current shortcut configuration
    try {
      const config = await invoke("get_shortcut_config");
      shortcut = config.shortcut;
      apiKey = config.api_key;
    } catch (error) {
      errorMessage = `Failed to load configuration: ${error}`;
    }

    // Listen for recording status changes
    unlistenRecordingStatus = await listen("recording-status", (event) => {
      isRecording = event.payload as boolean;
    });

    // Listen for transcription events
    unlistenTranscription = await listen("transcription", (event) => {
      const text = event.payload as string;
      if (text.trim()) {
        transcriptions = [...transcriptions, text];
      }
    });
  });

  onDestroy(() => {
    // Clean up event listeners
    if (unlistenRecordingStatus) unlistenRecordingStatus();
    if (unlistenTranscription) unlistenTranscription();
  });

  // Save the shortcut configuration
  async function saveConfig(event: Event) {
    event.preventDefault();
    errorMessage = "";
    successMessage = "";

    try {
      await invoke("update_shortcut_config", { shortcut, apiKey });
      successMessage = "Configuration saved successfully!";
      setTimeout(() => {
        successMessage = "";
      }, 3000);
    } catch (error) {
      errorMessage = `Failed to save configuration: ${error}`;
    }
  }

  // Clear transcriptions
  function clearTranscriptions() {
    transcriptions = [];
  }

  // Copy all transcriptions to clipboard
  function copyToClipboard() {
    if (transcriptions.length === 0) return;
    
    const text = transcriptions.join("\n\n");
    navigator.clipboard.writeText(text).then(() => {
      successMessage = "Transcriptions copied to clipboard!";
      setTimeout(() => {
        successMessage = "";
      }, 3000);
    }).catch(err => {
      errorMessage = `Failed to copy to clipboard: ${err}`;
    });
  }
</script>

<main>
  <h1>ReportBlitz</h1>
  <p class="description">Press {shortcut} to start/stop recording</p>

  <div class="status-indicator" class:recording={isRecording}>
    {isRecording ? 'Recording...' : 'Ready'}
  </div>

  <form on:submit={saveConfig}>
    <div class="form-group">
      <label for="shortcut">Keyboard Shortcut:</label>
      <input 
        id="shortcut" 
        type="text" 
        bind:value={shortcut} 
        placeholder="e.g., CommandOrControl+G"
      />
      <small>Use format like CommandOrControl+G, Alt+R, etc.</small>
    </div>

    <div class="form-group">
      <label for="api-key">OpenAI API Key:</label>
      <input 
        id="api-key" 
        type="password" 
        bind:value={apiKey} 
        placeholder="sk-..."
      />
    </div>

    <button type="submit">Save Configuration</button>
  </form>

  {#if errorMessage}
    <div class="error">{errorMessage}</div>
  {/if}

  {#if successMessage}
    <div class="success">{successMessage}</div>
  {/if}

  <div class="transcriptions-container">
    <div class="transcriptions-header">
      <h2>Transcriptions</h2>
      <div class="transcriptions-actions">
        <button on:click={clearTranscriptions} disabled={transcriptions.length === 0}>Clear</button>
        <button on:click={copyToClipboard} disabled={transcriptions.length === 0}>Copy All</button>
      </div>
    </div>

    <div class="transcriptions-list">
      {#if transcriptions.length === 0}
        <p class="empty-state">No transcriptions yet. Start recording to see them here.</p>
      {:else}
        {#each transcriptions as transcription, i}
          <div class="transcription-item">
            <span class="transcription-number">{i + 1}.</span>
            <p>{transcription}</p>
          </div>
        {/each}
      {/if}
    </div>
  </div>
</main>

<style>
  main {
    max-width: 100%;
    margin: 0 auto;
    padding: 1rem;
    font-family: system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, Cantarell, 'Open Sans', 'Helvetica Neue', sans-serif;
  }

  h1 {
    font-size: 1.5rem;
    margin-bottom: 0.5rem;
    text-align: center;
  }

  .description {
    text-align: center;
    margin-bottom: 1rem;
    font-size: 0.9rem;
    color: #666;
  }

  .status-indicator {
    text-align: center;
    padding: 0.5rem;
    margin-bottom: 1rem;
    background-color: #eee;
    border-radius: 4px;
    font-weight: bold;
  }

  .status-indicator.recording {
    background-color: #ff3e00;
    color: white;
    animation: pulse 1.5s infinite;
  }

  @keyframes pulse {
    0% { opacity: 1; }
    50% { opacity: 0.7; }
    100% { opacity: 1; }
  }

  .form-group {
    margin-bottom: 1rem;
  }

  label {
    display: block;
    margin-bottom: 0.5rem;
    font-weight: 500;
  }

  input {
    width: 100%;
    padding: 0.5rem;
    border: 1px solid #ccc;
    border-radius: 4px;
    font-size: 0.9rem;
  }

  small {
    display: block;
    margin-top: 0.25rem;
    font-size: 0.75rem;
    color: #666;
  }

  button {
    padding: 0.5rem 1rem;
    background-color: #3498db;
    color: white;
    border: none;
    border-radius: 4px;
    cursor: pointer;
    font-size: 0.9rem;
  }

  button:hover {
    background-color: #2980b9;
  }

  button:disabled {
    background-color: #ccc;
    cursor: not-allowed;
  }

  .error {
    margin-top: 1rem;
    padding: 0.5rem;
    background-color: #ffebee;
    color: #c62828;
    border-radius: 4px;
    font-size: 0.9rem;
  }

  .success {
    margin-top: 1rem;
    padding: 0.5rem;
    background-color: #e8f5e9;
    color: #2e7d32;
    border-radius: 4px;
    font-size: 0.9rem;
  }

  .transcriptions-container {
    margin-top: 1.5rem;
    border-top: 1px solid #eee;
    padding-top: 1rem;
  }

  .transcriptions-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 0.5rem;
  }

  .transcriptions-header h2 {
    font-size: 1.2rem;
    margin: 0;
  }

  .transcriptions-actions {
    display: flex;
    gap: 0.5rem;
  }

  .transcriptions-actions button {
    padding: 0.25rem 0.5rem;
    font-size: 0.8rem;
  }

  .transcriptions-list {
    max-height: 200px;
    overflow-y: auto;
    border: 1px solid #eee;
    border-radius: 4px;
    padding: 0.5rem;
  }

  .empty-state {
    color: #999;
    text-align: center;
    font-style: italic;
    font-size: 0.9rem;
  }

  .transcription-item {
    margin-bottom: 0.5rem;
    padding-bottom: 0.5rem;
    border-bottom: 1px solid #eee;
    display: flex;
  }

  .transcription-item:last-child {
    margin-bottom: 0;
    padding-bottom: 0;
    border-bottom: none;
  }

  .transcription-number {
    font-weight: bold;
    margin-right: 0.5rem;
    color: #666;
  }

  .transcription-item p {
    margin: 0;
    flex: 1;
  }

  @media (prefers-color-scheme: dark) {
    body {
      background-color: #1e1e1e;
      color: #f0f0f0;
    }

    .status-indicator {
      background-color: #333;
      color: #f0f0f0;
    }

    input {
      background-color: #333;
      color: #f0f0f0;
      border-color: #555;
    }

    .transcriptions-list {
      border-color: #333;
    }

    .transcription-item {
      border-bottom-color: #333;
    }

    .description, small {
      color: #aaa;
    }

    .empty-state {
      color: #777;
    }
  }
</style>
