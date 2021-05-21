use std::{collections::HashMap, ops::Deref};
use std::path::{PathBuf, Path};
use std::borrow::Cow;
use std::env;

use thiserror;

#[derive(Clone, Debug)]
pub(crate) struct AppConfig {
    pub git: AppGitConfig,
    pub repos: HashMap<Cow<'static, str>, Repo>,
}

#[derive(Clone, Debug)]
pub(crate) struct AppGitConfig {
    pub path: PathBuf,
    pub author: String,
    pub author_name: String,
    pub author_email: String,
}

#[derive(Clone, Debug)]
pub(crate) struct Repo {
    pub name: Cow<'static, str>,
}

#[derive(thiserror::Error, Debug)]
pub(crate) enum Error {
    #[error("Unable to find configuration file at {0}")]
    FileNotFound(PathBuf),
    #[error("Unable to read configuration file: {0}")]
    FileReadError(#[from] std::io::Error),
    #[error("Configuration file is not valid toml: {0}")]
    FileFormatSyntaxError(#[from] toml::de::Error),
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),
}


pub(crate) fn load<P: Deref<Target=Path>+AsRef<Path>>(path: Option<P>) -> Result<AppConfig, Error> {
    let mut result = AppConfig {
        git: AppGitConfig {
            path: env::current_dir().expect("Unable to determine current working directory").join("rotterdam-data").join("git"),
            author: String::from("rotterdam <rotterdam@rotterdam.jameselford.com>"),
            author_name: String::from("rotterdam"),
            author_email: String::from("rotterdam@rotterdam.jameselford.com"),
        },
        repos: HashMap::new(),
    };

    if let Some(config_path) = path {
        if ! config_path.is_file() {
            return Err(Error::FileNotFound(config_path.to_path_buf()));
        }
        let config = std::fs::read(config_path)?;
        let toml = toml::from_slice::<toml::Value>(&config)?;
        let git_path = 
            toml.get("rotterdam").ok_or(Error::InvalidConfiguration("missing configuration key: rotterdam".into()))?
                .get("git").and_then(|gc| gc.get("filesystem")).and_then(|fs| fs.get("path")).ok_or(Error::InvalidConfiguration("git storage path not specified in config".into()))?
                .as_str()
                .ok_or(Error::InvalidConfiguration("git storage path not a valid string".into()))?;
        let git_path = PathBuf::from(git_path);

        result.git.path = git_path;
        
        if let Some(config_repos) = toml.get("rotterdam").and_then(|rtrdm| rtrdm.get("repos")) {
            let mut repos = HashMap::new();
            match config_repos {
                toml::Value::Table(config_repos) => {
                    for (name, _info) in config_repos.iter() {
                        let name = Cow::from(name.clone());
                        repos.insert(name.clone(), Repo { name: name.clone() });
                    }
                },
                toml::Value::Array(config_repos) => {
                    for name in config_repos {
                        let name = name.as_str().ok_or(Error::InvalidConfiguration("rotterdam.repos must contain repository names when specified as an array".into()))?;
                        let name = Cow::from(name.to_string());
                        repos.insert(name.clone(), Repo { name: name.clone() });
                    }
                }
                _ => {
                    return Err(Error::InvalidConfiguration("rotterdam.repos must be either a table or list of repositories to serve".into()));
                }
            }

            result.repos = repos;
        }
    }

    Ok(result)
}