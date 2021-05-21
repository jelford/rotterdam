use std::{fs::File, path::{PathBuf}};
use std::{
    env,
    io::{Read, Write},
    process::{Command, Stdio},
};


struct RotterdamServerInstance {
    pub port: u16,
    _tempdir: tempfile::TempDir,
    process: std::process::Child,
}

impl Drop for RotterdamServerInstance {
    fn drop(&mut self) {
        if let Ok(Some(st)) = self.process.try_wait() {
            log::debug!("Server shut down with status: {}", st);
        } else {
            // Error likely implies process already dead (possible via a race with the try_wait).
            let _ = self.process.kill();
            // Either way we should wait around to read it.
            let _ = self.process.wait();
        }
    }
}

fn start_server() -> RotterdamServerInstance {
    let rotterdam_path = PathBuf::from(
        env::var("ROTTERDAM_BIN").expect("Need to set ROTTERDAM_BIN environment variable"),
    );

    log::info!("Using rotterdam bin from: {}", rotterdam_path.to_string_lossy());

    let working_dir = tempfile::tempdir().expect("Setting up temp directory");
    let workdir_path = PathBuf::from("/tmp/rotterdam-runtime-path"); // working_dir.path();
    if let Err(_) = std::fs::remove_dir_all(&workdir_path) {
        assert!(!workdir_path.exists());
    }
    std::fs::create_dir_all(&workdir_path).expect("Setting up runtime path");

    let config_path = test_data_path("create-repo/server-config.toml");

    let mut server = Command::new(rotterdam_path)
        .arg("--print-info")
        .arg("--config")
        .arg(config_path.as_os_str())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        // .stderr(Stdio::piped())
        .current_dir(workdir_path)
        .spawn()
        .expect("Unable to launch rotterdam server");

    let mut server_info = String::new();
    let server_stdout = server.stdout.as_mut().unwrap();
    server_stdout
        .read_to_string(&mut server_info)
        .expect("reading server info");
    let server_info = json::parse(&server_info)
        .expect("Couldn't understand server_info from rotterdam server process");

    let port = server_info["port"]
        .as_u16()
        .ok_or("port not found in server info")
        .unwrap();

    RotterdamServerInstance {
        port,
        _tempdir: working_dir,
        process: server,
    }
}

fn fetch_admin_token(server: &RotterdamServerInstance) -> String {
    log::trace!("Fetching token...");
    let response = ureq::post(&format!("http://localhost:{}/api/v1/token", server.port))
        .set("Content-Type", "application/json")
        .set("Accept", "application_json")
        .send_bytes(br#"{"name": "test-user"}"#);
    log::trace!("{:?}", response);
    let response = response.unwrap();
    log::trace!(
        "Got token response with HTTP version: {}",
        response.http_version()
    );
    log::trace!(
        "Expecting content-length: {}",
        response.header("Content-Length").unwrap_or("<not send>")
    );

    let result = response.into_string().unwrap();
    log::trace!("Got body from server: {}", result);

    json::parse(&result).unwrap()["token"]
        .take_string()
        .ok_or("No token in response")
        .unwrap();

    result
}

fn test_data_path<P: Into<PathBuf>>(rel_path_from_test_folder: P) -> PathBuf {
    let pkg_root = env!("CARGO_MANIFEST_DIR");
    let pkg_root = PathBuf::from(pkg_root).canonicalize().expect("Canonicalizing package manifest directory");
    let test_data = pkg_root.join("tests");
    let source_file = test_data.join(rel_path_from_test_folder.into());
    assert!(source_file.exists(), "Source file to copy does not exist: {}", source_file.to_string_lossy());
    let source_file = source_file.canonicalize().expect("Canonicalizing source file for copy");

    source_file
}

fn lib_project_dir() -> (tempfile::TempDir, PathBuf) {
    let lib_project_dir = tempfile::tempdir().expect("Setting up temp directory");
    // let p = lib_project_dir.path().to_path_buf();
    // (lib_project, p)

    let p = PathBuf::from("/tmp/fixed-test-dir");
    if let Err(e) = std::fs::remove_dir_all(&p) {
        assert!(!p.exists(), "Unable to delete {}: {}", p.to_string_lossy(), e);
    }
    std::fs::create_dir(&p).unwrap();

    (lib_project_dir, p)
}

// #[test]
fn main() {
    pretty_env_logger::init_timed();
    
    let mut server = start_server();
    let (_tmp, p) = lib_project_dir();
    
    assert!(Command::new("cargo")
        .arg("init")
        .arg("--lib")
        .arg("--name")
        .arg("test-library")
        .current_dir(&p)
        .spawn()
        .unwrap()
        .wait()
        .unwrap()
        .success());

    std::fs::create_dir(&p.join(".cargo")).expect("creating .cargo for lib project");
    let mut config = File::create(&p.join(".cargo").join("config.toml"))
        .expect("creating .cargo/config.toml for lib");
    write!(
        config,
        "\
        [registries]
        rotterdam-test-registry = {{ index = \"http://localhost:{}/repo/testrepo/index\" }}",
        server.port
    )
    .unwrap();
    drop(config);
    std::fs::copy(
        test_data_path("create-repo/library-Cargo.toml"),
        &p.join("Cargo.toml"),
    )
    .unwrap();

    assert!(Command::new("cargo")
        .current_dir(&p)
        .arg("build")
        .spawn()
        .unwrap()
        .wait()
        .unwrap()
        .success());

    let token = fetch_admin_token(&server);
    log::debug!("Token: {}", token);

    let mut login_child = Command::new("cargo")
        .env("CARGO_HOME", p.as_os_str())
        .env("CARGO_LOG", "debug")
        .current_dir(&p)
        .arg("login")
        .arg("--registry")
        .arg("rotterdam-test-registry")
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();

    login_child
        .stdin
        .as_ref()
        .expect("stdin of login child")
        .write_all(&token.as_bytes())
        .unwrap();
    assert!(login_child.wait().unwrap().success());

    // assert!(Command::new("cargo")
    //     .env("CARGO_HOME", p.as_os_str())
    //     .current_dir(&p)
    //     .arg("publish")
    //     .spawn()?
    //     .wait()?
    //     .success());

    log::info!("Server status: {:?}", server.process.try_wait());
}
