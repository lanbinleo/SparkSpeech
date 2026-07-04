# SparkSpeech 0.1.1 Release Notes

## Highlights

- Fixed Doubao streaming ASR retry behavior by marking the final audio chunk as the last packet instead of sending an empty final packet.
- Added local audio playback and “open recording folder” actions in transcription details.
- Improved failure handling when transcription or optimization fails after recording is saved.
- Added Settings tabs and an About tab with update checking.
- Added Tauri updater support for GitHub Releases.
- Added GitHub Actions release workflow for Windows builds and updater metadata.
- Refined the recording overlay, history loading state, toasts, settings layout, theme controls, and duration display.
- Added README presentation with product screenshot and project overview.

## Update Notes

SparkSpeech now checks for updates from GitHub Releases:

```text
https://github.com/lanbinleo/SparkSpeech/releases/latest/download/latest.json
```

The updater requires signed release artifacts. The public updater key is stored in `src-tauri/tauri.conf.json`; the private key and password are stored as GitHub Actions secrets.

## Verification

Recommended checks before tagging:

```powershell
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```

For release artifacts, push a `v0.1.1` tag and let GitHub Actions build and publish the release.
