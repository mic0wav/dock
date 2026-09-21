use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gtk::gio::{self, prelude::*};
use gtk::glib;

#[derive(serde::Deserialize, Clone, Debug)]
pub struct Pin {
    pub icon: String,
    pub command: String,
}

#[derive(serde::Deserialize, Clone, Copy, Default, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Position {
    Top,
    #[default]
    Bottom,
}

#[derive(serde::Deserialize, Clone, Default, Debug)]
pub struct Config {
    #[serde(default)]
    pub pinned: BTreeMap<String, Pin>,
    #[serde(default = "default_true")]
    pub hot_reload: bool, // this ofcourse is the only option that requires a full restart
    #[serde(default)]
    pub position: Position,
}

fn default_true() -> bool {
    true
}

/// gets the config from XDG_CONFIG_HOME or ~/.config/
pub fn dir() -> Option<PathBuf> {
    Some(gtk::glib::user_config_dir())
}

fn read_or_seed_default(filename: &str, default: &str) -> String {
    let Some(dir) = dir() else {
        log::warn!("could not resolve config directory, using default for {filename}");
        return default.to_string();
    };

    let path = dir.join(filename);

    match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(exist) if exist.kind() == std::io::ErrorKind::NotFound => {
            if let Err(e) =
                std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, default))
            {
                log::warn!(
                    "failed to write default {filename} to {}: {e}",
                    path.display()
                )
            } else {
                log::info!("wrote default {filename} to {}", path.display())
            }
            default.to_string()
        }
        Err(exist) => {
            log::warn!("{}", exist);
            default.to_string()
        }
    }
}

pub fn load() -> Config {
    const DEFAULT_CONFIG: &str = include_str!("../resources/config.toml");
    let contents = read_or_seed_default("config.toml", DEFAULT_CONFIG);

    match toml::from_str(&contents) {
        Ok(config) => config,
        Err(e) => {
            log::warn!("failed to parse config.toml: {e:#}, falling back on default");
            Config::default()
        }
    }
}

pub fn load_css() -> String {
    const DEFAULT_CSS: &str = include_str!("../resources/dock.css");
    read_or_seed_default("dock.css", DEFAULT_CSS)
}

/// watches the config dir
pub fn watch_changes(on_change: impl Fn() + 'static) -> Option<gio::FileMonitor> {
    let Some(dir) = dir() else {
        log::warn!("could not resolve config directory, disabeling hot reload");
        return None;
    };

    let monitor = match gio::File::for_path(&dir)
        .monitor_directory(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
    {
        Ok(m) => m,
        Err(e) => {
            log::warn!("could not watch {}: {e:#}", dir.display());
            return None;
        }
    };

    let on_change = Rc::new(on_change);
    let pending = Rc::new(Cell::new(false));

    monitor.connect_changed(move |_, file, _, event| {
        if !matches!(
            event,
            gio::FileMonitorEvent::Created | gio::FileMonitorEvent::ChangesDoneHint
        ) {
            return;
        }

        let name = file.basename();
        let relevant = matches!(
            name.as_deref().and_then(Path::to_str),
            Some("config.toml" | "dock.css")
        );
        if !relevant || pending.replace(true) {
            return;
        }

        // debounce: coalesce bursts of events into one reload
        let pending = pending.clone();
        let on_change = on_change.clone();
        glib::timeout_add_local_once(Duration::from_millis(50), move || {
            pending.set(false);
            on_change();
        });
    });

    Some(monitor)
}
