use std::{
    fmt::Display,
    io::{self, BufReader, Read, Seek},
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
// - Canonicalize all paths from configs
// - Add logging
// - Add verbosity
// - Fix weird crashes
// - Fix templates randomly missing
// - Implement dependency installation
// - Implement sanity checks
//   - Check if dotfiles dir exists
//   - Check if dotfiles dir contains `..toml`

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
    InstallDeps,
    /// Synchronize local dotfiles with remote repo
    Sync,
    /// Print dotfiles directory
    Locate,
    /// Process and copy templates and raw dotfiles to their locations
    Deploy { dotfiles: Option<Vec<String>> },
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
}

#[derive(Clone, Debug, Deserialize)]
struct Package {
    name: String,
    from_aur: bool,
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

    std::env::set_current_dir(&config.paths.dotfiles_path)?;

    let mut settings: toml::Value = std::fs::read_to_string("..toml")?
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

            // NOTE: Collecting into a `Vec<_>` isn't very efficient, but is preferred because
            // makes the code more readable. Iterators are different types, so the `if let` would be a
            // lot more cluttered.
            let packages: Vec<&str> = if !is_yay_installed() {
                cmd = "pacman";

                dotfiles
                    .packages
                    .iter()
                    .filter_map(|pkg| pkg.from_aur().then_some(pkg.name()))
                    .pipe(|pkgs| {
                        if let Some(ps) = packages_to_install {
                            pkgs.filter(|pkg| ps.contains(&pkg.to_string())).collect()
                        } else {
                            pkgs.collect()
                        }
                    })
            } else {
                cmd = "yay";

                dotfiles.packages.iter().map(Package::name).pipe(|pkgs| {
                    if let Some(ps) = packages_to_install {
                        pkgs.filter(|pkg| ps.contains(&pkg.to_string())).collect()
                    } else {
                        pkgs.collect()
                    }
                })
            };

            install_pkgs(cmd, packages).expect(&format!("failed to spawn process `{cmd}`"));

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
        } => {
            // TODO: Implement selective deployment
            let template_dir = format!("{}/template/", config.paths.dotfiles_path);
            let raw_dir = format!("{}/raw/", config.paths.dotfiles_path);

            let template_files = WalkDir::new(template_dir);
            let raw_files = WalkDir::new(raw_dir);

            let home = home_dir().unwrap();
            let home_str = home.to_str().unwrap();

            log_msg("Copying raw files");

            raw_files
                .into_iter()
                .filter_map(|r| match r {
                    Ok(d) => d.file_type().is_file().then_some(d),
                    Err(e) => {
                        eprintln!("failed to read file: {e}");
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

            let mut env = Environment::new();

            log_msg("Processing template files");

            template_files
                .into_iter()
                // .filter_entry(|e| {
                //     dotfiles_to_deploy.map_or(true, |ds| {
                //         e.clone()
                //             .into_path()
                //             .components()
                //             .find(|c| {
                //                 if let Component::Normal(d) = c {
                //                     ds.contains(&d.to_string_lossy().to_string())
                //                 } else {
                //                     false
                //                 }
                //             })
                //             .is_some()
                //     })
                // })
                .filter_map(|r| match r {
                    Ok(d) => d.file_type().is_file().then_some(d),
                    Err(e) => {
                        eprintln!("failed to read file: {e}");
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
                .pipe(log_on_err);
        }
        Command::Locate => {
            log_msg("Dotfiles directory");
            println!("{}", config.paths.dotfiles_path);
        }
        _ => (),
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
            eprintln!("failed to read config file: {e}");

            Ok(Config::default())
        }
        Ok(s) => Ok(toml::from_str(&s).expect("failed to parse config file")),
    }
}

fn install_pkgs<'a>(
    cmd: &str,
    packages: impl IntoIterator<Item = &'a str>,
) -> io::Result<ExitStatus> {
    let mut args = vec!["-S", "--needed"];

    args.extend(packages);

    process::Command::new(cmd)
        .args(args)
        .stdin(Stdio::inherit())
        .spawn()
        .map(|mut c| c.wait())
        .unwrap_or_else(|e| Err(e))
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
            eprintln!("{e}");

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
