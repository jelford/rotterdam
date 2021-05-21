
use super::config::AppConfig;
use super::{Request, TcpResponseWriter, Response};

use anyhow::{Result, Context, bail};

use std::{ffi::OsString, io::{Read}, process::Command};
use std::process::{Stdio};


#[cfg(target_family = "unix")]
fn as_os_str(bytes_from_network: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStrExt;
    use std::ffi::OsStr;
    let result = OsStr::from_bytes(&bytes_from_network);
    result.to_os_string()
}


// TODO: untested (no access to windows machine)
#[cfg(target_family = "windows")]
fn as_os_str(bytes_from_network: &[u8]) -> OsString {
    use std::os::windows::ffi::OsStrExt;
    use std::ffi::OsString;
    OsString::from_wide(&bytes_from_network)
}

pub(crate) fn handle(config: &AppConfig, req: &mut dyn Request, mut resp: TcpResponseWriter) -> Result<()> {
    let (git_cgi_path, repo_name) = {
        let path = req.path(); // /repo/<repo_name>/index/...
        let mut parts = path.splitn(5, '/');
        let _ = parts.next(); // /
        let _ = parts.next(); // repo/
        let repo_name = parts.next();  // <repo_name>/
        let _ = parts.next(); // index/
        let rest = parts.next(); // git path

        if repo_name.is_none() || rest.is_none() {
            log::debug!("Bad path in git request: {}", path);
            resp.send_response(Response::err(404))?;
            return Ok(())
        }

        let (repo_name, rest) = (repo_name.unwrap(), rest.unwrap());

        (format!("/{}/.git/{}", repo_name, rest), repo_name)
    };

    let repo = config.repos.get(repo_name);
    if repo.is_none() {
        log::debug!("Repo not found: {}", repo_name);
        resp.send_response(Response::err(404))?;
        return Ok(());
    }

    if let Some(service) = req.query_first_value("service") {
        if service != "git-upload-pack" {
            log::debug!("Received git service request for unsupported service: {}", service);
            resp.send_response(Response::err(400))?;
            return Ok(());
        }
    }

    let mut git_command = Command::new("git");

    let proj_root = config.git.path.as_os_str();
    log::debug!("GIT_PROJECT_ROOT={:?}", proj_root);
    git_command.env("GIT_PROJECT_ROOT", proj_root);

    if let Some(content_length) = req.headers().get(smtr::Header::ContentLength) {
        let result: Result<usize, _> = String::from_utf8_lossy(content_length).parse();
        if result.is_err() {
            log::debug!("Bad content-length");
            resp.send_response(Response::err(400))?;
            return Ok(());
        }
        let result = result.unwrap();
        let content_length_value = format!("{}", result);
        log::debug!("CONTENT_LENGTH={}", content_length_value);
        git_command.env("CONTENT_LENGTH", content_length_value);
    }
    
    if let Some(content_type) = req.headers().get(smtr::Header::ContentType) {
        git_command.env("CONTENT_TYPE", as_os_str(content_type));
    }

    let meth = req.method();
    let req_meth_value = meth.as_str();
    log::debug!("REQUEST_METHOD={}", req_meth_value);
    git_command.env("REQUEST_METHOD", req_meth_value);

    if let Some(query_str) = req.query_string() {
        log::debug!("QUERY_STRING={}", query_str);
        git_command.env("QUERY_STRING", query_str);
    }
    
    log::debug!("PATH_INFO={}", git_cgi_path);
    git_command.env("PATH_INFO", &git_cgi_path);
    

    let mut git = git_command
        .args(&["http-backend"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawning git backend")?;


    let mut git_stdin = git.stdin.take().unwrap();

    if let Some(mut body) = req.take_body() {
        std::io::copy(&mut body, &mut git_stdin)?;
    }
    drop(git_stdin);

    let result = git.wait_with_output().context("Git backend")?;
    if ! result.status.success() {
        log::error!("Error in git backend: {}", String::from_utf8_lossy(&result.stderr));
        resp.send_response(Response::err(500))?;
        bail!("Failed to read git backend");
    }

    log::debug!("Git stderr: {}", String::from_utf8_lossy(&result.stderr));

    let writer = resp.raw_writer();
    let status_line = std::io::Cursor::new(b"HTTP/1.0 200\r\nConnection: close\r\n");
    let git_response = std::io::Cursor::new(result.stdout);

    std::io::copy(&mut status_line.chain(git_response), writer)?;
    Ok(())
}