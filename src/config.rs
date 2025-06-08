use dirs::{config_dir, home_dir};
pub use serde::{Deserialize, Serialize};
use std::{fs::canonicalize, io};
use tap::prelude::*;

use crate::logging::log_error;

pub const CONFIG_DIR: &str = "dottery";
pub const CONFIG_FILE: &str = "config.toml";

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    pub paths: Paths,
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
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Paths {
    pub dotfiles_path: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Dotfiles {
    pub packages: Vec<Package>,
    pub dependencies: Option<Dependencies>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Package {
    name: String,
    from_aur: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Dependencies {
    pub required: Option<Vec<Package>>,
    pub optional: Option<Vec<Package>>,
}

impl Package {
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn from_aur(&self) -> bool {
        self.from_aur.unwrap_or(false)
    }
}

pub fn read_config() -> io::Result<Config> {
    let config_file = config_dir()
        .unwrap()
        .tap_mut(|path| path.push(CONFIG_DIR))
        .tap_mut(|path| path.push(CONFIG_FILE));

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
