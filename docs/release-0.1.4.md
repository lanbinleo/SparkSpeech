# SparkSpeech 0.1.4 Release Notes

## Highlights

- Added an experimental fast ASR finalization mode.
- Fast ASR starts a Doubao streaming session during recording, sends the final audio tail when recording stops, and uses the final streaming result when it arrives quickly.
- If the streaming WebSocket disconnects, times out, or returns no usable final text, SparkSpeech keeps the saved WAV and uses the existing complete-audio ASR flow.
- Added a recording setting for local audio segment save interval: 5, 10, 15, 20, 25, or 30 seconds.
- Added a separate realtime subtitle preview setting. Fast ASR can run without showing subtitle text in the overlay.
- Kept the normal post-recording overlay phases for saving, transcribing, and optimizing.
- Smoothed overlay progress changes so fast ASR progress visually catches up instead of jumping straight to the latest backend value.

## Update Notes

The fast ASR finalization setting is experimental and disabled by default. It does not replace the complete local WAV flow; it only tries to shorten the wait after recording when the streaming session remains healthy.

Realtime subtitle preview is also disabled by default. The preview text shown during recording is temporary and is not used as the final ASR result. SparkSpeech only uses the final response from the streaming session after the last audio packet has been sent.

When a streaming session disconnects during recording, SparkSpeech does not reconnect or retry that session. The app waits until recording ends, then transcribes the complete saved WAV with the existing flow.

## Verification

Recommended checks before tagging:

```powershell
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run release:check -- 0.1.4 -BuildInstaller
```
