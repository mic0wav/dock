use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gio_unix::DesktopAppInfo;
use gtk::gio;
use gtk::prelude::*;

const FAILURE_TTL: Duration = Duration::from_secs(30);

#[derive(Default)]
struct CacheState {
    resolved: HashMap<String, String>,
    failed: HashMap<String, Instant>,
}

pub struct IconCache {
    state: Mutex<CacheState>,
}

impl IconCache {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(CacheState::default()),
        }
    }

    pub fn lookup(&self, app_id: &str) -> Option<String> {
        {
            let state = self.state.lock().unwrap(); // this should probably be a Err(_)
            if let Some(icon) = state.resolved.get(app_id) {
                return Some(icon.clone());
            }
            if let Some(&failed_at) = state.failed.get(app_id)
                && failed_at.elapsed() < FAILURE_TTL
            {
                return None;
            }
        }

        let resolved = resolve(app_id);

        let mut state = self.state.lock().unwrap();
        match &resolved {
            Some(icon) => {
                state.resolved.insert(app_id.to_string(), icon.clone());
                state.failed.remove(app_id);
            }
            None => {
                state.failed.insert(app_id.to_string(), Instant::now());
            }
        }
        resolved
    }
}

/// if the app_id matches a icon directly, use that. otherwise use the .desktop files
fn resolve(app_id: &str) -> Option<String> {
    if let Some(display) = gtk::gdk::Display::default() {
        let theme = gtk::IconTheme::for_display(&display);
        if theme.has_icon(app_id) {
            return Some(app_id.to_string());
        }
    }

    let info = DesktopAppInfo::new(&format!("{app_id}.desktop"))?;
    let icon = info.icon()?;
    let themed: gio::ThemedIcon = icon.downcast().ok()?;
    themed.names().first().map(|n| n.to_string())
}
