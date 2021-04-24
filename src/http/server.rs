use std::{ops::Deref, sync::mpsc, thread};
use io::BufReader;
use thiserror::Error;

use std::{collections::HashMap, io::{self, Read, Write}, net::{TcpListener, TcpStream}};

use super::*;


pub fn serve(bind_address: &str) -> Result<mpsc::Receiver<RequestExt>, BindError> {

    let listener = TcpListener::bind(bind_address)?;
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        for stream in listener.incoming() {
            let stream = stream.expect("Listener thread has died");
            let request = parse_request(stream);

            match request {
                Err(HttpError::StreamError(e)) => eprint!("Got error reading http request: {:?}", e),
                Err(HttpError::ServerError(e)) => eprint!("Server error reading http request: {:?}", e),
                Err(HttpError::ClientError(code, reason)) => eprint!("Should respond to client with {} (reason: {})", code, reason),
                Ok(req) => tx.send(req).expect("Request handler has died"),
            }
        }
    });

    Ok(rx)
}

#[derive(Debug, Error)]
pub enum BindError{
    #[error("Unable to listen on http port")]
    HttpListenError(#[from] io::Error),
}

#[derive(Debug, Error)]
pub enum HttpError {
    #[error("Error reading request")]
    StreamError(#[from] io::Error),
    #[error("Server error: {0}")]
    ServerError(String),
    #[error("Bad client request: {0} ({1})")]
    ClientError(u16, String),

}


pub struct RequestExt {
    request: Request,
    responder: Box<dyn Write+Send>
}

impl Deref for RequestExt {
    type Target = Request;

    fn deref(&self) -> &Request {
        &self.request
    }
}


impl RequestExt {
    pub fn respond(self, response: Response) -> Result<(), io::Error> {
        let mut responder = io::BufWriter::new(self.responder);
        let status = format!("HTTP/1.1 {}\r\n\r\n", response.status);
        responder.write(status.as_bytes()).map(|_| ())?;
        Ok(())
    }
}

pub struct Response {
    status: u16,
    

}

impl Response {
    pub const fn ok() -> Self {
        Response { status: 200 }
    }
}


struct RequestExtBuilder {
    method: Method,
    headers: Headers,
    path: String,
    body: Option<Box<dyn Read+Send>>,
}

impl RequestExtBuilder {
    fn new(method: Method, path: &str) -> Self {
        RequestExtBuilder {
            method, path: path.into(), headers: Headers::default(), body: None,
        }
    }

    fn build(self, stream: TcpStream) -> RequestExt {
        RequestExt {
            request: Request {
                method: self.method,
                headers: self.headers,
                path: self.path,
            },
            responder: Box::new(stream),
        }
    }

    fn header(&mut self, key: Header, value: &str) -> &mut Self {
        println!("Found header: [{:?}], [{}]", key, value);
        self.headers.set(key, value.into());
        self
    }
}


fn parse_request(mut stream: TcpStream) -> Result<RequestExt, HttpError> {
    
    let mut buf = [0u8; 1024];

    let len_read =  stream.read(&mut buf)?;
    if len_read < "GET / HTTP".len() {
        return Err(HttpError::ClientError(400, "Not enough data".into()));
    }

    let (method, offset) = match &buf[0..3] {
        b"GET" => (Method::GET, 4),
        b"PUT" => (Method::PUT, 4),
        b"POS" => (Method::POST, 5),
        b"DEL" => (Method::DELETE, 7),
        _ => Err(HttpError::ServerError("Unable to parse method".into()))?
    };

    let mut req = Option::None;

    let mut rest = &buf[offset..];

    for (idx, &b) in rest.iter().enumerate() {
        if b == b' ' {
            req = Some(RequestExtBuilder::new(method, &String::from_utf8_lossy(&rest[..idx])));
            rest = &rest[idx..];
            break;
        }
    }

    let mut req = req.ok_or(HttpError::ClientError(400, "Parsing req-line".into()))?;


    for (idx, &b) in rest.iter().enumerate() {
        if b == b'\n' {
            rest = &rest[idx+1..];
            break;
        }
    }

    let mut cnt = 0;

    'headers: loop {

        let mut colon_pos = None;
        for (idx, &b) in rest.iter().enumerate() {
            if b == b':' {
                colon_pos = colon_pos.or(Some(idx));
                continue;
            }

            if b == b'\r' {
                let &nxt = rest.get(idx+1).ok_or(HttpError::ClientError(400, "Line break not at end of line".into()))?;
                if nxt == b'\n' {
                    if idx == 0 {
                        rest = &rest[idx+2..];
                        break 'headers;
                    } else {
                        let colon_pos = colon_pos.ok_or(HttpError::ClientError(400, "No colon in header".into()))?;
                        let key = &rest[..colon_pos];
                        let val = &rest[colon_pos+2..idx];

                        let header = match key.to_ascii_lowercase().as_slice() {
                            b"host" => Some(Header::Host),
                            b"accept" => Some(Header::Accept),
                            b"content-type" => Some(Header::ContentType),
                            b"content-length" => Some(Header::ContentLength),
                            b"authorization" => Some(Header::Authorization),
                            b"user-agent" => Some(Header::UserAgent),
                            _ => None
                        };
                        if let Some(h) = header {
                            req.header(h, &String::from_utf8_lossy(val));
                        }
                        rest = &rest[idx+2..];
                        continue 'headers;
                    }
                }
            }
        }
        cnt += 1;
        if cnt > 100 {
            eprintln!("Stopping after first 100 headers");
            break;
        }
    }

    Ok(req.build(stream))
}

