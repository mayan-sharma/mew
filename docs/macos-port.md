# Mew on macOS

Status of the macOS port (Apple Silicon), for whoever picks it up next. Windows keeps working: its CI job is unchanged, and a macOS job runs beside it.

## Working agreements

- Reply in concise pointers.
- **No AI attribution** in commits or PRs: no `Co-Authored-By: Claude`, no "Generated with Claude Code".
- Ask before pushing tags, publishing releases, or making the repo public.
- Match the existing code: small modules, few comments, `anyhow`, `log`. Run `cargo fmt` and `cargo clippy -- -D warnings`.
- Keep dependencies light; prefer `objc2` crates over big frameworks.
- Don't move the mouse or steal focus while the user is working. Ask before UI-driving tests. Never use the real data dir for tests.

## Decisions

- Talk key: **Fn** by default; Right Option and Right Command are the alternatives. Caps Lock and F8 aren't offered: macOS reports Caps Lock as a toggle and F-keys as media keys.
- **Apple Silicon only** (arm64, llama.cpp on Metal).
- **Ad-hoc signing** for now; `MEW_SIGN_IDENTITY` picks a certificate. No notarization and no CI release job yet.
- Bundle id `io.github.mayan-sharma.mew`. Minimum macOS 13.4, the prebuilt ONNX Runtime's own minimum (`.cargo/config.toml` sets it for every build).

## How the macOS side works

OS-specific code lives in `src/<module>/windows.rs` and `src/<module>/macos.rs`, picked with `#[cfg_attr(..., path = ...)]` and exposing the same functions; `app.rs` doesn't know which.

| Piece | macOS |
|---|---|
| Main loop (`system`) | NSApplication (Accessory policy) pumped by hand, like the Windows `GetMessageW` loop; workers wake it through the main dispatch queue |
| Hotkey | Event tap on keyDown and flagsChanged. The talk key is swallowed unless another key goes down while it's held: then it's a shortcut, Mew cancels quietly and the release reaches apps |
| Paste | NSPasteboard snapshot and restore, Cmd+V posted with `CGEventPost`; both writes marked Transient and Concealed |
| Overlay | Non-activating NSPanel at status level on every Space; tiny-skia frame → CGImage → CALayer, from a main-thread run loop timer |
| Mew window | NSWindow + wry, plus an Edit menu so Cmd+C/V/X/A/Z work in it. Closing it hides the app so the keyboard goes back to the app you were in |
| Learn from edits | `AXFocusedUIElement` → `AXValue`; skips `AXSecureTextField` |
| llama-server | Metal build in `Mew.app/Contents/MacOS/llama`, under an `sh` guard in its own process group that ends it when Mew quits or crashes |
| Start at login | `SMAppService.mainApp` |
| One instance | `flock` on `mew.lock`; opening Mew.app again sends a reopen event, which opens the Mew window |
| Build | `build.rs` links clang's runtime: ONNX Runtime's library calls `__isPlatformVersionAtLeast` |

## Verified (M3 Pro, macOS 26.6)

- `cargo fmt --check`, `cargo clippy --release --all-targets -- -D warnings`, `cargo test --release` (7 pass).
- The Windows build type-checks from the Mac with the same clippy flags, tests included (recipe below).
- `--render`: the same drawings; bubble text in SF Pro (Segoe UI on Windows).
- `--selftest` on the three clips in `tests/audio`: speech to text 0.15–0.93 s, tidy-up 0.12–0.54 s on Metal; fillers, "no wait" corrections and lists come out right; no llama-server left behind.
- `scripts/package-mac.sh`: a 52 MB Mew.app that passes `codesign --verify --deep --strict`; the bundled llama-server runs.
- Real use of that Mew.app: the Accessibility prompt appeared, and once it was granted the talk key worked without a restart. Hold Fn, talk, release typed the text (release to paste 0.44–0.81 s). After 10 idle minutes the tidy-up model unloaded and llama-server exited cleanly.
- The Mew window: launching Mew.app again opened it (one Mew process, a new WebKit content process), and the user found the pages right. Closing it ended that process; Mew idles at about 81 MB.

## Not verified yet (needs someone at the keyboard)

- Double-tap for hands-free; Esc; the clipboard restored after a paste; a fix within 30 seconds making the cat say "Noted!".
- The tray menu's items; dictation in Notes, Safari and a terminal.
- Whether swallowing Fn stops the emoji picker even without the "Do Nothing" setting.
- Open at login from an installed Mew.app; `vpk pack` (needs dotnet; its flags may need adjusting).

## Known gaps

- The cat stays visible over fullscreen apps (Windows hides it).
- Cmd+V is sent as the key in the V position, so Dvorak layouts paste with Cmd+K.
- Secure Keyboard Entry (a Terminal option, password managers) hides keystrokes from event taps, so the talk key may not work while it's on.
- A denied microphone records silence: "I didn't catch that", not an error.
- Behind TLS-intercepting proxies (such as Zscaler) model downloads fail on both OSes: ureq trusts its bundled roots, not the Keychain or the Windows store. ureq's platform verifier would fix it.
- Speech to text uses 8 threads; on an M3 Pro, 4 (performance cores only) measured 10–20% faster. Not tuned.

## Checking the Windows build from a Mac

CI is the real check. For a quick local type check, stub the C toolchain that `libsqlite3-sys` and `ring` call from their build scripts; `check` and `clippy` never link, so empty objects are enough:

```sh
rustup target add x86_64-pc-windows-msvc
mkdir -p /tmp/fakecc
printf '#!/bin/sh\nprev=; for a; do [ "$prev" = -o ] && : > "$a"; case $a in -Fo*) : > "${a#-Fo}";; -E) exit 0;; esac; prev=$a; done\n' > /tmp/fakecc/cc
printf '#!/bin/sh\nfor a; do case $a in -out:*|/OUT:*) : > "${a#*:}";; *.lib|*.a) : > "$a";; esac; done\n' > /tmp/fakecc/ar
chmod +x /tmp/fakecc/*
CC_x86_64_pc_windows_msvc=/tmp/fakecc/cc AR_x86_64_pc_windows_msvc=/tmp/fakecc/ar CARGO_TARGET_DIR=target/win \
    cargo clippy --release --all-targets --target x86_64-pc-windows-msvc -- -D warnings
```
