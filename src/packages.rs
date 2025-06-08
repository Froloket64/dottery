use std::{
    io,
    process::{self, ExitStatus, Stdio},
};

pub(crate) fn install_pkgs<'a>(
    cmd: &str,
    packages: impl Iterator<Item = &'a str>,
) -> io::Result<ExitStatus> {
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

pub(crate) fn command_exists(cmd: &str) -> bool {
    std::env::split_paths(&std::env::var("PATH").unwrap())
        .find(|dir| {
            dir.read_dir()
                .unwrap()
                .find(|file_res| match file_res {
                    Err(_) => false,
                    Ok(file) => file.file_name().to_str().map(|s| s == cmd).unwrap_or(false),
                })
                .is_some()
        })
        .is_some()
}

pub(crate) fn get_pkg_man() -> Option<&'static str> {
    if command_exists("yay") {
        Some("yay")
    } else if command_exists("pacman") {
        Some("pacman")
    } else {
        todo!()
    }
}
