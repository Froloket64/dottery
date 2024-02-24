dottery
-------
A dead simple, yet powerful utility that manages dotfiles:
- Synchronize with **git repo**
- Process **templates**
- **Install** configured packages and dependencies _(TODO)_

# Installation
## From source
You'll simply need to have `cargo` installed. The simplest way to get that done is by heading to rustup.rs and following their instructions.

Then, just run:
```sh
$ git clone https://github.com/Froloket64/dottery.git --depth 1
```
to grab the latest version of `dottery`'s source code, and install it with:
```sh
$ cargo install --path dottery
```

## Release
_Coming soon_

# Usage
<!-- TODO: Create an example set of dotfiles to demonstrate usage -->
Firstly, `dottery` uses the following structure:
```
/
├╴template/ <- Template files
├╴raw/ <- Raw files
╰╴..toml <- Config file
```
Template files are files that contain [`minininja`](https://crates.io/crates/minijinja) templates and are processed separately. They use substitutions configured in the config file (`..toml`).

The structure of `template/` and `raw/` directories must be the same as the path, where each file will be copied. For example, if one has the following structure:
```
/
├╴template/
│ ╰╴.config/
│   ╰╴dottery/
│     ╰╴config.toml
╰╴raw/
  ├╴.config/
  │ ╰╴gzdoom/
  │   ╰╴gzdoom.ini
  ╰╴Pictures/
    ╰╴Wallpapers/
      ╰╴sunset.png
```
then they will end up in the following locations (after [`dot deploy`](#deploy)):
```
~/ <- User's home directory
├╴.config/
│ ├╴dottery/
│ │ ╰╴config.toml
│ ╰╴gzdoom/
│   ╰╴gzdoom.ini
╰╴Pictures/
  ╰╴Wallpapers/
    ╰╴sunset.png
```

## Deploy
You can "deploy" or "copy" dotfiles into their expected locations, while processing templates via `deploy`:
`$ dot deploy`

## Install
You can install configured [packages](#dotfiles) via `install`:
`$ dot install`

Currently, only **Arch Linux** _(btw)_ is supported.

## Configuration
### General
General configuration is stored in `~/.config/dottery/config.toml` (on Unix). It currently contains the following settings:
- `dotfiles_path` - Path, where the dotfiles are kept

### Dotfiles
The `..toml` file is mainly used to set template substitutions, but also can have a `[dottery]` section.

It can contain the following fields:
- `packages` - List of packages that can be installed using the [`install`](#install) command. Example:
```toml
[dottery]
packages = [
	{ name = "kitty",  from_aur = false },
	{ name = "proton", from_aur = true },
]
```

