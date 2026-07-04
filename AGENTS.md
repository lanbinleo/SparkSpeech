# SparkSpeech Agent Notes

## Project

SparkSpeech is a Windows-first personal dictation app built with Tauri 2, React, TypeScript, and Rust.

The app is intentionally small:
- Global shortcut starts and stops recording.
- A transparent bottom overlay shows recording and processing status.
- Audio is saved locally before any network work.
- Doubao streaming ASR turns audio into text.
- OpenRouter-compatible chat completions clean up the ASR text.
- The final text is copied to the clipboard and can be auto-pasted.

## Product Rules

- Windows is the only supported platform for now.
- Doubao is the only ASR provider for `0.1.x`.
- OpenRouter is the only optimization provider for `0.1.x`.
- OpenRouter should use the system proxy by default.
- Recordings must be saved locally before transcription starts.
- A failed transcription must not delete audio.
- No-speech transcription is not an error. Store it as a history record with the `no_speech` status.
- Deleting a history item must ask for confirmation.
- The app should keep running in the system tray after the main window is closed.

## Local Data

The production app stores data under the Tauri app data directory, normally:

`%APPDATA%\com.leo.sparkspeech`

Important files:
- `settings.json`
- `prompts.json`
- `records.json`
- `app.log`
- `recordings/YYYY-MM-DD/*.wav`
- `tests/microphone-test.wav`

## Development

Use these commands from the repository root:

```powershell
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
npm run tauri:build -- --no-bundle
```

The npm/Tauri command currently still creates MSI and NSIS bundles even when `--no-bundle` is passed through npm.

## Coding Guidelines

- Keep changes small and aligned with the existing app structure.
- Prefer Tauri commands for native behavior and keep React focused on UI state.
- Do not delete user recordings or configuration during normal fixes.
- Use structured JSON storage consistently until the project migrates to SQLite.
- Do not commit generated `dist/`, `node_modules/`, `output/`, or Rust `target/`.
- Update docs when product behavior changes.

