use std::{
    collections::HashMap,
    error::Error,
    io::{stdout, Write},
    os::unix::prelude::AsRawFd,
    path::PathBuf,
    str::FromStr,
};

use anyhow::Result;
use smtr::{
    server::{Response, TcpResponseWriter},
    Method, Request,
};

use clap;

/*
/api/v1/
       /token            <-- POST (issues new token)
/repo/index/             <-- git stuff
/repo/api/v1/crates      <-- downloads
/repo/api                <-- API base path
/repo/api/v1/new         <-- PUT (cargo publish)
/repo/api/v1/crates/{crate_name}/{version}/yank    <-- DELETE (cargo yank)
/repo/api/v1/crates/{crate_name}/{version}/unyank  <-- PUT (cargo unyank)
*/

#[derive(Debug)]
struct Repo {
    name: String,
}

impl Repo {
    fn git_path(&self) -> PathBuf {
        PathBuf::from("rotterdam-data")
            .join(&self.name)
            .join("index")
            .join(".git")
    }
}

struct App {
    git_storage_path: PathBuf,
    configured_repos: HashMap<&'static str, Repo>,
}

impl App {
    fn handle(&self, req: &dyn Request, mut resp: TcpResponseWriter) -> Result<()> {
        let path_parts: Vec<_> = req.path().split("/").collect();

        match (req.method(), path_parts.as_slice()) {
            (Method::POST, ["", "api", "v1", "token"]) => self.handle_token_create(req, resp),
            (_, ["", "repo", "index"]) => self.handle_git_request(req, resp),
            (_method, ["", "repo", repo_name, "index", rest @ ..]) => {
                let _headers = req.headers().clone();
                let mut path = PathBuf::from(repo_name);
                for r in rest {
                    path.push(r);
                }

                return self.handle_git_request(req, resp);
            }
            _ => {
                resp.send_response(Response::err(404))?;
                Ok(())
            }
        }
    }

    fn handle_token_create(&self, _req: &dyn Request, mut resp: TcpResponseWriter) -> Result<()> {
        let r = Response::builder(200)
            .content_type("application/json")
            .body(br#"{ "token": "12345" }"#.to_vec())
            .build();
        resp.send_response(r)?;

        Ok(())
    }

    fn handle_git_request(&self, _req: &dyn Request, mut resp: TcpResponseWriter) -> Result<()> {
        resp.send_response(
            Response::builder(200)
                .content_type("text/plain")
                .body("Hello world!".as_bytes().to_vec())
                .build(),
        )?;
        Ok(())
        // println!("Got a request for git stuff: {}, {:?}, {:?}", req.repo_name, req.path, req.args);

        // let repo = self.configured_repos.get(req.repo_name).context("No such repo")?;

        // let git_path = repo.git_path().canonicalize().context("canonicalizing git repo path")?;

        // let chld = Command::new("git")
        //     .args(&["upload-pack", "--advertise-refs", &git_path.to_str().context("path to string")? ])
        //     .stdout(Stdio::piped())
        //     .current_dir(&git_path)
        //     .spawn().context("spawning git")?;

        // let status = chld.wait_with_output()?;
        // if status.status.success() {
        //     resp.set_status(200)?;
        //     resp.set_header(Header::ContentType, "application/x-git-upload-pack-advertisement")?;
        //     resp.set_header(Header::CacheControl, "no-cache")?;
        //     let out = status.stdout;

        //     let git_content_header = b"001e# service=git-upload-pack\n\
        //         0000";

        //     resp.set_header(Header::ContentLength, &(git_content_header.len() + out.len()).to_string())?;

        //     println!("{}", String::from_utf8(out.clone())?);

        //     resp.write_body(git_content_header)?;
        //     resp.stream_body(&mut Cursor::new(out))?;
        //     Ok(())
        // } else {
        //     Ok(())
        // }
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

    let mut configured_repos = HashMap::new();
    configured_repos.insert(
        "foorepo",
        Repo {
            name: "foorepo".into(),
        },
    );

    let app = App {
        git_storage_path: PathBuf::from_str("rotterdam-data/git").expect("git storage path"),
        configured_repos: configured_repos,
    };

    let chan = smtr::server::serve("127.0.0.1:8080")?;
    for (req, response_writer) in chan {
        log::debug!("Reading request: {:?} : {:?}", req.method(), req.path());
        match app.handle(&req, response_writer) {
            Ok(_) => {}
            Err(e) => {
                eprint!("Something went wrong: {:?}", e);
            }
        }
    }

    Ok(())
}
