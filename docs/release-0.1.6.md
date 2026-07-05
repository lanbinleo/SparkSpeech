# SparkSpeech 0.1.6 Release Notes

## Highlights

- Improved overlay positioning so status changes no longer visibly jump to the corner during recording transitions.
- Moved the overlay slightly higher above the Windows taskbar and reduced the realtime preview block size.
- Improved Right Alt responsiveness by triggering on key down and keeping the frontend shortcut listener stable.
- Added a clear notice when a new recording is requested while the previous recording is still being processed.
- Reorganized the Settings page into General, Recording, Logs, and About tabs.

## Update Notes

SparkSpeech still processes one recording in the foreground at a time. If transcription or optimization is still running, pressing the shortcut shows a notice instead of starting a second recording. Background processing for older recordings is tracked in the roadmap.

General settings now contains theme, auto-paste, and launch-at-startup options. Recording settings contains microphone, shortcut, retention, segment saving, fast ASR, and realtime subtitle options.

## Verification

Recommended local checks:

```powershell
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```
