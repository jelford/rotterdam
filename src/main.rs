
use std::{collections::HashMap, error::Error, path::PathBuf};
use std::borrow::Cow;
use smtr::{Method, Request, Header, server::{Response, Responder}};
use anyhow::{Result, Context};
use std::process::{Command, Stdio, ExitStatus};
use std::io::Cursor;

/*

/<reponame>/index/             <-- git stuff
/<reponame>/api/v1/crates      <-- downloads
/<reponame>/api                <-- API base path
/<reponame>/api/v1/new         <-- PUT (cargo publish)
/<reponame>/api/v1/crates/{crate_name}/{version}/yank    <-- DELETE (cargo yank)
/<reponame>/api/v1/crates/{crate_name}/{version}/unyank  <-- PUT (cargo unyank)

/ 
*/

#[derive(Debug)]
struct Repo {
    name: String,
}

impl Repo {
    fn git_path(&self) -> PathBuf {
        PathBuf::from("rotterdam-data").join(&self.name).join("index").join(".git")
    }
}

#[derive(Debug)]
struct GitRequest<'a> {
    repo_name: &'a str,
    path: &'a[&'a str],
    args: Vec<(Cow<'a, str>, Cow<'a, str>)>,
}

impl<'a> GitRequest<'a> {
    fn from(repo_name: &'a str, path: &'a[&'a str], args: Vec<(Cow<'a, str>, Cow<'a, str>)>) -> Self {
        GitRequest {
            repo_name, path, args
        }
    }
}

struct App {
    configured_repos: HashMap<&'static str, Repo>,
}

impl App {
    fn handle(&self, req: Request, resp: Responder) -> Result<()> {
        let path_parts: Vec<_> = req.path().split("/").collect();
    
        match (req.method(), path_parts.as_slice()) {
            (Method::GET, ["", repo_name, "index", rest @ ..]) => {
                return self.handle_git_request(GitRequest::from(repo_name, rest, req.query_pairs()), resp);
            },
            _ => {
                resp.send_response(Response::err(404))?;
            }
        }
    
        Ok(())
    }

    fn handle_git_request(&self, req: GitRequest, mut resp: Responder) -> Result<()> {
        println!("Got a request for git stuff: {}, {:?}, {:?}", req.repo_name, req.path, req.args);

        let repo = self.configured_repos.get(req.repo_name).context("No such repo")?;

        let git_path = repo.git_path().canonicalize().context("canonicalizing git repo path")?;

        let chld = Command::new("git")
            .args(&["upload-pack", "--advertise-refs", &git_path.to_str().context("path to string")? ])
            .stdout(Stdio::piped())
            .current_dir(&git_path)
            .spawn().context("spawning git")?;

        let status = chld.wait_with_output()?;
        if status.status.success() {
            resp.set_status(200)?;
            resp.set_header(Header::ContentType, "application/x-git-upload-pack-advertisement")?;
            resp.set_header(Header::CacheControl, "no-cache")?;
            let out = status.stdout;

            let git_content_header = b"001e# service=git-upload-pack\n\
                0000";

            resp.set_header(Header::ContentLength, &(git_content_header.len() + out.len()).to_string())?;

            println!("{}", String::from_utf8(out.clone())?);

            resp.write_body(git_content_header)?;
            resp.stream_body(&mut Cursor::new(out))?;
            Ok(())
        } else {
            Ok(())

        }

        
    }
}


fn main() -> Result<(), Box<dyn Error>> {


    let mut configured_repos = HashMap::new();
    configured_repos.insert("foorepo", Repo { name: "foorepo".into() });

    let app = App {
        configured_repos: configured_repos,
    };


    let chan = smtr::server::serve("127.0.0.1:8080")?;
    for (req, response_writer) in chan {
        println!("Reading request: {:?} : {:?}", req.method(), req.path());
        match app.handle(req, response_writer) {
            Ok(_) => {},
            Err(e) => { eprint!("Something went wrong: {:?}", e); }
        }
    }

    Ok(())
}
