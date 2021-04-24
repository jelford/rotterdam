
use std::error::Error;

mod http;

/*

/<reponame>/index/             <-- git stuff
/<reponame>/api/v1/crates      <-- downloads
/<reponame>/api                <-- API base path
/<reponame>/api/v1/new         <-- PUT (cargo publish)
/<reponame>/api/v1/crates/{crate_name}/{version}/yank    <-- DELETE (cargo yank)
/<reponame>/api/v1/crates/{crate_name}/{version}/unyank  <-- PUT (cargo unyank)

/ 
*/


fn main() -> Result<(), Box<dyn Error>> {

    let chan = http::server::serve("127.0.0.1:8080")?;
    for req in chan {
        println!("Reading request: {:?} : {:?}", req.method(), req.path());
        let headers = req.headers();
        let host = headers.get(http::Header::Host);
        println!("Client sent host: {:?}", host);

        req.respond(http::server::Response::ok())?
    }

    Ok(())
}
