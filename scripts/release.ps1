param(
    [Parameter(Mandatory = $true)]
    [string]$Version,

    [switch]$BuildInstaller,
    [switch]$Tag,
    [switch]$PushTag
)

$ErrorActionPreference = "Stop"

function Step($Message) {
    Write-Host ""
    Write-Host "==> $Message" -ForegroundColor Cyan
}

function Require-CleanWorktree {
    $status = git status --short
    if ($status) {
        Write-Host $status
        throw "Worktree is not clean. Commit or stash changes before releasing."
    }
}

function Read-Json($Path) {
    Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
}

function Assert-Version($Name, $Actual, $Expected) {
    if ($Actual -ne $Expected) {
        throw "$Name version is '$Actual', expected '$Expected'."
    }
}

Step "Checking repository state"
git rev-parse --show-toplevel | Out-Null
Require-CleanWorktree

Step "Checking version surfaces"
$packageJson = Read-Json "package.json"
$tauriConfig = Read-Json "src-tauri/tauri.conf.json"
$cargoToml = Get-Content -LiteralPath "src-tauri/Cargo.toml" -Raw

Assert-Version "package.json" $packageJson.version $Version
Assert-Version "tauri.conf.json" $tauriConfig.version $Version

if ($cargoToml -notmatch "version\s*=\s*`"$([regex]::Escape($Version))`"") {
    throw "src-tauri/Cargo.toml version does not match '$Version'."
}

$releaseNotes = "docs/release-$Version.md"
if (!(Test-Path -LiteralPath $releaseNotes)) {
    throw "Missing release notes: $releaseNotes"
}

Step "Running verification"
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml

if ($BuildInstaller) {
    Step "Building signed Tauri installers"
    $keyPath = Join-Path $env:USERPROFILE ".tauri\sparkspeech-updater.key"
    $passwordPath = Join-Path $env:USERPROFILE ".tauri\sparkspeech-updater.key.password.txt"

    if (!(Test-Path -LiteralPath $keyPath)) {
        throw "Missing updater signing key: $keyPath"
    }
    if (!(Test-Path -LiteralPath $passwordPath)) {
        throw "Missing updater signing key password: $passwordPath"
    }

    $env:TAURI_SIGNING_PRIVATE_KEY = Get-Content -LiteralPath $keyPath -Raw
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = Get-Content -LiteralPath $passwordPath -Raw
    npm run tauri:build

    $artifacts = @(
        "src-tauri/target/release/sparkspeech.exe",
        "src-tauri/target/release/bundle/nsis/SparkSpeech_${Version}_x64-setup.exe",
        "src-tauri/target/release/bundle/nsis/SparkSpeech_${Version}_x64-setup.exe.sig",
        "src-tauri/target/release/bundle/msi/SparkSpeech_${Version}_x64_en-US.msi",
        "src-tauri/target/release/bundle/msi/SparkSpeech_${Version}_x64_en-US.msi.sig"
    )

    foreach ($artifact in $artifacts) {
        if (!(Test-Path -LiteralPath $artifact)) {
            throw "Missing release artifact: $artifact"
        }
    }
}

if ($Tag) {
    Step "Creating annotated tag"
    $tagName = "v$Version"
    $existing = git tag --list $tagName
    if ($existing) {
        throw "Tag already exists: $tagName"
    }
    git tag -a $tagName -m "SparkSpeech $Version"

    if ($PushTag) {
        Step "Pushing tag"
        git push origin $tagName
    } else {
        Write-Host "Tag created locally. Push with: git push origin $tagName"
    }
}

Step "Release checks completed"
