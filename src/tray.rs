//! Tray icon and menu. Lives on the main thread; turns clicks into commands for the app.

use anyhow::Result;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::app::Command;
use crate::config::{Hotkey, Settings};
use crate::{cat, system};

pub struct Tray {
    _icon: TrayIcon,
    title: MenuItem,
    hotkeys: Vec<(Hotkey, CheckMenuItem)>,
    mics: Submenu,
    mic_items: Vec<(Option<String>, CheckMenuItem)>,
    cleanup: CheckMenuItem,
    autostart: CheckMenuItem,
}

impl Tray {
    pub fn new(settings: &Settings) -> Result<Self> {
        let title = MenuItem::with_id("title", title_text(settings.hotkey), false, None);
        let hotkey_menu = Submenu::new("Talk key", true);
        let hotkeys: Vec<_> = Hotkey::ALL
            .iter()
            .map(|&h| (h, CheckMenuItem::with_id(format!("hotkey:{h:?}"), h.label(), true, h == settings.hotkey, None)))
            .collect();
        for (_, item) in &hotkeys {
            hotkey_menu.append(item)?;
        }
        let mics = Submenu::new("Microphone", true);
        let cleanup = CheckMenuItem::with_id("cleanup", "Tidy up text with AI", true, settings.cleanup, None);
        let autostart =
            CheckMenuItem::with_id("autostart", "Start with Windows", true, system::autostart_enabled(), None);
        let menu = Menu::new();
        menu.append_items(&[
            &MenuItem::with_id("open", "Open Mew", true, None),
            &MenuItem::with_id("history", "History", true, None),
            &PredefinedMenuItem::separator(),
            &title,
            &hotkey_menu,
            &mics,
            &cleanup,
            &autostart,
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id("quit", "Quit Mew", true, None),
        ])?;
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false) // left click opens the window
            .with_menu_on_right_click(true)
            .with_tooltip(title_text(settings.hotkey))
            .with_icon(Icon::from_rgba(cat::icon_rgba(32), 32, 32)?)
            .build()?;
        let mut tray = Self { _icon: icon, title, hotkeys, mics, mic_items: Vec::new(), cleanup, autostart };
        tray.refresh_mics(&[], settings.microphone.as_deref());
        Ok(tray)
    }

    /// Rebuilds the microphone list from an already-fetched device list (devices come and go).
    pub fn refresh_mics(&mut self, names: &[String], selected: Option<&str>) {
        let current: Vec<&str> = self.mic_items.iter().filter_map(|(n, _)| n.as_deref()).collect();
        if !self.mic_items.is_empty() && current == names.iter().map(String::as_str).collect::<Vec<_>>() {
            return; // unchanged
        }
        for (_, item) in self.mic_items.drain(..) {
            let _ = self.mics.remove(&item);
        }
        let selected = selected.filter(|s| names.iter().any(|n| n == s));
        let entries = std::iter::once((None, "Windows default".to_string()))
            .chain(names.iter().map(|n| (Some(n.clone()), n.clone())));
        for (i, (name, label)) in entries.enumerate() {
            let item = CheckMenuItem::with_id(format!("mic:{i}"), &label, true, name.as_deref() == selected, None);
            let _ = self.mics.append(&item);
            self.mic_items.push((name, item));
        }
    }

    /// Updates the menu for a click and returns what the app should do.
    pub fn handle(&mut self, event: &MenuEvent) -> Option<Command> {
        let id = event.id.0.as_str();
        if let Some(name) = id.strip_prefix("hotkey:") {
            let hotkey = *Hotkey::ALL.iter().find(|h| format!("{h:?}") == name)?;
            for (h, item) in &self.hotkeys {
                item.set_checked(*h == hotkey);
            }
            self.title.set_text(title_text(hotkey));
            let _ = self._icon.set_tooltip(Some(title_text(hotkey)));
            return Some(Command::SetHotkey(hotkey));
        }
        if id.starts_with("mic:") {
            let index = self.mic_items.iter().position(|(_, item)| item.id() == &event.id)?;
            for (i, (_, item)) in self.mic_items.iter().enumerate() {
                item.set_checked(i == index);
            }
            return Some(Command::SetMicrophone(self.mic_items[index].0.clone()));
        }
        match id {
            "open" => Some(Command::Open("overview")),
            "history" => Some(Command::Open("history")),
            "cleanup" => Some(Command::SetCleanup(self.cleanup.is_checked())),
            "autostart" => Some(Command::SetAutostart(self.autostart.is_checked())),
            "quit" => Some(Command::Quit),
            _ => None,
        }
    }

    /// Mirrors settings changed elsewhere (the Mew window) into the menu.
    pub fn sync(&mut self, settings: &Settings) {
        for (h, item) in &self.hotkeys {
            item.set_checked(*h == settings.hotkey);
        }
        self.title.set_text(title_text(settings.hotkey));
        let _ = self._icon.set_tooltip(Some(title_text(settings.hotkey)));
        for (name, item) in &self.mic_items {
            item.set_checked(name.as_deref() == settings.microphone.as_deref());
        }
        self.cleanup.set_checked(settings.cleanup);
        self.autostart.set_checked(system::autostart_enabled());
    }
}

fn title_text(hotkey: Hotkey) -> String {
    format!("Mew · hold {} to talk", hotkey.label())
}
