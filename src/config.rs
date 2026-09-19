use std::collections::BTreeMap;
use std::path::PathBuf;

use notify::{RecursiveMode, Watcher};

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
    if let Ok(x) = std::env::var("XDG_CONFIG_HOME") {
        Some(PathBuf::from(x).join("dock"))
    } else if let Ok(x) = std::env::var("HOME") {
        Some(PathBuf::from(x).join(".config/dock"))
    } else {
        None
    }
}

fn read_or_seed_default(filename: &str, default: &str) -> String {
    let Some(dir) = dir() else {
        log::warn!("could not resolve config directory, using default for {filename}");
        return default.to_string();
    };

    let path = dir.join(filename);

    match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(_) => {
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
pub fn watch_changes(on_change: impl Fn() + Send + 'static) {
    let Some(dir) = dir() else {
        log::warn!("could not resolve config directory, disabeling hot reload");
        return;
    };

    std::thread::spawn(move || {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = match notify::recommended_watcher(tx) {
            Ok(w) => w,
            Err(e) => {
                log::warn!("could not start config watcher: {e:#}");
                return;
            }
        };
        if let Err(e) = watcher.watch(&dir, RecursiveMode::NonRecursive) {
            log::warn!("could not watch {}: {e:#}", dir.display());
            return;
        }

        while let Ok(res) = rx.recv() {
            let Ok(event) = res else {
                continue;
            };
            let relevant = event.paths.iter().any(|p| {
                // for now only config.toml and dock.css get watched
                matches!(
                    p.file_name().and_then(|n| n.to_str()),
                    Some("config.toml" | "dock.css")
                )
            });

            if relevant {
                // this makes sure the dock does not get reloaded too much
                std::thread::sleep(std::time::Duration::from_millis(50));
                while rx.try_recv().is_ok() {}
                on_change();
            }
        }
    });
}
