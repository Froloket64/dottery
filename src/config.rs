use std::{fs::canonicalize, io, path::Path};

use dirs::home_dir;
pub use serde::{Deserialize, Serialize};
use tap::prelude::*;

use crate::logging::log_error;

/// Default config directory.
///
/// Typically appended to `~/.config/` to get the full path.
pub const CONFIG_DIR: &str = "dottery";
/// Default config file location. See [`CONFIG_DIR`] for full path.
pub const CONFIG_FILE: &str = "config.toml";

/// Dottery config.
///
/// See [`CONFIG_FILE`] for default location.
#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    pub paths: Paths,
    pub files: Files,
}

impl Default for Config {
    fn default() -> Self {
        let mut home = home_dir().unwrap();
        home.push(".dotfiles");

        let dotfiles_path = home.to_str().unwrap();

        Self {
            paths: Paths {
                dotfiles_path: dotfiles_path.into(),
            },
            files: Files {
                include: vec![".personal.toml".into()],
            }
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Paths {
    pub dotfiles_path: String,
}


#[derive(Debug, Deserialize, Serialize)]
pub struct Files {
    /// Files that need to be included when reading manifest.
    pub include: Vec<String>,
}

/// Core dotfiles manifest.
///
/// NOTE: The rest of the variables in `..toml` and `.personal.toml` are stored
/// and processed separately.
#[derive(Clone, Debug, Deserialize)]
pub struct Dotfiles {
    /// Packages that are linked to the dotfiles (usually `template/`).
    pub packages: Vec<Package>,
    /// Other packages that are expected to be installed.
    pub dependencies: Option<Dependencies>,
}

// TODO: Post-installation (scripts?)
/// A recipe that contains all information for a package to be installed.
#[derive(Clone, Debug, Deserialize)]
pub struct Package {
    name: String,
    from_aur: Option<bool>,
}

/// Contains required and optional dependencies.
#[derive(Clone, Debug, Deserialize)]
pub struct Dependencies {
    pub required: Option<Vec<Package>>,
    pub optional: Option<Vec<Package>>,
}

impl Package {
    /// Returns package name.
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Returns whether the package should be installed from the AUR.
    pub fn from_aur(&self) -> bool {
        self.from_aur.unwrap_or(false)
    }
}

/// Parses the config file into `io::Result<[Config]>`.
pub fn read_config(config_file: &Path) -> io::Result<Config> {
    let mut config_res = match std::fs::read_to_string(config_file.clone()) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            std::fs::create_dir_all(config_file.clone().parent().unwrap())?;

            let config_default = Config::default();

            toml::to_string(&config_default)
                .expect("failed to convert config to `toml`")
                .pipe(|s| std::fs::write(config_file, s))
                .expect("failed to write config file");

            Ok(config_default)
        }
        Err(err) => {
            log_error(&format!("failed to read config file: {err}"));

            Ok(Config::default())
        }
        Ok(contents) => Ok(toml::from_str(&contents).expect("failed to parse config file")),
    };

    // Canonicalize dotfiles path
    if let Ok(ref mut config) = config_res {
        let absolute_path = canonicalize(&config.paths.dotfiles_path)
            .expect(&format!(
                "dotfiles path not found: `{}`",
                &config.paths.dotfiles_path
            ))
            .to_str()
            .unwrap()
            .to_string();

        config.paths.dotfiles_path = absolute_path;
    }

    config_res
}
