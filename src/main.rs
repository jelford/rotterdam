
use std::{collections::HashMap, error::Error};

use smtr::{Request, server::{ResponseWriter, Response}};


/*

/<reponame>/index/             <-- git stuff
/<reponame>/api/v1/crates      <-- downloads
/<reponame>/api                <-- API base path
/<reponame>/api/v1/new         <-- PUT (cargo publish)
/<reponame>/api/v1/crates/{crate_name}/{version}/yank    <-- DELETE (cargo yank)
/<reponame>/api/v1/crates/{crate_name}/{version}/unyank  <-- PUT (cargo unyank)

/ 
*/

use std::path::PathBuf;
#[derive(Debug)]
struct Repo {
    name: String,
}



fn handle_request(req: Request, resp: ResponseWriter, repos: &HashMap<&str, Repo>) -> Result<(), Box<dyn Error>> {

    let path_parts: Vec<_> = req.path().split("/").collect();
    println!("path_parts: {:?}", path_parts);

    match path_parts.as_slice() {
        ["", repo_name, "index", rest @ ..] => {
            println!("Got index request for {}, rest={:?}", repo_name, rest);
            let mut path = PathBuf::from("rotterdam-data").join(repo_name).join("index").join(".git");
            for p in rest {
                if p.contains(".") || p.contains("\\") {
                    panic!("Naughty path");
                }
                let pre_question = p.split("?").next().expect("no question");
                path.push(pre_question);
            }            

            if !path.is_file() {
                println!("Can't find {:?}", path);
                resp.send_response(Response::err(404))?;
                return Ok(())
            }

            let f = std::fs::File::open(path)?;
            resp.send_response(Response::builder(200)
            .content_type("text/plain; charset=utf-8").send_file(f).build())?;
        }

        _ => {
            resp.send_response(Response::err(404))?;
        }
    }

    Ok(())
}



fn main() -> Result<(), Box<dyn Error>> {


    let mut configured_repos = HashMap::new();
    configured_repos.insert("foorepo", Repo { name: "foorepo".into() });


    let chan = smtr::server::serve("127.0.0.1:8080")?;
    for (req, response_writer) in chan {
        println!("Reading request: {:?} : {:?}", req.method(), req.path());
        match handle_request(req, response_writer, &configured_repos) {
            Ok(_) => {},
            Err(e) => { eprint!("Something went wrong: {:?}", e); }
        }
    }

    Ok(())
}
