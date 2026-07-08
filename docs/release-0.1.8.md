# SparkSpeech 0.1.8 Release Notes

## Highlights

- Added a recording animation sensitivity setting for low-volume microphones.
- Replaced the native Windows title bar with SparkSpeech window controls.

## Update Notes

The new sensitivity setting only changes the bottom recording animation. It does not change recorded audio, transcription input, or recognition results.

Closing the main window still keeps SparkSpeech running in the system tray.

## Verification

Local checks completed:

```powershell
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run release:check -- 0.1.8 -BuildInstaller
```
