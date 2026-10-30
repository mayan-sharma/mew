# Mew

Hold a key, talk, let go. Mew types what you said into whatever app you're in. Everything runs on your computer. Windows and macOS (Apple Silicon).

A small cat sits above the taskbar (on macOS, at the bottom of the screen) to show Mew is running: asleep while waiting, ears up while listening, a speech bubble with what it typed.

## Using it

| Do this | What happens |
|---|---|
| Hold **Right Alt** (macOS: **Fn**), talk, release | Mew types it into the focused app |
| Double-tap the talk key | Hands-free: keep talking, press once more to finish |
| **Shift** + talk key | Types your last dictation again, for when it landed in the wrong place |
| **Esc** while it's listening | Cancel |
| Click the tray cat, or launch Mew again | Opens the Mew window (macOS: the menu bar cat opens its menu; choose Open Mew) |
| Right-click the tray cat | Talk key, microphone, AI tidy-up, start at login, quit |

The talk keys on offer are Right Alt, Right Ctrl, Caps Lock and F8 on Windows; Fn, Right Option and Right Command on macOS. On macOS, holding the talk key with another key (a shortcut such as Fn+Delete) cancels quietly and the shortcut goes through.

**Fn on macOS:** set System Settings › Keyboard › "Press 🌐 key to" to "Do Nothing", and don't use "Press 🌐 twice" as the Dictation shortcut, or macOS reacts to the key too.

The **Mew window** has four pages:

- **Overview**: time saved, words, streak, typical wait, words per day (30 days), top apps, latest transcripts.
- **History**: every transcript grouped by day, searchable (Ctrl+F, ⌘F on macOS); copy, view what was heard before tidy-up, delete (with Undo).
- **Dictionary**: names and jargon Mew should always spell your way. Capitals and spacing are fixed in everything it types ("github" and "git hub" become "GitHub"), with tidy-up on or off. Everyday words ("Will") and short symbol terms ("C++") are only passed to tidy-up as a hint, since forcing them would change other text. Words it hears as something else altogether are covered by Learn from my edits.
- **Settings**: talk key, microphone, AI tidy-up, how long to keep transcripts (default 30 days), start at login, automatic update checks, licenses.

**AI tidy-up** removes "um"s, applies "no wait, I mean…" corrections and fixes punctuation. Turn it off to get exactly what was heard.

If Mew can't type (for example, the clipboard is busy), it keeps the dictation: it's in History, and Shift + talk key types it once you've clicked into a text box. With "Keep transcripts" set to "Don't keep", the last dictation stays in memory only, for 10 minutes.

**Learn from my edits**: after Mew types, it watches that one text box for 30 seconds (via UI Automation on Windows, the Accessibility API on macOS, never password fields). If you fix a misheard word, say "cooper netties" to "Kubernetes", Mew remembers the pair, replaces it in future dictations (even with tidy-up off) and adds the word to your dictionary. Rewrites, added words and swaps to everyday words are ignored. Learned pairs are listed, and removable, on the Dictionary page.

## Install

Downloads are on the [Releases page](https://github.com/mayan-sharma/mew/releases). Neither build is signed by Microsoft or Apple yet, so each OS warns you once.

**Windows:** run `Mew-win-Setup.exe`. If "Windows protected your PC" appears, click **More info**, then **Run anyway**. No admin prompt. Mew starts, downloads its models once (about 1.9 GB, with progress shown by the cat) and starts with Windows from then on. Uninstall from Windows Settings > Apps, which also removes the models. New versions download in the background and apply the next time Mew starts, or right away with **Restart to update** in Settings.

**macOS** (13.4 or later, Apple Silicon): unzip `Mew-macOS-arm64.zip`, move `Mew.app` to Applications and open it. macOS blocks the first launch because Mew isn't notarized: open System Settings › Privacy & Security, scroll down and click **Open Anyway** next to the Mew message (or run `xattr -dr com.apple.quarantine /Applications/Mew.app` once). Mew then downloads the same models and adds itself as a login item. macOS asks for three permissions:

- **Accessibility**, to catch the talk key, type with Cmd+V and watch for your edits. Mew asks on first launch and works as soon as you allow it in System Settings › Privacy & Security › Accessibility.
- **Input Monitoring**, if macOS asks for it too.
- **Microphone**, the first time you talk.

Each release is signed separately, so after replacing `Mew.app` with a newer one, remove Mew from the Accessibility list and allow it again.

Mew tells you when a new version is out; Settings links to the download. To update, replace `Mew.app` with it.

To uninstall, quit Mew, drag it to the Trash, and delete `~/Library/Application Support/Mew` (the models).

## How it works

| Step | What runs |
|---|---|
| Hotkey | Windows: low-level keyboard hook. macOS: event tap. The talk key never reaches other apps (on macOS, unless it's part of a shortcut) |
| Microphone | `cpal` (WASAPI, CoreAudio), open only while you hold the key, resampled to 16 kHz |
| Speech to text | **Parakeet TDT 0.6B v3** (int8 ONNX) in ONNX Runtime on the CPU (~0.4 s per 12 s of speech) |
| Tidy-up | **Qwen3.5 2B** (Q4_K_M) in a bundled `llama-server` (Vulkan GPU on Windows, Metal on macOS, else CPU). Starts when you press the key, unloads after 10 idle minutes, dies with Mew |
| Typing | Clipboard + Ctrl+V (macOS: Cmd+V); your clipboard is restored and dictations stay out of clipboard history (Win+V, and clipboard managers that honor nspasteboard.org markers) |
| Overlay | Drawn with `tiny-skia`; a layered window on Windows, a non-activating panel on macOS. Never takes focus; hides during fullscreen apps on Windows |
| Mew window | `ui/` (Preact + Vite, built to one HTML file embedded in the executable) in a web view via `wry` (WebView2, WKWebView). Talks to Rust over `window.ipc` (`src/api.rs`). Closing it ends its web view, so it costs nothing while closed |
| History | SQLite (`mew.db`, `src/history.rs`): one row per dictation. Text expires per the "Keep transcripts" setting; numbers are kept for stats |
| Install/updates | Velopack on Windows (per-user, updates from GitHub Releases). On macOS, a check of the latest GitHub Release |

Safety net for the tidy-up: short phrases skip it, and if the model's output looks like an answer instead of a cleanup, Mew types the original transcript.

OS-specific code sits next to the module that uses it: `src/<module>/windows.rs` and `src/<module>/macos.rs`, behind the same functions.

Mew goes online only to download its models and, unless you turn it off in Settings, to ask GitHub for the newest version every 12 hours.

Files: on Windows the app is in `%LOCALAPPDATA%\Mew`, models/settings/dictionary/logs in `%LOCALAPPDATA%\Mew Data`. On macOS, `Mew.app` and `~/Library/Application Support/Mew`.

## Build

**Windows:** needs Rust (stable MSVC), Visual Studio C++ build tools, Node.js, and `dotnet tool install -g vpk`.

```powershell
npm --prefix ui ci; npm --prefix ui run build   # the window's page; cargo embeds ui/dist/index.html
cargo build --release
powershell -ExecutionPolicy Bypass -File scripts\package.ps1   # does both, -> dist\releases\Mew-win-Setup.exe
```

**macOS:** needs Rust, the Xcode command line tools and Node.js; `vpk` only for the installer.

```sh
npm --prefix ui ci && npm --prefix ui run build
cargo build --release
scripts/package-mac.sh   # does both, -> dist/Mew.app (and dist/releases if vpk is installed)
```

`package-mac.sh` signs ad-hoc. macOS ties the Accessibility and Input Monitoring permissions to the signature, so each ad-hoc rebuild needs them granted again (remove Mew from the list and add it back). Set `MEW_SIGN_IDENTITY` to a certificate in your keychain (a self-signed one works) to keep them.

Developer commands (`mew <flag>`):

- `--download`: fetch models with console progress
- `--selftest <16k-mono.wav>`: run speech-to-text and tidy-up on a file and print timings (types nothing)
- `--render <dir>`: write PNGs of every overlay state
- `--write-icon <file.ico>` / `--write-icns <file.icns>`: regenerate `assets/mew.ico` or `assets/mew.icns` from the cat drawing
- `--seed-demo`: fill a scratch history with sample dictations (requires `MEW_DATA_DIR`, never touches real data)

`MEW_DATA_DIR` points Mew at a different data folder (settings, history, models, logs) for development.

Running from `target/release` needs `MEW_LLAMA_DIR` pointing at a folder with `llama-server` (the package scripts stage it into `llama` next to the executable). A dev build run from a terminal gets its permissions through the terminal app on macOS.

## CI and releases

- **CI** (`.github/workflows/ci.yml`): every push to `main` and every PR builds the UI, then runs `cargo fmt --check`, `cargo clippy -D warnings` and the tests on Windows, and clippy and the tests on macOS.
- **Release** (`.github/workflows/release.yml`): push a tag matching Cargo.toml's version (`git tag v0.4.0; git push --tags`) to build the Windows installer and publish a GitHub Release with the Velopack files, then attach the ad-hoc signed `Mew-macOS-arm64.zip`. Run it manually from the Actions tab to just build both as downloadable artifacts.

## Before a public release

- Code-sign `Mew-win-Setup.exe` and `mew.exe` (unsigned apps that hook the keyboard trip SmartScreen and antivirus heuristics).
- On macOS, sign with a Developer ID and notarize; Gatekeeper blocks ad-hoc signed apps downloaded from the internet.

Third-party licenses: `assets/NOTICE.txt` (shipped next to `mew.exe` and in `Mew.app/Contents/Resources`, opened by Settings > Licenses).
