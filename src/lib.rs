use std::{borrow::Cow, collections::HashMap, env, error::Error, io::{Write, stdout}, os::unix::prelude::AsRawFd, path::{Path, PathBuf}, process::Command, str::FromStr};
use std::process::{Stdio};
use anyhow::{Context, Result, anyhow};
use smtr::{
    server::{Response, TcpResponseWriter},
    Method, Request,
};

mod git_cgi;


/*
/api/v1/
       /token            <-- POST (issues new token)
/repo/<reponame>/index/             <-- git stuff
/repo/<reponame>/api/v1/crates      <-- downloads
/repo/<reponame>/api                <-- API base path
/repo/<reponame>/api/v1/new         <-- PUT (cargo publish)
/repo/<reponame>/api/v1/crates/{crate_name}/{version}/yank    <-- DELETE (cargo yank)
/repo/<reponame>/api/v1/crates/{crate_name}/{version}/unyank  <-- PUT (cargo unyank)
*/

#[derive(Debug)]
struct Repo {
    name: Cow<'static, str>,
}

impl Repo {
    // fn git_path(&self) -> PathBuf {
    //     PathBuf::from("rotterdam-data")
    //         .join(&self.name)
    //         .join("index")
    //         .join(".git")
    // }
}

struct AppConfig {
    git: AppGitConfig,
    repos: HashMap<Cow<'static, str>, Repo>,
}

struct AppGitConfig {
    path: PathBuf,
}

struct App {
    config: AppConfig,
}


fn ensure_index_setup(repo_storage_path: &Path, repo_name: &str) -> Result<()> {
    if ! repo_name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(anyhow!("Repo names must match [a-zA-Z_]. Got: {}", repo_name));
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
            .spawn().context("Commiting repo config")?
            .wait_with_output()?;

        if ! add_result.status.success() {
            log::error!("Preparing repo config for {} (add): {}", repo_name, String::from_utf8_lossy(&add_result.stderr));
            anyhow::bail!("Failed to initialize repo - couldn't add initial config file to git");
        }

        let commit_result = Command::new("git")
            .current_dir(&repo_index_path)
            .args(&["commit", "-m", "(rotterdam): Initializing repo", "--", "config.json"])
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
    fn ready_config(config: AppConfig) -> Result<AppConfig> {
        if ! config.git.path.exists() {
            std::fs::create_dir_all(&config.git.path)?;
        }

        let canonical_path = config.git.path.canonicalize()?;

        for (name, _details) in config.repos.iter() {
            ensure_index_setup(&canonical_path, name)?;
        }

        let config = AppConfig {
            repos: config.repos,
            git: AppGitConfig {
                path: canonical_path
            }
        };

        Ok(config)
    }

    fn new(config: AppConfig) -> Result<Self> {

        let app = App {
            config: App::ready_config(config)?
        };

        Ok(app)
    }

    fn handle(&self, req: &mut dyn Request, mut resp: TcpResponseWriter) -> Result<()> {
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

pub fn main() -> Result<(), Box<dyn Error>> {
    let matches = clap::App::new("rotterdam")
        .version(clap::crate_version!())
        .author(clap::crate_authors!())
        .arg(
            clap::Arg::with_name("print-info")
                .long("print-info")
                .help("If set, the server will print connection details and then close stdout")
                .long_help("If set, the server will print connection details and then close stderr. This is useful when the server is allowed to pick the listen port.")
                .takes_value(false))
        .arg(
            clap::Arg::with_name("config")
                .long("config")
                .short("c")
                .help("Where can I find my configuration?")
                .takes_value(true))
        .get_matches();

    if matches.is_present("print-info") {
        let stdout = stdout();
        let mut s = stdout.lock();
        s.write_all(br#"{ "port": 8080 }"#)?;
        s.flush()?;
        unsafe {
            let _ = libc::close(s.as_raw_fd());
        };
    }

    log::debug!("Running here: {}", env::current_dir()?.to_string_lossy());

    let config: AppConfig = if let Some(config_path) = matches.value_of("config") {
        let path= PathBuf::from_str(config_path)?;
        if ! path.is_file() {
            return Err(format!("Configuration file not found: {}", config_path).into());
        }
        let config = std::fs::read(&path)?;
        let toml = toml::from_slice::<toml::Value>(&config)?;
        let git_path = 
            toml.get("rotterdam").ok_or("missing configuration key: rotterdam")?
                .get("git").and_then(|gc| gc.get("filesystem")).and_then(|fs| fs.get("path")).ok_or("git storage path not specified in config")?
                .as_str()
                .ok_or("git storage path not a valid string")?;
        let git_path = PathBuf::from(git_path);

        let mut repos = HashMap::new();

        if let Some(config_repos) = toml.get("rotterdam").and_then(|rtrdm| rtrdm.get("repos")) {
            match config_repos {
                toml::Value::Table(config_repos) => {
                    for (name, _info) in config_repos.iter() {
                        let name = Cow::from(name.clone());
                        repos.insert(name.clone(), Repo { name: name.clone() });
                    }
                },
                toml::Value::Array(config_repos) => {
                    for name in config_repos {
                        let name = name.as_str().ok_or("rotterdam.repos must contain repository names when specified as an array")?;
                        let name = Cow::from(name.to_string());
                        repos.insert(name.clone(), Repo { name: name.clone() });
                    }
                }
                _ => {
                    return Err(anyhow!("rotterdam.repos must be either a table or list of repositories to serve").into());
                }
            }
        }
        
        AppConfig {
            git: AppGitConfig {
                path: git_path,
            },
            repos,
        }
    } else {
        AppConfig {
            git: AppGitConfig {
                path: env::current_dir()?.join("rotterdam-data").join("git"),
            },
            repos: HashMap::new(),
        }
    };

    let app = App::new(config)?;

    log::debug!("Initialized with {} repos", app.config.repos.len());


    let chan = smtr::server::serve("127.0.0.1:8080")?;
    for (mut req, response_writer) in chan {
        log::debug!("Reading request: {:?} : {:?}", req.method(), req.path());
        match app.handle(&mut req, response_writer) {
            Ok(_) => {}
            Err(e) => {
                eprint!("Something went wrong: {:?}", e);
            }
        }
    }

    Ok(())
}
