use std::error::Error as StdError;
use std::fmt;
use std::io;
use std::path::Path;
use std::path::StripPrefixError;
use std::result::Result as StdResult;

use git2;
use walkdir;
use yaml_rust::{EmitError, ScanError};

pub type Result<T> = StdResult<T, Error>;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Format,
    Git(String),
    Editor,
    Build(String),
    PluginNotInstalled(String),
    NoPlugin,
    SkipLocal,
    PluginInstalled(String),
    PackFile(String),
    CopyDir(String),
    SaveYaml,
    LoadYaml,
}

impl Error {
    pub fn copy_dir(s: &str) -> Error {
        Error::CopyDir(format!("Fail to copy directory: {}", s))
    }

    pub fn build<T: AsRef<str>>(s: T) -> Error {
        Error::Build(format!("Fail to build plugin: {}", s.as_ref()))
    }

    pub fn plugin_installed<T: AsRef<Path>>(s: T) -> Error {
        Error::PluginInstalled(format!("Plugin already installed under {:?}", s.as_ref()))
    }

    pub fn plugin_not_installed(s: &str) -> Error {
        Error::PluginNotInstalled(format!("{} not installed", s))
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
        Error::Io(err)
    }
}

impl From<git2::Error> for Error {
    fn from(err: git2::Error) -> Error {
        // err.to_string() has extraneous info so use the message only
        Error::Git(err.message().to_string())
    }
}

impl From<walkdir::Error> for Error {
    fn from(err: walkdir::Error) -> Error {
        Error::copy_dir(&err.to_string())
    }
}

impl From<StripPrefixError> for Error {
    fn from(err: StripPrefixError) -> Error {
        Error::copy_dir(&err.to_string())
    }
}

impl From<EmitError> for Error {
    fn from(_: EmitError) -> Error {
        Error::SaveYaml
    }
}

impl From<ScanError> for Error {
    fn from(_: ScanError) -> Error {
        Error::LoadYaml
    }
}

impl StdError for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Error::Format => write!(f, "Invalid format"),
            Error::SaveYaml => write!(f, "Fail to save packfile"),
            Error::LoadYaml => write!(f, "Fail to load packfile"),
            Error::Editor => write!(f, "Can not open editor"),
            Error::NoPlugin => write!(f, "Can not find such plugin"),
            Error::SkipLocal => write!(f, "Local plugin. Skipping"),
            Error::Io(ref e) => write!(f, "{}", e.to_string()),
            Error::Build(ref s)
            | Error::Git(ref s)
            | Error::CopyDir(ref s)
            | Error::PluginInstalled(ref s)
            | Error::PluginNotInstalled(ref s)
            | Error::PackFile(ref s) => write!(f, "{}", s),
        }
        // write!(f, "{}", self.description())
    }
}
