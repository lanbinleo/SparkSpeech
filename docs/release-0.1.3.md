# SparkSpeech 0.1.3 Release Notes

## Highlights

- Improved recording reliability with local recording sessions and periodic audio segment saves during recording.
- Added startup recovery for unfinished recording sessions, so interrupted audio can be preserved as history and reprocessed.
- Added visible overlay progress for recording time, saved audio segments, full-audio ASR, and streaming text optimization.
- Switched normal recording flow to transcribe the complete local WAV after recording ends.
- Added text optimization provider support for OpenRouter, DeepSeek, and custom OpenAI-compatible endpoints.
- Added cleanup strength modes: plain, light cleanup, and deep cleanup.
- Added Windows startup launch setting.
- Added expired recording cleanup and deletion of the local recording file when a history item is deleted.

## Update Notes

Doubao remains the only ASR provider in this release. The 0.1.3 recording flow focuses on making longer dictation safer by saving audio locally throughout the recording and using the final complete WAV for ASR.

Text optimization still uses OpenAI-compatible chat completions. OpenRouter remains supported, and DeepSeek or a custom compatible endpoint can now be selected from the model settings page.

Some behavior should still be checked in a real Windows desktop session after installation:

- Global shortcut and overlay placement.
- Microphone recording and segment save feedback.
- ASR and text optimization progress display.
- Auto-paste into target applications.
- Windows startup launch.

## Verification

Recommended checks before tagging:

```powershell
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run release:check -- 0.1.3 -BuildInstaller
```
