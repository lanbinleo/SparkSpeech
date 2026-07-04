# SparkSpeech 0.1.0

Initial Windows release.

## Included

- Tauri 2 desktop app with React, TypeScript, and Rust.
- Global keyboard hook for configurable recording shortcut.
- Transparent overlay window for recording, ASR, and optimization states.
- Local WAV recording retention with retry support.
- Doubao streaming ASR integration.
- OpenRouter text optimization through the OpenAI-compatible chat completions API.
- Editable system prompt, writing preferences, and replacement dictionary.
- Local history with copy, retry ASR, retry optimization, detail modal, and delete confirmation.
- Clipboard copy and optional automatic paste after optimization.
- Settings for microphone, shortcut capture, theme, logs, and microphone test recording.
- System tray with open and quit actions.
- Light and dark themes with custom scrollbars and motion.
- Local app icon and favicon.

## Local Data

Production data is stored in:

`%APPDATA%\com.leo.sparkspeech`

Development runs launched from Codex may store data under the Codex package cache:

`%LOCALAPPDATA%\Packages\OpenAI.Codex_2p2nqsd0c76g0\LocalCache\Roaming\com.leo.sparkspeech`

## Build Outputs

Release artifacts are generated under:

- `src-tauri/target/release/sparkspeech.exe`
- `src-tauri/target/release/bundle/nsis/SparkSpeech_0.1.0_x64-setup.exe`
- `src-tauri/target/release/bundle/msi/SparkSpeech_0.1.0_x64_en-US.msi`

## Verification

Verified before release:

```powershell
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
npm run tauri:build -- --no-bundle
```

