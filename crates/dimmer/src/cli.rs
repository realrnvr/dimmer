//! Command-line parsing without a heavyweight dependency.

use std::ffi::OsString;
use std::path::PathBuf;

/// Parsed command-line options.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Options {
    pub show: bool,
    pub minimised: bool,
    pub quit: bool,
    pub version: bool,
    pub help: bool,
    pub debug: bool,
    pub install_extension: bool,
    pub config: Option<PathBuf>,
    pub page: Option<String>,
}

impl Options {
    /// Parses process arguments (excluding argv[0]).
    ///
    /// # Errors
    ///
    /// Returns a human-readable message for an unknown option or a missing value.
    pub fn parse<I>(arguments: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = OsString>,
    {
        let mut options = Self::default();
        let mut arguments = arguments.into_iter();

        while let Some(argument) = arguments.next() {
            let argument = argument.to_string_lossy();
            match argument.as_ref() {
                "--show" => options.show = true,
                "--minimised" | "--minimized" => options.minimised = true,
                "--quit" => options.quit = true,
                "--version" | "-V" => options.version = true,
                "--help" | "-h" => options.help = true,
                "--debug" | "-d" => options.debug = true,
                "--install-extension" => options.install_extension = true,
                "--config" => {
                    let value =
                        arguments.next().ok_or_else(|| "--config requires a path".to_owned())?;
                    options.config = Some(PathBuf::from(value));
                }
                "--page" => {
                    let value =
                        arguments.next().ok_or_else(|| "--page requires a name".to_owned())?;
                    options.page = Some(value.to_string_lossy().into_owned());
                }
                other if other.starts_with("--config=") => {
                    let value = &other["--config=".len()..];
                    if value.is_empty() {
                        return Err("--config requires a path".to_owned());
                    }
                    options.config = Some(PathBuf::from(value));
                }
                other if other.starts_with("--page=") => {
                    let value = &other["--page=".len()..];
                    if value.is_empty() {
                        return Err("--page requires a name".to_owned());
                    }
                    options.page = Some(value.to_owned());
                }
                other => return Err(format!("unknown option: {other}")),
            }
        }

        Ok(options)
    }

    /// Usage text shown by `--help`.
    #[must_use]
    pub fn help_text() -> String {
        format!(
            "\
Dimmer {version} — dim every display independently

Usage: dimmer [OPTIONS]

Options:
  --show                 Show the settings window (default)
  --minimised            Start hidden in the background
  --quit                 Ask a running instance to save and quit
  --config <path>        Use a specific Dimmer.ini
  --page <name>          Open screens, options, about or debug
  --install-extension    Install the bundled extension (native installs only)
  --version, -V          Print the version and exit
  --debug, -d            Enable verbose logging
  --help, -h             Show this help
",
            version = crate::APP_VERSION
        )
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    fn parse(arguments: &[&str]) -> Result<Options, String> {
        Options::parse(arguments.iter().map(OsString::from))
    }

    #[test]
    fn defaults_to_showing() {
        let options = parse(&[]).unwrap();
        assert!(!options.minimised);
        assert!(!options.quit);
        assert!(options.config.is_none());
    }

    #[test]
    fn parses_flags_and_values() {
        let options =
            parse(&["--minimized", "--debug", "--config", "/tmp/a.ini", "--page=debug"]).unwrap();
        assert!(options.minimised);
        assert!(options.debug);
        assert_eq!(options.config.as_deref(), Some(std::path::Path::new("/tmp/a.ini")));
        assert_eq!(options.page.as_deref(), Some("debug"));
    }

    #[test]
    fn config_equals_form() {
        let options = parse(&["--config=/tmp/b.ini"]).unwrap();
        assert_eq!(options.config.as_deref(), Some(std::path::Path::new("/tmp/b.ini")));
    }

    #[test]
    fn rejects_missing_values() {
        assert!(parse(&["--config"]).is_err());
        assert!(parse(&["--page"]).is_err());
        assert!(parse(&["--config="]).is_err());
    }

    #[test]
    fn rejects_unknown_options() {
        assert!(parse(&["--bogus"]).is_err());
    }
}
