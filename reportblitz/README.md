# ReportBlitz

ReportBlitz is a simple desktop application that allows you to quickly record audio and transcribe it using OpenAI's Whisper API. It's perfect for taking quick voice notes, transcribing meetings, or dictating text.

## Features

- **Global Keyboard Shortcut**: Press a customizable keyboard shortcut to start and stop recording, no matter which application is in focus.
- **Automatic Transcription**: Audio is automatically transcribed using OpenAI's Whisper API.
- **Continuous Recording**: For longer recordings, the application automatically splits the audio into chunks and transcribes them in real-time.
- **Simple Interface**: Configure your keyboard shortcut and OpenAI API key through a clean, minimal interface.
- **Copy to Clipboard**: Easily copy all transcriptions to your clipboard with a single click.

## Requirements

- An OpenAI API key (get one at [https://platform.openai.com/api-keys](https://platform.openai.com/api-keys))
- A microphone connected to your computer

## Getting Started

1. Launch the application
2. Enter your OpenAI API key in the settings
3. Optionally, customize your keyboard shortcut (default is Command+G on macOS or Control+G on Windows/Linux)
4. Press the keyboard shortcut to start recording
5. Press it again to stop recording
6. View your transcriptions in the application window
7. Use the "Copy All" button to copy all transcriptions to your clipboard

## Development

This application is built with [Tauri](https://tauri.app/), [Svelte](https://svelte.dev/), and [Rust](https://www.rust-lang.org/).

### Prerequisites

- [Node.js](https://nodejs.org/) (v16 or later)
- [Rust](https://www.rust-lang.org/tools/install)
- [pnpm](https://pnpm.io/installation)

### Setup

```bash
# Install dependencies
pnpm install

# Start development server
pnpm tauri dev

# Build for production
pnpm tauri build
```

## License

MIT
