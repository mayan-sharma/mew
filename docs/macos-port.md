# Porting Mew to macOS

Handoff for a Claude session on a Mac. Goal: the same app on macOS (Apple Silicon first). Same models, same cat, same Mew window, same history database. **Windows must keep working.** CI runs on Windows and must stay green.

Read `README.md` first; it describes the app and how each piece works.

## Working agreements

- Reply in concise pointers.
- **No AI attribution** in commits or PRs: no `Co-Authored-By: Claude`, no "Generated with Claude Code".
- Ask before pushing tags, publishing releases, or making the repo public.
- Match the existing code: small modules, few comments, `anyhow`, `log`. Run `cargo fmt` and `cargo clippy -- -D warnings`.
- Keep dependencies light; prefer `objc2` crates over big frameworks.

## Shape of the change

- Split each OS-specific module into `#[cfg(target_os = "windows")]` and `#[cfg(target_os = "macos")]` implementations behind the same functions. Callers in `app.rs` shouldn't care.
- Move the Windows-only crates (`windows`, `windows-sys`, `embed-resource`) to `[target.'cfg(windows)'.dependencies]`. Do the same for the build dependency, and gate `build.rs`'s resource embedding.
- Mac crates: `objc2`, `objc2-app-kit`, `objc2-foundation`, `objc2-core-graphics` or `core-graphics`, `objc2-application-services` or `accessibility-sys`.
- First milestone: `cargo build` on the Mac with the Mac stubs returning errors. Then port one piece at a time.

## Already portable (just check it works)

| File | Notes |
|---|---|
| `asr.rs`, `models.rs` | ONNX Runtime via `ort` has macOS arm64 binaries; stay on the CPU, which is plenty fast |
| `audio.rs` | `cpal` uses CoreAudio. Needs the mic permission (see Packaging) |
| `cat.rs`, `learn.rs`, `history.rs`, `api.rs` | Pure Rust |
| `ui/` | Same page; wry renders it with WKWebView |
| `tray.rs` | `tray-icon` supports the macOS menu bar, but it must be created on the main thread inside a running NSApplication loop |

## Windows-only pieces and their macOS replacements

| File | Windows today | macOS |
|---|---|---|
| `app.rs` main loop | `GetMessageW` loop on the main thread; workers wake it with `PostThreadMessageW(WM_NULL)` | NSApplication run loop with the `Accessory` activation policy (no Dock icon). Wake it by dispatching to the main queue, or use `tao`'s event loop for the tray and window together |
| `hotkey.rs` + `config::Hotkey::vk` | Low-level keyboard hook that swallows the talk key; double-tap; Esc | `CGEventTap` on `flagsChanged` and `keyDown` (return null to swallow). Needs the **Accessibility** and **Input Monitoring** permissions. Give each key a macOS keycode next to `vk()` |
| `paste.rs` | Snapshot the clipboard, set the text, Ctrl+V, restore; private formats keep it out of clipboard history | NSPasteboard snapshot and restore, then send Cmd+V with `CGEventPost`. Mark the item with `org.nspasteboard.TransientType` and `ConcealedType` so clipboard managers skip it |
| `overlay.rs` | Layered window; tiny-skia premultiplied RGBA → BGRA, `UpdateLayeredWindow`; hidden for fullscreen apps; per-monitor DPI | Borderless, transparent, non-activating `NSPanel` at the status-window level, with `ignoresMouseEvents` and on all Spaces. Draw the tiny-skia pixmap into a `CALayer`'s contents (a CGImage). Scale by `backingScaleFactor`. Hiding for fullscreen is optional for v1. The Windows-only Alt-Tab hide/show workaround is not needed |
| `ui.rs` | Win32 "MewWindow" + wry WebView2; kills `msedgewebview2` children on close | `NSWindow` (or a `tao` window) + wry. Drop the process-kill code: WKWebView doesn't leak the same way, but check Activity Monitor after closing |
| `edits.rs` (learn from edits) | UI Automation: focused element, Value or Text pattern, skips passwords | Accessibility API: system-wide `AXFocusedUIElement`, read `AXValue`, skip role `AXSecureTextField`. Same 30-second poll |
| `llm.rs` | `llama-server.exe` in a job object (dies with Mew), key from `BCryptGenRandom` | llama.cpp's macOS arm64 release (Metal). No job objects: start it in its own process group and kill it on exit, and have it exit when its parent dies (watch the parent pid or a stdin pipe). Generate the key with `getrandom` |
| `system.rs` | HKCU Run autostart + StartupApproved, app name from the exe's version info, `ShellExecuteW` | Autostart: `SMAppService.mainApp` (macOS 13+). App name: `NSWorkspace.frontmostApplication.localizedName`. Open: `/usr/bin/open` |
| `main.rs` | Named mutex; a second launch messages the first to open the window; DPI awareness | `flock` on a lock file in the data dir. For "launch again opens the window", handle the app's reopen event (`applicationShouldHandleReopen`). No DPI call |
| `watchdog.rs` | Message-only window pinged every second; logs hangs | Keep the heartbeat idea and drop the window. The reopen event replaces `WM_OPEN` |
| `logger.rs` | `GetLocalTime` | `libc::localtime_r` or a tiny formatter |
| `config.rs` | `%LOCALAPPDATA%\Mew Data`; the llama dir sits next to the exe | `~/Library/Application Support/Mew`; llama in `Mew.app/Contents/Resources/llama` or `MacOS/llama`. Keep the `MEW_DATA_DIR` and `MEW_LLAMA_DIR` overrides |
| `update.rs` | Velopack | Velopack supports macOS too. Keep the same `FEED` |

## Decisions for the user (ask, don't assume)

1. **Default talk key.** Right Option types special characters on many layouts. Fn/Globe is what Mac users expect, but macOS must be set to "Press 🌐 to: Do Nothing". Options: A) Fn/Globe, B) Right Option, C) Right Command.
2. **Intel Macs:** support them, or Apple Silicon only?
3. **Signing:** an Apple Developer ID ($99/yr) is needed for notarization and a smooth install. Local builds can use an ad-hoc signature or a self-signed one.

## Packaging (after it runs from `cargo run`)

- Build a `Mew.app` bundle. `Info.plist` needs `LSUIElement=true` (no Dock icon) and `NSMicrophoneUsageDescription`. Accessibility and Input Monitoring prompts come from the API calls themselves.
- **Gotcha:** macOS ties permissions to the code signature. Each ad-hoc-signed rebuild can lose the Accessibility and Input Monitoring grants. Sign dev builds with one stable identity (a self-signed certificate in Keychain works) or expect to re-grant.
- Add `scripts/package-mac.sh`, the macOS counterpart of `scripts/package.ps1`:
  - Build `ui/` and run `cargo build --release`.
  - Fetch the pinned llama.cpp macOS arm64 zip from the same `b11714` release and check its sha256.
  - Assemble `Mew.app`, then `vpk pack` (Velopack's macOS output).
- CI: add a `macos-latest` job to `.github/workflows/ci.yml` (fmt, clippy, tests). Add the release job only once signing is decided.

## How to verify

- `cargo test` for the platform-neutral logic.
- `mew --render <dir>`: the overlay PNGs should look identical to Windows.
- `MEW_LLAMA_DIR=<llama dir> mew --selftest <16k-mono.wav>`: transcription and tidy-up timings (types nothing).
- `MEW_DATA_DIR=/tmp/mew-demo mew --seed-demo`, then open the Mew window to check the UI. Never use the real data dir for tests.
- Real use, in TextEdit, Notes, Safari and a terminal:
  - Hold the key, talk, release; the text appears and the clipboard is restored.
  - Double-tap for hands-free; Esc cancels.
  - Fix a misheard word within 30 seconds; the cat says "Noted!".
- Don't move the mouse or steal focus while the user is working. Ask before UI-driving tests.
