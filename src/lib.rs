use std::{env, error::Error, io::{Write, stdout}, os::unix::prelude::AsRawFd, path::{Path, PathBuf}, process::Command, str::FromStr};
use std::process::{Stdio};
use anyhow::{Context, Result, anyhow};
use smtr::{
    server::{Response, TcpResponseWriter},
    Method, Request,
};

mod git_cgi;
mod config;
mod app;


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

    let config: config::AppConfig = config::load(matches.value_of("config").map(PathBuf::from))?;

    let app = app::App::new(config)?;

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
