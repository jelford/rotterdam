use core::panic;
use std::{io::{BufWriter}, marker::PhantomData, sync::mpsc, thread};
use thiserror::Error;

use std::{io::{self, Read, Write}, net::{TcpListener, TcpStream}};

use super::*;

pub type TcpResponseWriter = ResponseWriter<'static, BufWriter<TcpStream>>;

pub fn serve(bind_address: &str) -> 
    Result<
        mpsc::Receiver<(
            Request, 
            TcpResponseWriter)>, 
        BindError> {

    let listener = TcpListener::bind(bind_address)?;
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        for stream in listener.incoming() {
            let stream = stream.expect("Listener thread has died");
            let response = match stream.try_clone() {
                Err(e) => {
                    eprint!("Unable to clone stream objects for response: {:?}", e);
                    continue;
                }
                Ok(s) => new_response_writer(s)
            };

            let request = parse_request(stream);
            

            match request {
                Err(HttpError::StreamError(e)) => eprint!("Got error reading http request: {:?}", e),
                Err(HttpError::ServerError(e)) => eprint!("Server error reading http request: {:?}", e),
                Err(HttpError::ClientError(code, reason)) => eprint!("Should respond to client with {} (reason: {})", code, reason),
                Ok(req) => tx.send((req, response)).expect("Request handler has died"),
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

#[derive(Debug, PartialEq, Eq)]
enum ResponseState {
    Status,
    Headers,
    Body,
}

pub struct ResponseWriter<'a, Stream> 
    where Stream: 'a + Write + Send {

    stream: Stream,
    state: ResponseState,
    _lifetime: PhantomData<&'a Stream>,
}


fn new_response_writer<Stream>(s: Stream) -> ResponseWriter<'static, BufWriter<Stream>>
    where Stream: Write+Send+'static {
    ResponseWriter {
        stream: BufWriter::new(s),
        state: ResponseState::Status,
        _lifetime: PhantomData,
    }
}


impl<'a, Stream> ResponseWriter<'a, Stream> where Stream: Write+Send {

    pub fn set_status(&mut self, status: u16) -> Result<(), io::Error> {
        assert!(self.state == ResponseState::Status, "Invalid state: status code has already been sent; cannot update");
        write!(self.stream, "HTTP/1.1 {}\r\n", status)?;
        self.state = ResponseState::Headers;
        Ok(())
    }

    pub fn set_headers(&mut self, headers: Headers) -> Result<(), io::Error> {
        match self.state {
            ResponseState::Status => panic!("Invalid state: status code has not yet been sent; cannot start headers"),
            ResponseState::Headers => {},
            ResponseState::Body => panic!("Invalid state: headers have already been sent; cannot update"),
        };

        for (hname, hvalue) in headers.iter() {
            write!(self.stream, "{}: {}\r\n", hname.as_header_string(), hvalue)?;
        }
        write!(self.stream, "\r\n")?;

        self.state = ResponseState::Body;

        Ok(())
    }

    pub fn stream_body(mut self, mut reader: &mut dyn Read) -> Result<(), io::Error> {
        match self.state {
            ResponseState::Status => panic!("Invalid state: status code has not yet been sent; cannot start body"),
            ResponseState::Headers => write!(self.stream, "\r\n")?,
            ResponseState::Body => (),
        }
        std::io::copy(&mut reader, &mut self.stream)?;
        Ok(())
    }

    pub fn send_response(mut self, response: Response) -> Result<(), io::Error> {
        assert!(self.state == ResponseState::Status, "Invalid state: response has already started");

        self.set_status(response.status)?;
        self.set_headers(response.headers)?;

        if let Some(mut body_stream) = response.stream {
            self.stream_body(&mut body_stream)?;
        }
        
        Ok(())
    }
}

pub struct Response {
    status: u16,
    stream: Option<Box<dyn Read>>,
    headers: Headers,
}

pub struct ResponseBuilder {
    status: u16,
    stream: Option<Box<dyn Read>>,
    headers: Headers,
}

impl Response {
    pub fn ok() -> Self {
        Response { status: 200 , stream: None , headers: Headers::default() }
    }

    pub fn err(code: u16) -> Self {
        Response { status: code , stream: None , headers: Headers::default() }
    }

    pub fn builder(status: u16) -> ResponseBuilder {
        ResponseBuilder { status, stream: None, headers: Headers::default() }
    }

}

impl ResponseBuilder {

    pub fn content_type(mut self, content_type: &str) -> Self {
        self.headers.set(Header::ContentType, content_type);
        self
    }

    pub fn send_file(mut self, f: std::fs::File) -> Self {
        self.headers.set(Header::ContentLength, f.metadata().expect("metadata").len().to_string());
        self.stream = Some(Box::new(f));
        self
    }

    pub fn build(self) -> Response {
        Response {
            status: self.status,
            headers: self.headers,
            stream: self.stream,
        }
    }
}


fn parse_request<R>(mut stream: R) -> Result<Request, HttpError> 
    where R: Read {
    
    let mut buf = [0u8; 1000];

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

    let mut rest = &buf[offset..];

    let path = (|| {
        for (idx, &b) in rest.iter().enumerate() {
            if b == b' ' {
                let path = String::from_utf8_lossy(&rest[..idx]);
                rest = &rest[idx..];
                return Ok(path);
            }
        }
        return Err(HttpError::ClientError(400, "Path not found in first data chunk".into()));
    })()?;

    let path = Arc::new(path.into_owned());

    let mut headers = Headers::default();

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
                            headers.set(h, String::from_utf8_lossy(val));
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

    Ok(Request {
        method,
        path,
        headers
    })
}



#[cfg(test)]
mod test {
    use super::*;
    use std::io::Cursor;


    fn new_response_writer_for_ref<'a, UnderlyingStream>(s: UnderlyingStream) -> ResponseWriter<'a, BufWriter<UnderlyingStream>>
    where UnderlyingStream: Write+Send+'a {

        ResponseWriter {
            stream: BufWriter::new( s),
            state: ResponseState::Status,
            _lifetime: PhantomData
        }
    }

    #[test]
    fn parse_empty_get_can_extract_path_and_well_known_headers() {
        let req = Cursor::new(
            b"GET /hello HTTP/1.1\r\n\
            Host: localhost:8080\r\n\
            User-Agent: curl/7.71.1\r\n\
            Accept: */*\r\n\
            \r\n");


        let result = parse_request(req);
        
        let result = result.unwrap();

        assert_eq!(result.headers.get(Header::Host), Some(&"localhost:8080".to_string()));
        assert_eq!(result.headers.get(Header::UserAgent), Some(&"curl/7.71.1".to_string()));
        assert_eq!(result.headers.get(Header::Accept), Some(&"*/*".to_string()));
    }

    #[test]
    fn response_writes_out_status_and_headers_for_ok() {
        let mut bytes = Vec::new();
        let mut output = Cursor::new(&mut bytes);
        
        {
            let response = new_response_writer_for_ref(&mut output);
            let _ = response.send_response(Response::ok()).unwrap();
        }

        let result = output.get_ref().as_slice();

        assert_eq!(
            result, 
            b"HTTP/1.1 200\r\n\
            \r\n");
    }
}