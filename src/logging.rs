use owo_colors::OwoColorize;
use std::fmt::Display;

pub(crate) fn log_msg(msg: &str) {
    println!("{} {}", ">>".bright_black(), msg.bold());
}

pub(crate) fn log_error(msg: &str) {
    eprintln!("{} {}", "ERROR:".bright_red(), msg.bold());
}

pub(crate) fn log_on_err<T, E: Display>(result: Result<T, E>) {
    let _ = result.map_err(|e| log_error(&format!("{e}")));
}
