<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from '@tauri-apps/api/event';

  interface ShortcutConfig {
    _shortcut: string;
    _hold_shortcut: string;
    _cancel_shortcut: string;
    api_key: string;
  }

  let shortcut = $state("");
  let holdShortcut = $state("");
  let cancelShortcut = $state(""); // New cancel shortcut
  let isRecording = $state(false);
  let transcriptions = $state<string[]>([]);
  let errorMessage = $state("");
  let successMessage = $state("");
  let strictTextFieldMode = $state(false);
  
  // State variables for shortcut recording
  let isListeningForShortcut = $state(false);
  let isListeningForHoldShortcut = $state(false);
  let isListeningForCancelShortcut = $state(false); // New state for cancel shortcut

  let unlistenRecordingStatus: (() => void) | null = null;
  let unlistenTranscription: (() => void) | null = null;
  let unlistenError: (() => void) | null = null;
  let unlistenShortcutsUpdated: (() => void) | null = null;
  let unlistenTextFieldDetection: (() => void) | null = null;
  let unlistenRecordingCancelled: (() => void) | null = null; // New listener for cancellation

  let shortcutConfig = {
    _shortcut: "super+KeyG",
    _hold_shortcut: "super+KeyK",
    _cancel_shortcut: "super+KeyC", // Default cancel shortcut
    api_key: ""
  };

  onMount(async () => {
    try {
      shortcutConfig = await invoke("get_shortcut_config");
      shortcut = shortcutConfig._shortcut || "super+KeyG";
      holdShortcut = shortcutConfig._hold_shortcut || "super+KeyK";
      cancelShortcut = shortcutConfig._cancel_shortcut || "super+KeyC";
      console.log("Loaded shortcuts:", shortcut, holdShortcut, cancelShortcut);
    } catch (error) {
      console.error("Failed to get shortcut config:", error);
    }

    // Get the current strict mode setting
    try {
      strictTextFieldMode = await invoke("get_strict_text_field_mode");
    } catch (error) {
      console.error("Failed to get strict mode setting:", error);
    }

    unlistenRecordingStatus = await listen("recording-status", (event) => {
      console.log("Recording status changed:", event.payload);
      isRecording = event.payload as boolean;
    });

    unlistenTranscription = await listen("transcription", (event) => {
      console.log("Transcription received:", event.payload);
      const text = event.payload as string;
      if (text.trim()) {
        transcriptions = [...transcriptions, text];
      }
    });

    // Add listener for error events
    unlistenError = await listen("error", (event) => {
      console.error("Error received:", event.payload);
      errorMessage = event.payload as string;
    });
    
    // Add listener for shortcut updates
    unlistenShortcutsUpdated = await listen("shortcuts-updated", (event) => {
      console.log("Shortcuts updated:", event.payload);
      const data = event.payload as { toggle_shortcut: string, hold_shortcut: string, cancel_shortcut: string };
      shortcut = data.toggle_shortcut;
      holdShortcut = data.hold_shortcut;
      cancelShortcut = data.cancel_shortcut;
    });
    
    // Add listener for text field detection events
    unlistenTextFieldDetection = await listen("text-field-detection", (event) => {
      console.log("Text field detection:", event.payload);
      const data = event.payload as { detected: boolean, message: string };
      if (!data.detected) {
        errorMessage = data.message;
        setTimeout(() => {
          errorMessage = "";
        }, 5000);
      }
    });
    
    // Add listener for recording cancellation
    unlistenRecordingCancelled = await listen("recording-cancelled", () => {
      console.log("Recording was cancelled");
      successMessage = "Recording cancelled";
      setTimeout(() => {
        successMessage = "";
      }, 3000);
    });

    console.log("Event listeners set up");
    
    // Set up keyboard event listener for shortcut recording
    window.addEventListener('keydown', handleKeyDown);
  });

  onDestroy(() => {
    if (unlistenRecordingStatus) unlistenRecordingStatus();
    if (unlistenTranscription) unlistenTranscription();
    if (unlistenError) unlistenError();
    if (unlistenShortcutsUpdated) unlistenShortcutsUpdated();
    if (unlistenTextFieldDetection) unlistenTextFieldDetection();
    if (unlistenRecordingCancelled) unlistenRecordingCancelled();
    
    window.removeEventListener('keydown', handleKeyDown);
  });
  
  // Handle key press for shortcut recording
  function handleKeyDown(event: KeyboardEvent) {
    if (!isListeningForShortcut && !isListeningForHoldShortcut && !isListeningForCancelShortcut) return;
    
    event.preventDefault();
    
    // Build shortcut string (e.g., "super+KeyG")
    const modifiers = [];
    if (event.ctrlKey) modifiers.push('ctrl');
    if (event.metaKey) modifiers.push('super'); // Command key on Mac
    if (event.altKey) modifiers.push('alt');
    if (event.shiftKey) modifiers.push('shift');
    
    // Get the key - if it's a letter, make it KeyX format for consistency
    let key = event.code;
    if (key.startsWith('Key')) {
      // Already in the right format
    } else if (key.length === 1 && /[a-z]/i.test(key)) {
      key = 'Key' + key.toUpperCase();
    }
    
    const newShortcut = [...modifiers, key].join('+');
    
    if (isListeningForShortcut) {
      shortcut = newShortcut;
      isListeningForShortcut = false;
    } else if (isListeningForHoldShortcut) {
      holdShortcut = newShortcut;
      isListeningForHoldShortcut = false;
    } else if (isListeningForCancelShortcut) {
      cancelShortcut = newShortcut;
      isListeningForCancelShortcut = false;
    }
  }
  
  function startListeningForShortcut() {
    isListeningForShortcut = true;
    isListeningForHoldShortcut = false;
    isListeningForCancelShortcut = false;
  }
  
  function startListeningForHoldShortcut() {
    isListeningForHoldShortcut = true;
    isListeningForShortcut = false;
    isListeningForCancelShortcut = false;
  }
  
  function startListeningForCancelShortcut() {
    isListeningForCancelShortcut = true;
    isListeningForShortcut = false;
    isListeningForHoldShortcut = false;
  }
  
  async function toggleStrictMode() {
    try {
      strictTextFieldMode = await invoke("toggle_strict_text_field_mode");
      
      // Show feedback to the user
      if (strictTextFieldMode) {
        successMessage = "Strict text field mode enabled. Text will only be typed if a text field is detected.";
      } else {
        successMessage = "Strict text field mode disabled. Text will be typed regardless of field detection.";
      }
      
      setTimeout(() => {
        successMessage = "";
      }, 3000);
    } catch (error) {
      errorMessage = `Failed to toggle strict mode: ${error}`;
    }
  }

  async function saveConfig(event?: Event) {
    if (event) event.preventDefault();
    errorMessage = "";
    successMessage = "";

    try {
      await invoke("update_shortcut_config", { 
        toggleShortcut: shortcut,
        holdShortcut: holdShortcut,
        cancelShortcut: cancelShortcut
      });
      successMessage = "Shortcuts saved successfully!";
      setTimeout(() => {
        successMessage = "";
      }, 3000);
    } catch (error) {
      errorMessage = `Failed to save shortcuts: ${error}`;
    }
  }

  function clearTranscriptions() {
    transcriptions = [];
  }

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
  
  // Function to format shortcut for display
  function formatShortcut(shortcutStr: string) {
    return shortcutStr
      .replace('super', 'Command')
      .replace('ctrl', 'Ctrl')
      .replace('alt', 'Alt')
      .replace('shift', 'Shift')
      .replace(/Key([A-Z])/g, '$1');
  }
</script>

<main>
  <h1>ReportBlitz</h1>
  <div class="shortcuts-info">
    <p class="description">Press <kbd>{formatShortcut(shortcut)}</kbd> to toggle recording on/off</p>
    <p class="description">Hold <kbd>{formatShortcut(holdShortcut)}</kbd> to record while pressed</p>
    <p class="description">Press <kbd>{formatShortcut(cancelShortcut)}</kbd> to cancel current recording</p>
  </div>

  <div class="status-indicator" class:recording={isRecording}>
    {#if isRecording}
      <div class="recording-icon"></div>
      Recording...
    {:else}
      Ready
    {/if}
  </div>

  <form on:submit|preventDefault={saveConfig}>
    <div class="form-group">
      <label for="shortcut">Toggle Recording Shortcut:</label>
      <div class="shortcut-input-container">
        <input 
          id="shortcut" 
          type="text" 
          bind:value={shortcut} 
          placeholder="Click 'Record' to set shortcut"
          readonly
          class:listening={isListeningForShortcut}
        />
        <button 
          type="button" 
          class="record-btn" 
          on:click={startListeningForShortcut}
          disabled={isListeningForHoldShortcut || isListeningForCancelShortcut}
        >
          {isListeningForShortcut ? 'Press any key...' : 'Record'}
        </button>
      </div>
    </div>
    
    <div class="form-group">
      <label for="holdShortcut">Hold-to-Record Shortcut:</label>
      <div class="shortcut-input-container">
        <input 
          id="holdShortcut" 
          type="text" 
          bind:value={holdShortcut} 
          placeholder="Click 'Record' to set shortcut"
          readonly
          class:listening={isListeningForHoldShortcut}
        />
        <button 
          type="button" 
          class="record-btn" 
          on:click={startListeningForHoldShortcut}
          disabled={isListeningForShortcut || isListeningForCancelShortcut}
        >
          {isListeningForHoldShortcut ? 'Press any key...' : 'Record'}
        </button>
      </div>
    </div>
    
    <!-- New cancel shortcut input -->
    <div class="form-group">
      <label for="cancelShortcut">Cancel Recording Shortcut:</label>
      <div class="shortcut-input-container">
        <input 
          id="cancelShortcut" 
          type="text" 
          bind:value={cancelShortcut} 
          placeholder="Click 'Record' to set shortcut"
          readonly
          class:listening={isListeningForCancelShortcut}
        />
        <button 
          type="button" 
          class="record-btn" 
          on:click={startListeningForCancelShortcut}
          disabled={isListeningForShortcut || isListeningForHoldShortcut}
        >
          {isListeningForCancelShortcut ? 'Press any key...' : 'Record'}
        </button>
      </div>
      <small>Use this shortcut to immediately cancel the current recording without typing text.</small>
    </div>

    <div class="form-group checkbox-group">
      <label for="strictMode" class="checkbox-label">
        <input 
          id="strictMode" 
          type="checkbox" 
          bind:checked={strictTextFieldMode}
          on:change={toggleStrictMode}
        />
        <span>Strict Text Field Mode</span>
      </label>
      <small>
        When enabled, the app will only type text if it detects your cursor is in a text field. 
        When disabled, it will attempt to type text regardless.
      </small>
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

  .shortcuts-info {
    text-align: center;
    margin-bottom: 1rem;
  }

  .shortcuts-info p {
    margin: 0.5rem 0;
    font-size: 0.9rem;
    color: #666;
  }

  .status-indicator {
    text-align: center;
    padding: 0.75rem;
    margin-bottom: 1rem;
    background-color: #eee;
    border-radius: 4px;
    font-weight: bold;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.5rem;
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

  .recording-icon {
    width: 12px;
    height: 12px;
    background-color: #fff;
    border-radius: 50%;
    animation: blink 1s infinite;
  }

  @keyframes blink {
    0% { opacity: 1; }
    50% { opacity: 0.3; }
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
  
  .shortcut-input-container {
    display: flex;
    gap: 0.5rem;
  }
  
  .shortcut-input-container input {
    flex: 1;
  }
  
  .record-btn {
    background-color: #f0f0f0;
    color: #333;
    border: 1px solid #ccc;
  }
  
  .record-btn:hover {
    background-color: #e0e0e0;
  }
  
  input.listening {
    background-color: #fff3cd;
    border-color: #ffeeba;
  }

  small {
    display: block;
    margin-top: 0.25rem;
    font-size: 0.75rem;
    color: #666;
  }
  
  .checkbox-group {
    display: flex;
    flex-direction: column;
    margin-bottom: 1rem;
  }
  
  .checkbox-label {
    display: flex;
    align-items: center;
    cursor: pointer;
    margin-bottom: 0.25rem;
  }
  
  .checkbox-label input {
    width: auto;
    margin-right: 0.5rem;
  }
  
  .checkbox-label span {
    font-weight: 500;
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

  kbd {
    background-color: #f7f7f7;
    border: 1px solid #ccc;
    border-radius: 3px;
    box-shadow: 0 1px 0 rgba(0,0,0,0.2);
    color: #333;
    display: inline-block;
    font-family: monospace;
    font-size: 0.85em;
    font-weight: 700;
    line-height: 1;
    padding: 2px 4px;
    white-space: nowrap;
  }

  @media (prefers-color-scheme: dark) {
    :global(body) {
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
    
    .record-btn {
      background-color: #444;
      color: #f0f0f0;
      border-color: #555;
    }
    
    .record-btn:hover {
      background-color: #555;
    }
    
    input.listening {
      background-color: #3a3a00;
      border-color: #4a4a00;
      color: #ffffcc;
    }

    .transcriptions-list {
      border-color: #333;
    }

    .transcription-item {
      border-bottom-color: #333;
    }

    .shortcuts-info p, small {
      color: #aaa;
    }

    .empty-state {
      color: #777;
    }

    kbd {
      background-color: #333;
      border-color: #555;
      box-shadow: 0 1px 0 rgba(255,255,255,0.1);
      color: #f0f0f0;
    }

    .recording-icon {
      background-color: #fff;
    }
  }
</style>