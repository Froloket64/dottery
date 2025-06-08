use std::io;

use clap::{Parser, Subcommand};
use cmd_lib::run_cmd;
use dirs::{config_dir, home_dir};
use tap::prelude::*;
use toml;

mod config;
mod logging;
mod packages;
mod processing;

use config::*;
use logging::*;
use packages::*;
use processing::*;

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
        /// Print log
        #[arg(short, long)]
        verbose: bool,
    },
}

fn main() -> io::Result<()> {
    let config_file = config_dir()
        .unwrap()
        .tap_mut(|path| path.push(CONFIG_DIR))
        .tap_mut(|path| path.push(CONFIG_FILE));
    let config = read_config(&config_file)?;

    std::env::set_current_dir(&config.paths.dotfiles_path).expect("dotfiles directory not found");

    // TODO? Defer loading config until actually needed
    let mut settings: toml::Value = std::fs::read_to_string("..toml")
        .expect("`..toml` not found in dotfiles directory")
        .pipe(|s| toml::from_str(&s))
        .expect("failed to parse dotfiles configuration");

    let other: toml::Value = std::fs::read_to_string(".personal.toml")
        .expect("`.personal.toml` not found in dotfiles directory")
        .pipe(|s| toml::from_str(&s))
        .expect("failed to parse dotfiles configuration");

    // TODO? Extend each section instead of replacing it
    // match &mut settings {
    //     _ => todo!(), // ?
    //     toml::Value::Table(lhs) => lhs.iter_mut().filter_map(|x| match x {
    //         toml::
    //     })
    // }
    match (&mut settings, other) {
        (toml::Value::Table(lhs), toml::Value::Table(rhs)) => lhs.extend(rhs.into_iter()),
        _ => todo!(),
    };

    let dotfiles = settings
        .as_table_mut()
        .map(|table| {
            let dottery = table
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
            let pkg_man_opt = get_pkg_man();

            match pkg_man_opt {
                None => todo!(),
                Some(pkg_man) => {
                    let packages = filter_packages(
                        pkg_man,
                        dotfiles.packages.iter(),
                        packages_to_install.as_ref(),
                    );

                    install_pkgs(pkg_man, packages.into_iter())
                        .expect(&format!("failed to spawn process `{pkg_man}`"));

                    // TODO: Perform post-installation
                }
            }
        }
        Command::Sync => {
            run_cmd! {
                git pull;

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
            verbose,
        } => {
            let home = home_dir().unwrap();
            let home_str = home.to_str().unwrap();

            let do_template = !raw_only;
            let do_raw = !template_only;

            if do_template {
                log_msg("Processing template files");

                process_templates(dotfiles_to_deploy, settings, &config, home_str, verbose)
                    .pipe(log_on_err);
            }

            if do_raw {
                log_msg("Copying raw files");

                copy_raw(&config, home_str, verbose);
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
            Some(deps) => {
                let pkg_man_opt = get_pkg_man();
                let do_required = !optional_only;
                let do_optional = !required_only;

                if do_required {
                    if let Some(pkgs) = deps.required {
                        match pkg_man_opt {
                            None => todo!(),
                            Some(pkg_man) => {
                                let packages = filter_packages(pkg_man, pkgs.iter(), None);

                                install_pkgs(pkg_man, packages.into_iter()).pipe(log_on_err);
                            }
                        }
                    };
                }

                if do_optional {
                    if let Some(pkgs) = deps.optional {
                        match pkg_man_opt {
                            None => todo!(),
                            Some(pkg_man) => {
                                let packages = filter_packages(pkg_man, pkgs.iter(), None);

                                install_pkgs(pkg_man, packages.into_iter()).pipe(log_on_err);
                            }
                        }
                    };
                }
            }
        },
    }

    Ok(())
}

// TODO? Use an enum for package manager
// TODO? Use custom/other iterator type for return value to chain with other filter functions (e.g. `filter_recipe()` -> `filter_to_install()`)
fn filter_packages<'a>(
    pkg_man: &str,
    packages: impl Iterator<Item = &'a Package>,
    to_install: Option<&Vec<String>>,
) -> Vec<&'a str> {
    // NOTE: Collecting into a `Vec<_>` isn't very efficient, but is preferred because
    // makes the code more readable. Iterators are different types, so the `if let` would be a
    // lot more cluttered.
    if pkg_man == "yay" {
        packages.into_iter().map(Package::name).pipe(|pkgs| {
            if let Some(ps) = to_install {
                pkgs.filter(|pkg| ps.contains(&pkg.to_string())).collect()
            } else {
                pkgs.collect()
            }
        })
    } else if pkg_man == "pacman" {
        packages
            .into_iter()
            .filter_map(|pkg| pkg.from_aur().then_some(pkg.name()))
            .pipe(|pkgs| {
                if let Some(ps) = to_install {
                    pkgs.filter(|pkg| ps.contains(&pkg.to_string())).collect()
                } else {
                    pkgs.collect()
                }
            })
    } else {
        todo!()
    }
}
