use std::{
    fmt::Display,
    fs::canonicalize,
    io::{self, BufReader, Read, Seek},
    path::Component,
    process::{self, ExitStatus, Stdio},
};

use bindet;
use clap::{Parser, Subcommand};
use cmd_lib::run_cmd;
use dirs::{config_dir, home_dir};
use minijinja::{self, Environment};
use owo_colors::OwoColorize;
use serde::{Deserialize, Serialize};
use tap::prelude::*;
use toml;
use walkdir::WalkDir;

// TODO:
// - Add verbosity
// - Fix weird crashes
// - Fix templates randomly missing
// - Implement dependency installation

const CONFIG_DIR: &str = "dottery";
const CONFIG_FILE: &str = "config.toml";

#[derive(Parser)]
#[command(version)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Clone)]
enum Command {
    /// Install configured packages
    Install { packages: Option<Vec<String>> },
    /// Install dependencies
    InstallDeps {
        /// Only install required dependencies
        #[arg(short, long)]
        required: bool,
        /// Only install optional dependencies
        #[arg(short, long)]
        optional: bool,
    },
    /// Synchronize local dotfiles with remote repo
    Sync,
    /// Print dotfiles directory
    Locate,
    /// Process and copy templates and raw dotfiles to their locations
    Deploy {
        dotfiles: Option<Vec<String>>,
        /// Only copy raw files
        #[arg(short, long)]
        raw: bool,
        /// Only process templates
        #[arg(short, long)]
        template: bool,
    },
}

#[derive(Debug, Deserialize, Serialize)]
struct Config {
    paths: Paths,
}

#[derive(Debug, Deserialize, Serialize)]
struct Paths {
    dotfiles_path: String,
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

#[derive(Clone, Debug, Deserialize)]
struct Dotfiles {
    packages: Vec<Package>,
    dependencies: Option<Dependencies>,
}

#[derive(Clone, Debug, Deserialize)]
struct Package {
    name: String,
    from_aur: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct Dependencies {
    required: Option<Vec<Package>>,
    optional: Option<Vec<Package>>,
}

impl Package {
    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn from_aur(&self) -> bool {
        self.from_aur
    }
}

fn main() -> io::Result<()> {
    let config = read_config()?;

    std::env::set_current_dir(&config.paths.dotfiles_path).expect("dotfiles directory not found");

    let mut settings: toml::Value = std::fs::read_to_string("..toml")
        .expect("`..toml` not found in dotfiles directory")
        .pipe(|s| toml::from_str(&s))
        .expect("failed to parse dotfiles configuration");

    let dotfiles = settings
        .as_table_mut()
        .map(|ss| {
            let dottery = ss
                .remove("dottery")
                .expect("failed to get section `dottery`");

            Dotfiles::deserialize(dottery)
        })
        .expect("failed to parse config file") // HACK: Unwrapping `Option<Result<_>>`
        .expect("failed to parse config file");

    let args = Args::parse();

    match args.command {
        Command::Install {
            packages: packages_to_install,
        } => {
            let cmd: &str;

            let (cmd, packages) =
                filter_packages(dotfiles.packages.iter(), packages_to_install.as_ref());

            install_pkgs(cmd, packages.into_iter())
                .expect(&format!("failed to spawn process `{cmd}`"));

            // TODO: Perform post-installation
        }
        Command::Sync => {
            run_cmd!(git pull).pipe(log_on_err);

            run_cmd! {
                git submodule init;
                git submodule sync;
                git submodule update;
            }
            .pipe(log_on_err);
        }
        Command::Deploy {
            dotfiles: dotfiles_to_deploy,
            template: template_only,
            raw: raw_only,
        } => {
            let home = home_dir().unwrap();
            let home_str = home.to_str().unwrap();

            if !template_only {
                log_msg("Copying raw files");

                copy_raw(&config, home_str);
            }

            if !raw_only {
                log_msg("Processing template files");

                process_templates(dotfiles_to_deploy, settings, &config, home_str).pipe(log_on_err);
            }
        }
        Command::Locate => {
            log_msg("Dotfiles directory");
            println!("{}", config.paths.dotfiles_path);
        }
        Command::InstallDeps {
            required: required_only,
            optional: optional_only,
        } => match dotfiles.dependencies {
            None => (),
            Some(ds) => {
                if !optional_only {
                    if let Some(ps) = ds.required {
                        let (cmd, packages) = filter_packages(ps.iter(), None);

                        install_pkgs(cmd, packages.into_iter()).pipe(log_on_err);
                    };
                }

                if !required_only {
                    if let Some(ps) = ds.optional {
                        let (cmd, packages) = filter_packages(ps.iter(), None);

                        install_pkgs(cmd, packages.into_iter()).pipe(log_on_err);
                    }
                }
            }
        },
    }

    Ok(())
}

fn read_config() -> io::Result<Config> {
    let config_file = config_dir()
        .unwrap()
        .tap_mut(|cf| cf.push(CONFIG_DIR))
        .tap_mut(|cf| cf.push(CONFIG_FILE));

    match std::fs::read_to_string(config_file.clone()) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            std::fs::create_dir_all(config_file.clone().parent().unwrap())?;

            let config_default = Config::default();

            toml::to_string(&config_default)
                .expect("failed to convert config to `toml`")
                .pipe(|s| std::fs::write(config_file, s))
                .expect("failed to write config file");

            Ok(config_default)
        }
        Err(e) => {
            log_error(&format!("failed to read config file: {e}"));

            Ok(Config::default())
        }
        Ok(s) => Ok(toml::from_str(&s).expect("failed to parse config file")),
    }
    .tap_ok_mut(|c| {
        c.paths.dotfiles_path = canonicalize(&c.paths.dotfiles_path)
            .expect(&format!(
                "dotfiles path not found: `{}`",
                &c.paths.dotfiles_path
            ))
            .to_str()
            .unwrap()
            .to_string()
    })
}

fn install_pkgs<'a>(cmd: &str, packages: impl Iterator<Item = &'a str>) -> io::Result<ExitStatus> {
    let mut args = vec!["-S", "--needed"];

    args.extend(packages);

    if args.len() == 2 {
        // HACK: Should return signify that there's no packages to install
        return Ok(ExitStatus::default());
    }

    process::Command::new(cmd)
        .args(args)
        .stdin(Stdio::inherit())
        .spawn()
        .map(|mut c| c.wait())
        .unwrap_or_else(|e| Err(e))
}

fn copy_raw(config: &Config, home_str: &str) {
    let dir = format!("{}/raw/", config.paths.dotfiles_path);
    let files = WalkDir::new(dir);

    files
        .into_iter()
        .filter_map(|r| match r {
            Ok(d) => d.file_type().is_file().then_some(d),
            Err(e) => {
                log_error(&format!("failed to read file: {e}"));
                None
            }
        })
        .for_each(|f| {
            let path = f
                .path()
                .to_string_lossy()
                .to_string()
                .tap(|p| println!("{p}"));

            std::fs::copy(
                &path,
                path.replace(&format!("{}/raw", config.paths.dotfiles_path), home_str),
            )
            .pipe(log_on_err)
        });
}

fn process_templates(
    to_deploy: Option<Vec<String>>,
    settings: toml::Value,
    config: &Config,
    home_str: &str,
) -> io::Result<()> {
    let dir = format!("{}/template/", config.paths.dotfiles_path);
    let files = WalkDir::new(dir);

    let mut env = Environment::new();

    files
        .into_iter()
        .filter_entry(|e| {
            e.file_type().is_dir()
                || if let Some(ref ds) = to_deploy {
                    e.clone()
                        .into_path()
                        .components()
                        .find(|c| {
                            if let Component::Normal(d) = c {
                                ds.contains(&d.to_string_lossy().to_string())
                            } else {
                                false
                            }
                        })
                        .is_some()
                } else {
                    true
                }
        })
        .filter_map(|r| match r {
            Ok(d) => d.file_type().is_file().then_some(d),
            Err(e) => {
                log_error(&format!("failed to read file: {e}"));
                None
            }
        })
        .map(|f| {
            let path = f.path();
            let path_str = path.to_str().unwrap();
            let file = std::fs::File::open(path)?;

            let mut buf = BufReader::new(file);
            let mut contents = String::new();

            match bindet::detect(&mut buf)? {
                Some(_) => return Ok(()),
                None => (),
            };

            buf.rewind()?;
            buf.read_to_string(&mut contents)?;

            println!("{path_str}");

            // NOTE: I hate to do this, but it's safe, since we don't use the values after
            // the closure ends
            let result = env.add_template(
                unsafe { std::mem::transmute::<&str, &'_ str>(path_str) },
                unsafe { std::mem::transmute::<&String, &'_ String>(&contents) },
            );

            if let Err(e) = result {
                log_error(&format!("{e}"));
                return Ok(());
            }

            let tmpl = match env.get_template(path_str) {
                Ok(t) => t,
                Err(e) => {
                    log_error(&format!("{e}"));
                    return Ok(());
                }
            };

            let output = match tmpl.render(&settings) {
                Ok(o) => o,
                Err(e) => {
                    log_error(&format!("{e}"));
                    return Ok(());
                }
            };

            std::fs::write(
                path.to_str().unwrap().replace(
                    &format!("{}/template", config.paths.dotfiles_path),
                    home_str,
                ),
                output,
            )?;

            io::Result::Ok(())
        })
        .collect::<io::Result<()>>()
}

// TODO: Use an enum for package manager
fn filter_packages<'a>(
    packages: impl Iterator<Item = &'a Package>,
    to_install: Option<&Vec<String>>,
) -> (&'a str, Vec<&'a str>) {
    // NOTE: Collecting into a `Vec<_>` isn't very efficient, but is preferred because
    // makes the code more readable. Iterators are different types, so the `if let` would be a
    // lot more cluttered.
    if !is_yay_installed() {
        (
            "pacman",
            packages
                .into_iter()
                .filter_map(|pkg| pkg.from_aur().then_some(pkg.name()))
                .pipe(|pkgs| {
                    if let Some(ps) = to_install {
                        pkgs.filter(|pkg| ps.contains(&pkg.to_string())).collect()
                    } else {
                        pkgs.collect()
                    }
                }),
        )
    } else {
        (
            "yay",
            packages.into_iter().map(Package::name).pipe(|pkgs| {
                if let Some(ps) = to_install {
                    pkgs.filter(|pkg| ps.contains(&pkg.to_string())).collect()
                } else {
                    pkgs.collect()
                }
            }),
        )
    }
}

fn is_yay_installed() -> bool {
    match process::Command::new("yay")
        .arg("--version")
        .stdout(Stdio::inherit())
        .spawn()
    {
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => {
            // Assume that it's just an error on user's side and
            // let them know about it
            log_error(&format!("{e}"));

            // Continue attempting to use `yay`
            true
        }
        Ok(_) => true,
    }
}

fn log_msg(msg: &str) {
    println!("{} {}", ">>".bright_black(), msg.bold());
}

fn log_error(msg: &str) {
    eprintln!("{} {}", "ERROR:".bright_red(), msg.bold());
}

fn log_on_err<T, E: Display>(result: Result<T, E>) {
    let _ = result.map_err(|e| log_error(&format!("{e}")));
}
