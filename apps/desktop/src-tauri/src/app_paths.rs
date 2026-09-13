use std::{path::PathBuf, sync::OnceLock};

#[derive(Clone, Debug)]
pub struct AppPaths {
    pub app_data: PathBuf,
    pub documents: PathBuf,
}

impl AppPaths {
    pub fn config_path(&self) -> PathBuf {
        self.app_data.join("install.json")
    }
    pub fn models_root(&self) -> PathBuf {
        self.app_data.join("models")
    }
    pub fn model_settings_path(&self) -> PathBuf {
        self.app_data.join("model-settings.json")
    }
    #[cfg(test)]
    pub fn default_board_path(&self) -> PathBuf {
        self.documents.join("Default Board")
    }
    pub fn documents_dir(&self) -> PathBuf {
        self.documents.clone()
    }
}

static PATHS: OnceLock<AppPaths> = OnceLock::new();

#[cfg(mobile)]
pub fn initialize(app_data: PathBuf, documents: PathBuf) -> Result<(), String> {
    PATHS
        .set(AppPaths {
            app_data,
            documents,
        })
        .map_err(|_| "application paths already initialized".into())
}

pub fn app_data_dir() -> PathBuf {
    #[cfg(mobile)]
    {
        PATHS
            .get()
            .expect("application paths not initialized")
            .app_data
            .clone()
    }
    #[cfg(not(mobile))]
    {
        PATHS.get().map(|p| p.app_data.clone()).unwrap_or_else(|| {
            std::env::var_os("LOCALAPPDATA")
                .or_else(|| std::env::var_os("APPDATA"))
                .map(PathBuf::from)
                .map(|p| p.join("Knot"))
                .unwrap_or_else(|| {
                    std::env::var_os("HOME")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from("."))
                        .join(".config/knot")
                })
        })
    }
}

pub fn documents_dir() -> Result<PathBuf, String> {
    if let Some(paths) = PATHS.get() {
        return Ok(paths.documents_dir());
    }
    #[cfg(mobile)]
    {
        return Err("application paths not initialized".into());
    }
    #[cfg(not(mobile))]
    {
        Ok(std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Documents"))
    }
}

pub fn config_path() -> PathBuf {
    #[cfg(not(mobile))]
    if let Ok(path) = std::env::var("KNOT_CONFIG_PATH") {
        return PathBuf::from(path);
    }
    PATHS
        .get()
        .map(|p| p.config_path())
        .unwrap_or_else(|| app_data_dir().join("install.json"))
}
pub fn models_root() -> PathBuf {
    PATHS
        .get()
        .map(|p| p.models_root())
        .unwrap_or_else(|| app_data_dir().join("models"))
}
pub fn model_settings_path() -> PathBuf {
    PATHS
        .get()
        .map(|p| p.model_settings_path())
        .unwrap_or_else(|| app_data_dir().join("model-settings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pure_paths_persist_real_values() {
        let root = std::env::temp_dir().join(format!("knot-paths-{}", std::process::id()));
        let paths = AppPaths {
            app_data: root.join("data"),
            documents: root.join("docs"),
        };
        std::fs::create_dir_all(paths.models_root()).unwrap();
        let settings = paths.model_settings_path();
        std::fs::write(&settings, b"{\"model_id\":\"local\"}").unwrap();
        assert_eq!(
            std::fs::read(&settings).unwrap(),
            b"{\"model_id\":\"local\"}"
        );
        let board = paths.default_board_path();
        std::fs::create_dir_all(&board).unwrap();
        let identity = board.join("identity.json");
        std::fs::write(&identity, b"board-identity").unwrap();
        assert_eq!(std::fs::read_to_string(identity).unwrap(), "board-identity");
        let _ = std::fs::remove_dir_all(root);
    }
}
