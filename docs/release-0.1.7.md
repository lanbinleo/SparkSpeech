# SparkSpeech 0.1.7 Release Notes

## Highlights

- Added silent startup for launch-at-startup so SparkSpeech opens directly to the tray instead of showing the main window.
- Added audio save status to history records, including saved, pending, failed, expired, and missing states.
- Improved recording flow so transcription can begin while the app clearly tracks whether the audio file has been saved.
- Added timing details to local logs for audio save, Doubao ASR, fast ASR, and text optimization.
- Updated local build guidance to use the direct Rust release build path for ordinary compilation.

## Update Notes

Launch-at-startup now writes a silent startup argument to the Windows Run entry. Manual app launches still show the main window.

Existing history records are normalized when loaded. Records with audio files are treated as saved, records with save-failure errors are marked as save failed, and older records without audio paths are treated as expired.

## Verification

Recommended local checks:

```powershell
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```
