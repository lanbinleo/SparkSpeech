# Development Workflow

This document records the development and release flow for SparkSpeech.

## Branch Model

Use `main` as the stable branch.

Normal development should happen on a version branch:

```powershell
git checkout main
git pull
git checkout -b dev/0.1.1
```

Focused branches can be created from the active version branch:

```powershell
git checkout dev/0.1.1
git checkout -b feat/history-search
git checkout -b fix/overlay-voice-hold
git checkout -b docs/release-process
```

Merge focused branches back into the active `dev/x.y.z` branch, then open a pull request from `dev/x.y.z` into `main` for the release.

Direct commits to `main` should be limited to user-approved documentation-only changes, emergency release fixes, or initial repository setup.

## Commit Style

Use Conventional Commits:

- `feat:` new user-facing behavior
- `fix:` bug fixes
- `docs:` documentation-only changes
- `style:` visual or formatting-only changes without behavior changes
- `refactor:` code restructuring without behavior changes
- `test:` tests or verification helpers
- `chore:` tooling, build, metadata, or maintenance
- `perf:` performance improvements

Keep commits grouped by intent. For example:

```text
docs: add release workflow
feat(ui): add history detail modal
feat(native): add tray integration
fix(asr): handle no-speech results
chore(release): publish 0.1.0
```

## Verification

Run checks based on what changed.

Frontend:

```powershell
npm run build
```

Rust/native:

```powershell
cargo check --manifest-path src-tauri/Cargo.toml
```

Release build:

```powershell
npm run tauri:build -- --no-bundle
```

The Tauri build command currently still creates MSI and NSIS installers even when `--no-bundle` is passed through npm.

Some behavior needs real Windows desktop testing:

- global shortcut while the app is in the background
- transparent recording overlay
- microphone capture
- clipboard copy and automatic paste
- system tray open/quit behavior

## Version Surfaces

When bumping a release, update these together:

- `package.json`
- `package-lock.json`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`
- `docs/release-x.y.z.md`

The visible product version should match the release being prepared.

## Updater Secrets

SparkSpeech uses the official Tauri updater. The updater public key is stored in `src-tauri/tauri.conf.json`.

The private signing key and password are stored in GitHub Actions secrets:

- `TAURI_SIGNING_PRIVATE_KEY`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`

The local signing key is stored outside the repository:

- `%USERPROFILE%\.tauri\sparkspeech-updater.key`
- `%USERPROFILE%\.tauri\sparkspeech-updater.key.password.txt`

If the key ever needs to be regenerated, use:

```powershell
npx tauri signer generate --write-keys "$env:USERPROFILE\.tauri\sparkspeech-updater.key" --force
Get-Content "$env:USERPROFILE\.tauri\sparkspeech-updater.key" -Raw | gh secret set TAURI_SIGNING_PRIVATE_KEY --repo lanbinleo/SparkSpeech
"<password>" | gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --repo lanbinleo/SparkSpeech
```

After regenerating the key, update the `plugins.updater.pubkey` value in `src-tauri/tauri.conf.json`.

## GitHub Actions Release Checklist

1. Start from a clean workspace.
2. Confirm the target version and branch.
3. Update all version surfaces.
4. Write release notes in `docs/release-x.y.z.md`.
5. Run:

```powershell
npm run release:check -- 0.1.1
```

6. For local installer verification, run:

```powershell
npm run release:check -- 0.1.1 -BuildInstaller
```

7. Review the diff.
8. Commit release changes.
9. Push the release branch and open a PR into `main`.
10. After the PR is merged, create and push an annotated tag from `main`:

```powershell
git checkout main
git pull
npm run release:check -- 0.1.1 -Tag -PushTag
```

11. GitHub Actions runs `.github/workflows/release.yml`.
12. Confirm the GitHub Release includes:

- NSIS installer
- MSI installer
- updater artifacts
- `latest.json`

13. Install the previous production version and use Settings -> About -> Check Update to verify the app sees the new release.

## Manual Local Release Build

Use this only for local artifact verification:

```powershell
npm run release:check -- 0.1.1 -BuildInstaller
```

The local build should create installers under `src-tauri\target\release\bundle\`.

## Local Data Migration

Production data normally lives in:

`%APPDATA%\com.leo.sparkspeech`

Codex-launched development builds may store data under:

`%LOCALAPPDATA%\Packages\OpenAI.Codex_2p2nqsd0c76g0\LocalCache\Roaming\com.leo.sparkspeech`

To migrate data from the Codex development directory to the production directory:

```powershell
$source = Join-Path $env:LOCALAPPDATA 'Packages\OpenAI.Codex_2p2nqsd0c76g0\LocalCache\Roaming\com.leo.sparkspeech'
$target = Join-Path $env:APPDATA 'com.leo.sparkspeech'
New-Item -ItemType Directory -Force -Path $target | Out-Null
Get-ChildItem -LiteralPath $source -Force | ForEach-Object {
    Copy-Item -LiteralPath $_.FullName -Destination $target -Recurse -Force
}
```

Close SparkSpeech before copying data so the running app does not rewrite files during migration.

## Definition of Done

- The requested user-facing behavior is implemented.
- Existing local data remains safe.
- Documentation is updated when behavior, storage, release, or development flow changes.
- Verification commands relevant to the change pass.
- Release artifacts are regenerated for release work.
- Git history is grouped into clear Conventional Commits.
