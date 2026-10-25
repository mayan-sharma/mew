# Mew

Hold a key, talk, let go. Mew types what you said into whatever app you're in. Everything runs on your computer.

A small cat sits above the taskbar to show Mew is running: asleep while waiting, ears up while listening, a speech bubble with what it typed.

## Using it

| Do this | What happens |
|---|---|
| Hold **Right Alt**, talk, release | Mew types it into the focused app |
| Double-tap **Right Alt** | Hands-free: keep talking, press once more to finish |
| **Esc** while it's listening | Cancel |
| Click the tray cat, or launch Mew again | Opens the Mew window |
| Right-click the tray cat | Talk key, microphone, AI tidy-up, start with Windows, quit |

The **Mew window** has four pages:

- **Overview**: time saved, words, streak, typical wait, words per day (30 days), top apps, latest transcripts.
- **History**: every transcript grouped by day, searchable (Ctrl+F); copy, view what was heard before tidy-up, delete (with Undo).
- **Dictionary**: names and jargon Mew should always spell your way.
- **Settings**: talk key, microphone, AI tidy-up, how long to keep transcripts (default 30 days), start with Windows, updates, licenses.

**AI tidy-up** removes "um"s, applies "no wait, I mean…" corrections and fixes punctuation. Turn it off to get exactly what was heard.

## Install

Run `Mew-win-Setup.exe`. No admin prompt. Mew starts, downloads its models once (about 1.9 GB, with progress shown by the cat) and starts with Windows from then on. Uninstall from Windows Settings > Apps, which also removes the models.

## How it works

| Step | What runs |
|---|---|
| Hotkey | Low-level keyboard hook; the talk key never reaches other apps |
| Microphone | WASAPI via `cpal`, open only while you hold the key, resampled to 16 kHz |
| Speech to text | **Parakeet TDT 0.6B v3** (int8 ONNX) in ONNX Runtime on the CPU (~0.4 s per 12 s of speech) |
| Tidy-up | **Qwen3.5 2B** (Q4_K_M) in a bundled `llama-server` (Vulkan GPU, else CPU). Starts when you press the key, unloads after 10 idle minutes, dies with Mew |
| Typing | Clipboard + Ctrl+V; your clipboard is restored and dictations stay out of Win+V history |
| Overlay | Layered window drawn with `tiny-skia`; never takes focus, hides during fullscreen apps |
| Mew window | WebView2 (via `wry`) showing `ui/` (Preact + Vite, built to one HTML file embedded in mew.exe). Talks to Rust over `window.ipc` (`src/api.rs`). Closing it ends its WebView processes, so it costs nothing while closed |
| History | SQLite (`mew.db`, `src/history.rs`): one row per dictation. Text expires per the "Keep transcripts" setting; numbers are kept for stats |
| Install/updates | Velopack (per-user, updates from GitHub Releases once `update::FEED` is set) |

Safety net for the tidy-up: short phrases skip it, and if the model's output looks like an answer instead of a cleanup, Mew types the original transcript.

Files: app in `%LOCALAPPDATA%\Mew`, models/settings/dictionary/logs in `%LOCALAPPDATA%\Mew Data`.

## Build

Needs Rust (stable MSVC), Visual Studio C++ build tools, Node.js, and `dotnet tool install -g vpk`.

```powershell
npm --prefix ui ci; npm --prefix ui run build   # the window's page; cargo embeds ui/dist/index.html
cargo build --release
powershell -ExecutionPolicy Bypass -File scripts\package.ps1   # does both, -> dist\releases\Mew-win-Setup.exe
```

Developer commands (`mew.exe <flag>`):

- `--download`: fetch models with console progress
- `--selftest <16k-mono.wav>`: run speech-to-text and tidy-up on a file and print timings (types nothing)
- `--render <dir>`: write PNGs of every overlay state
- `--write-icon <file.ico>`: regenerate `assets/mew.ico` from the cat drawing
- `--seed-demo`: fill a scratch history with sample dictations (requires `MEW_DATA_DIR`, never touches real data)

`MEW_DATA_DIR` points Mew at a different data folder (settings, history, models, logs) for development.

Running from `target\release` needs `MEW_LLAMA_DIR` pointing at a folder with `llama-server.exe` (the package script stages it into `llama\` next to `mew.exe`).

## Before a public release

- Code-sign `Mew-win-Setup.exe` and `mew.exe` (unsigned apps that hook the keyboard trip SmartScreen and antivirus heuristics).
- Set `update::FEED` to the GitHub repo and upload `dist\releases` with `vpk upload github`.

Third-party licenses: `assets/NOTICE.txt` (shipped next to `mew.exe`, opened by tray > About).
