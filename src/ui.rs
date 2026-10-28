//! The Mew window: history, stats, dictionary and settings, as a page in a web view (WebView2 on Windows, WKWebView
//! on macOS). The page is built from `ui/` and embedded in the executable; it talks to Rust over `window.ipc`.
//! Closing the window destroys the web view, so it costs nothing while closed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

use serde_json::{Value, json};
use wry::http::Request;
use wry::{WebContext, WebViewBuilder};

use crate::config;

#[cfg_attr(windows, path = "ui/windows.rs")]
#[cfg_attr(target_os = "macos", path = "ui/macos.rs")]
mod platform;

const PAGE: &str = include_str!("../ui/dist/index.html");

/// Set when the user closes the window; the main loop tears it down.
static CLOSE_REQUESTED: AtomicBool = AtomicBool::new(false);

pub struct Ui {
    window: Option<platform::Window>,
    context: WebContext,
    requests: Receiver<String>,
    request_tx: Sender<String>,
}

impl Ui {
    pub fn new() -> Self {
        let (request_tx, requests) = channel();
        let context = WebContext::new(Some(config::data_dir().join("webview")));
        Self { window: None, context, requests, request_tx }
    }

    pub fn is_open(&self) -> bool {
        self.window.is_some()
    }

    /// Opens the window, or brings it to the front if it's already open. `page` picks the starting tab.
    pub fn open(&mut self, page: &str) {
        if let Some(w) = &self.window {
            w.focus();
            self.push(&json!({ "type": "navigate", "page": page }));
            return;
        }
        match self.create(page) {
            Ok(window) => self.window = Some(window),
            Err(e) => {
                log::error!("opening the Mew window failed: {e:#}");
                #[cfg(windows)]
                crate::system::open("https://go.microsoft.com/fwlink/p/?LinkId=2124703"); // WebView2 runtime
            }
        }
    }

    fn create(&mut self, page: &str) -> anyhow::Result<platform::Window> {
        let dark = platform::dark_mode();
        let requests = self.request_tx.clone();
        let (r, g, b) = background(dark);
        // The page is loaded as a string, not via a custom protocol: wry's protocol handler holds the WebView2
        // environment, which kept Edge's browser processes (~360 MB) alive after the window closed.
        let builder = WebViewBuilder::new_with_web_context(&mut self.context)
            .with_initialization_script(format!("window.__MEW_PAGE = {};", serde_json::to_string(page)?))
            .with_ipc_handler(move |request: Request<String>| {
                let _ = requests.send(request.into_body());
            })
            .with_html(PAGE)
            .with_background_color((r, g, b, 255))
            .with_devtools(cfg!(debug_assertions))
            .with_navigation_handler(|url| url.starts_with("about:blank") || url.starts_with("data:"));
        let window = platform::Window::create(builder, dark)?;
        CLOSE_REQUESTED.store(false, Ordering::Relaxed);
        Ok(window)
    }

    /// Handles window housekeeping and answers queued page requests with `api`. Call after each message.
    pub fn pump(&mut self, api: &mut dyn FnMut(&str, &Value) -> anyhow::Result<Value>) {
        if CLOSE_REQUESTED.swap(false, Ordering::Relaxed)
            && let Some(window) = self.window.take()
        {
            window.close();
        }
        if let Some(w) = &self.window {
            w.follow_theme();
        }
        while let Ok(raw) = self.requests.try_recv() {
            let Ok(request) = serde_json::from_str::<Value>(&raw) else { continue };
            let id = request["id"].as_i64().unwrap_or(0);
            let method = request["method"].as_str().unwrap_or_default();
            let reply = match api(method, &request["params"]) {
                Ok(data) => json!({ "id": id, "ok": true, "data": data }),
                Err(e) => {
                    log::warn!("ui request {method} failed: {e:#}");
                    json!({ "id": id, "ok": false, "error": e.to_string() })
                }
            };
            self.send(&reply);
        }
    }

    /// Sends an event to the page (new dictation, settings changed elsewhere, navigate).
    pub fn push(&self, event: &Value) {
        self.send(&json!({ "event": event }));
    }

    fn send(&self, message: &Value) {
        if let Some(w) = &self.window {
            let _ = w.webview.evaluate_script(&format!("window.__mew && window.__mew.receive({message})"));
        }
    }
}

/// The page's background, shown until it paints.
fn background(dark: bool) -> (u8, u8, u8) {
    if dark { (0x1c, 0x19, 0x17) } else { (0xfb, 0xf8, 0xf4) }
}
