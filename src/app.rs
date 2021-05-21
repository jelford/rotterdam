use super::config;
use super::Result;
use super::git_cgi;

use std::{io::{Write}, path::{Path}, process::Command};
use std::process::{Stdio};
use anyhow::{Context, bail};
use smtr::{
    server::{Response, TcpResponseWriter},
    Method, Request,
};


pub(crate) struct App {
    config: config::AppConfig,
}

impl App {

    pub(crate) fn new(config: config::AppConfig) -> Result<Self> {

        let app = App {
            config: App::ready_config(config)?
        };

        log::debug!("Initialized with {} repos", app.config.repos.len());

        Ok(app)
    }

    pub(crate) fn handle(&self, req: &mut dyn Request, mut resp: TcpResponseWriter) -> Result<()> {
        let path_parts: Vec<_> = req.path().split('/').collect();

        match (req.method(), path_parts.as_slice()) {
            (Method::Post, ["", "api", "v1", "token"]) => self.handle_token_create(req, resp),
            (_method, ["", "repo", _repo_name, "index", _rest @ ..]) => {
                self.handle_git_request(req, resp)
            }
            _ => {
                resp.send_response(Response::err(404))?;
                Ok(())
            }
        }
    }
}


fn ensure_index_setup(repo_storage_path: &Path, repo_name: &str) -> Result<()> {
    if ! repo_name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        bail!("Repo names must match [a-zA-Z_]. Got: {}", repo_name);
    }

    let repo_index_path = repo_storage_path.join(repo_name);

    if ! repo_index_path.exists() {
        log::info!("Initializing repo: {} (creating folder at: {})", repo_name, repo_index_path.to_string_lossy());
        std::fs::create_dir(&repo_index_path).context("Initializing repo")?;
    }

    if ! repo_index_path.join(".git").exists() {
        log::debug!("Initializing repo: {} (initializing git)", repo_name);
        let child = Command::new("git")
            .current_dir(&repo_index_path)
            .args(&[
                "init",
                "-b", "master", // Cargo still expects the main branch to be called "master"
                ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn().context("Spawning git-init")?
            ;

        let output = child.wait_with_output().context("Awaiting git-init")?;
        if ! output.status.success() {
            log::error!("{}", String::from_utf8_lossy(&output.stderr));
            anyhow::bail!("Failed to initialize fresh repo ({})", repo_name);
        }
    }

    let git_export_marker = repo_index_path.join(".git").join("git-daemon-export-ok");
    if ! git_export_marker.exists() {
        log::debug!("Initializing repo: {} (setting git cgi export)", repo_name);
        let _ = std::fs::File::create(&git_export_marker).context("Marking repo index for git export")?;
    }

    let cargo_config_file = repo_index_path.join("config.json");
    if ! cargo_config_file.exists() {
        log::debug!("Initializing repo: {} (setting up cargo repo config)", repo_name);
        let mut f = std::fs::File::create(&cargo_config_file)?;
        write!(f, 
            "{{\n\
                \"dl\": \"http://localhost:8080/repo/{}/api/v1/crates\",\n\
                \"api\": \"http://localhost:8080/repo/{}\"\n\
            }}
            ", repo_name, repo_name)?;


        let add_result = Command::new("git")
            .current_dir(&repo_index_path)
            .args(&["add", "config.json"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn().context("Staging repo config")?
            .wait_with_output()?;

        if ! add_result.status.success() {
            log::error!("Preparing repo config for {} (add): {}", repo_name, String::from_utf8_lossy(&add_result.stderr));
            anyhow::bail!("Failed to initialize repo - couldn't add initial config file to git");
        }

        let commit_result = Command::new("git")
            .current_dir(&repo_index_path)
            .args(&["commit", "-m", "(rotterdam): Initializing repo", "--author", "rotterdam <bot@example.com>", "--", "config.json"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn().context("Commiting repo config")?
            .wait_with_output()?;

        if ! commit_result.status.success() {
            log::error!("Preparing repo config for {} (commit): {}", repo_name, String::from_utf8_lossy(&commit_result.stderr));
            anyhow::bail!("Failed to initialize repo - couldn't commit initial config file");
        }
    }
    
    Ok(())
}


impl App {
    fn ready_config(config: config::AppConfig) -> Result<config::AppConfig> {
        if ! config.git.path.exists() {
            std::fs::create_dir_all(&config.git.path)?;
        }

        let canonical_path = config.git.path.canonicalize()?;

        for (name, _details) in config.repos.iter() {
            ensure_index_setup(&canonical_path, name)?;
        }

        let config = config::AppConfig {
            repos: config.repos,
            git: config::AppGitConfig {
                author: config.git.author,
                path: canonical_path,
            }
        };

        Ok(config)
    }

    fn handle_token_create(&self, _req: &dyn Request, mut resp: TcpResponseWriter) -> Result<()> {
        log::debug!("Token create request");
        let r = Response::builder(200)
            .content_type("application/json")
            .body(br#"{ "token": "12345" }"#.to_vec())
            .build();
        resp.send_response(r)?;

        Ok(())
    }


    fn handle_git_request(&self, req: &mut dyn Request, resp: TcpResponseWriter) -> Result<()> {
        log::debug!("Git request");

        git_cgi::handle(&self.config, req, resp)
    }
}