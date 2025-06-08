use std::{
    ffi::OsStr,
    io::{self, BufReader, Read, Seek},
    path::{Component, Path},
};

use cmd_lib::run_cmd;
use minijinja::Environment;
use tap::prelude::*;
use walkdir::{DirEntry, WalkDir};

use crate::{
    config::Config,
    logging::{log_error, log_on_err},
    packages::command_exists,
};

// HACK?
/// Extensions of binary files
const BIN_EXTENSIONS: [&str; 2] = ["png", "jpg"];

pub(crate) fn copy_raw(config: &Config, home_str: &str, verbose: bool) {
    let dots_dir = format!("{}/raw/", config.paths.dotfiles_path);
    let dot_files = WalkDir::new(dots_dir);

    dot_files
        .into_iter()
        // .filter_entry(|entry| entry.file_type().is_dir() || should_deploy(entry, &to_deploy))
        .filter_map(|entry_res| match entry_res {
            Ok(entry) => entry.file_type().is_file().then_some(entry),
            Err(err) => {
                log_error(&format!("error while reading dir contents: {err}"));
                None
            }
        })
        .for_each(|f| {
            let path_str = f.path().to_string_lossy().to_string().tap(|p| {
                if verbose {
                    println!("{p}")
                }
            });
            let target_path =
                path_str.replace(&format!("{}/raw", config.paths.dotfiles_path), home_str);
            let parent_dir = Path::new(&target_path).parent().unwrap();

            if !parent_dir.exists() {
                std::fs::create_dir_all(parent_dir).pipe(log_on_err);
            }

            std::fs::copy(&path_str, &target_path).pipe(log_on_err)
        });
}

pub(crate) fn process_templates(
    to_deploy: Option<Vec<String>>,
    settings: toml::Value,
    config: &Config,
    home_str: &str,
    verbose: bool,
) -> io::Result<()> {
    let dots_dir = format!("{}/template/", config.paths.dotfiles_path);
    let dot_files = WalkDir::new(dots_dir);

    let env = Environment::new();
    let sass_extensions: [&OsStr; 2] = ["sass".as_ref(), "scss".as_ref()];

    let bin_extensions = BIN_EXTENSIONS.map(OsStr::new);

    dot_files
        .into_iter()
        .filter_entry(|entry| entry.file_type().is_dir() || should_deploy(entry, &to_deploy))
        .filter_map(|entry_res| match entry_res {
            Ok(entry) => entry.file_type().is_file().then_some(entry),
            Err(err) => {
                log_error(&format!("error while reading dir contents: {err}"));
                None
            }
        })
        .map(|f| {
            let path = f.path();
            let path_str = path.to_str().unwrap();
            let file = std::fs::File::open(path)?;

            // Check for binary
            // HACK?
            // NOTE: Skips files with no extension as well
            if path
                .extension()
                .is_none_or(|ext| bin_extensions.contains(&ext))
            {
                return Ok(());
            }

            let mut buf = BufReader::new(file);
            let mut contents = String::new();

            buf.rewind()?;
            buf.read_to_string(&mut contents)?;

            if verbose {
                println!("{path_str}");
            }

            // NOTE: I'll think about it later
            // let tmpl = match env.template_from_str(&contents) {
            //     Ok(x) => x,
            //     Err(e) => {
            //         log_error(&format!("{e}"));
            //         return Ok(());
            //     }
            // };
            // let tmpl = env.template_from_str(&contents).tap_err(|err| log_error(&err.to_string()));
            let tmpl = env.template_from_str(&contents).unwrap();
            // TODO OPTIM: Use `render_to_write()`
            // TODO? Report missing templates
            let output = tmpl.render(&settings).unwrap();

            let target_path_str = path_str.replace(
                &format!("{}/template", config.paths.dotfiles_path),
                home_str,
            );
            let target_path = Path::new(&target_path_str);
            let parent_dir = target_path.parent().unwrap();

            if !parent_dir.exists() {
                std::fs::create_dir_all(parent_dir)?;
            }

            std::fs::write(target_path, output)?;

            match f.path().extension() {
                None => (),
                Some(ext) => {
                    if sass_extensions.contains(&ext) {
                        process_sass(target_path)?
                    }
                }
            }

            Ok(())
        })
        .collect()
}

pub(crate) fn process_sass<P: AsRef<Path>>(path: P) -> std::io::Result<()> {
    if !command_exists("sass") {
        todo!()
        // return cmd_lib::CmdResult::Err();
    }

    let old_path = path.as_ref();
    let new_path = old_path.with_extension("css");

    run_cmd! {
        sass ${old_path} ${new_path} --no-source-map
    }
}

fn should_deploy(entry: &DirEntry, to_deploy: &Option<Vec<String>>) -> bool {
    match to_deploy {
        None => true,
        Some(ref dots) => entry
            .clone()
            .into_path()
            .components()
            .any(|component| match component {
                Component::Normal(dot) => dots.contains(&dot.to_string_lossy().to_string()),
                _ => false,
            }),
    }
}
