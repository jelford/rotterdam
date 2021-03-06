use core::panic;
use std::{
    io::{BufRead, BufReader, BufWriter, Cursor},
    marker::PhantomData,
    net::TcpStream,
    sync::mpsc,
    thread,
    time::Duration,
};
use thiserror::Error;

use std::{
    io::{self, Read, Write},
    net::TcpListener,
};

use super::*;

pub type TcpResponseWriter = ResponseWriter<'static, BufWriter<TcpStream>>;

pub fn serve(
    bind_address: &str,
) -> Result<mpsc::Receiver<(impl Request, TcpResponseWriter)>, BindError> {
    let base_url = Url::parse(&format!("http://{}", bind_address))
        .map_err(BindError::InvalidBindUrl)?;
    let listener = TcpListener::bind(bind_address.to_string())
        .map_err(BindError::InvalidBindAddress)?;

    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        for stream in listener.incoming() {
            let stream = stream.expect("Listener thread has died");
            stream
                .set_read_timeout(Some(Duration::from_millis(500)))
                .expect("Setting socket read timeout");

            let response = match stream.try_clone() {
                Err(e) => {
                    log::error!("Unable to clone stream objects for response: {:?}", e);
                    continue;
                }
                Ok(s) => new_response_writer_1_0(s),
            };

            let request = parse_request(&base_url, stream);

            let handle_error = |mut responder: ResponseWriter<_>, code: u16| {
                let _ = responder.send_response(Response::err(code));
            };

            match request {
                Err(HttpError::StreamError(e)) => {
                    log::error!("Got error reading http request: {:?}", e);
                }
                Err(HttpError::ServerError(e)) => {
                    log::error!("Server error reading http request: {:?}", e);
                    handle_error(response, 500);
                }
                Err(HttpError::ClientError(code, reason)) => {
                    log::debug!(
                        "Should respond to client with {} (reason: {})",
                        code,
                        reason
                    );
                    handle_error(response, code);
                }
                Ok(req) => tx.send((req, response)).expect("Request handler has died"),
            }
        }
    });

    Ok(rx)
}

#[derive(Debug, Error)]
pub enum BindError {
    #[error("Unable to listen on http port")]
    HttpListenError(#[source] io::Error),
    #[error("Bind address cannot be base for http url")]
    InvalidBindUrl(#[from] url::ParseError),
    #[error("Bind address cannot be base for http url")]
    InvalidBindAddress(#[source] io::Error),
}

#[derive(Debug, Error)]
pub enum HttpError {
    #[error("Error reading request")]
    StreamError(#[from] io::Error),
    #[error("Server error: {0}")]
    ServerError(String),
    #[error("Bad client request: {0} ({1})")]
    ClientError(u16, &'static str),
}

struct ReceivedRequest {
    method: Method,
    headers: Headers,
    url: Url,
    body: Option<Box<dyn BufRead + Send>>,
}

impl Drop for ReceivedRequest {
    fn drop(&mut self) {
        // Consume whatever's left of our body before letting the connection drop
        if let Some(mut body) = self.body.take() {
            let mut buf = [0u8; 1024];
            while let Ok(left) = body.read(&mut buf) {
                if left == 0 {
                    break;
                }
            }
        }
    }
}

impl Request for ReceivedRequest {
    fn method(&self) -> Method {
        self.method
    }

    fn path(&self) -> &str {
        &self.url.path()
    }

    fn query_string(&self) -> Option<&str> {
        self.url.query()
    }

    fn query_pairs(&self) -> Vec<(Cow<str>, Cow<str>)> {
        let mut pairs = Vec::new();
        for p in self.url.query_pairs() {
            pairs.push((p.0.clone(), p.1.clone()));
        }
        pairs
    }

    fn query_first_value(&self, key: &str) -> Option<Cow<str>> {
        for (k, v) in self.query_pairs() {
            if k == key {
                return Some(v.clone());
            }
        }

        None
    }

    fn headers(&self) -> &Headers {
        &self.headers
    }

    fn read_body(&mut self) -> Result<Option<Vec<u8>>, std::io::Error> {
        if let Some(mut b) = self.body.take() {
            let mut buff = Vec::new();
            b.read_to_end(&mut buff)?;
            Ok(Some(buff))
        } else {
            Ok(None)
        }
    }

    fn take_body(&mut self) -> Option<Box<dyn BufRead + Send>> {
        self.body.take()
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ResponseState {
    Status,
    Headers,
    Body,
}

pub struct ResponseWriter<'a, Stream>
where
    Stream: 'a + Write + Send,
{
    stream: Stream,
    state: ResponseState,
    _lifetime: PhantomData<&'a Stream>,
    status: Option<u16>,
}

fn new_response_writer_1_0<'a, Stream>(s: Stream) -> ResponseWriter<'a, BufWriter<Stream>>
where
    Stream: Write + Send + 'a,
{
    ResponseWriter {
        stream: BufWriter::new(s),
        state: ResponseState::Status,
        _lifetime: PhantomData,
        status: None,
    }
}

impl<'a, Stream> Drop for ResponseWriter<'a, Stream>
where
    Stream: 'a + Write + Send,
{
    fn drop(&mut self) {
        log::debug!("{}", self.status.unwrap_or(0));
    }
}

impl<'a, Stream> ResponseWriter<'a, Stream>
where
    Stream: Write + Send,
{
    fn set_status(&mut self, status: u16) -> Result<(), io::Error> {
        assert!(
            self.state == ResponseState::Status,
            "Invalid state: status code has already been sent; cannot update"
        );
        log::trace!("HTTP/1.0 {}\r\n", status);
        write!(self.stream, "HTTP/1.0 {}\r\n", status)?;
        self.state = ResponseState::Headers;
        self.status = Some(status);
        Ok(())
    }

    fn set_headers(&mut self, headers: Headers) -> Result<(), io::Error> {
        match self.state {
            ResponseState::Status => {
                panic!("Invalid state: status code has not yet been sent; cannot start headers")
            }
            ResponseState::Headers => {}
            ResponseState::Body => {
                panic!("Invalid state: headers have already been sent; cannot update")
            }
        };

        for (h_name, h_value) in headers.iter() {
            log::trace!(
                "Writing header: {}:{}",
                String::from_utf8_lossy(&h_name.as_header_string()),
                String::from_utf8_lossy(h_value)
            );
            self.stream.write_all(&h_name.as_header_string())?;
            self.stream.write_all(": ".as_bytes())?;
            self.stream.write_all(h_value)?;
            self.stream.write_all(b"\r\n")?;
        }
        write!(self.stream, "\r\n")?;

        self.state = ResponseState::Body;

        Ok(())
    }

    fn stream_body(&mut self, mut reader: &mut dyn Read) -> Result<(), io::Error> {
        match self.state {
            ResponseState::Status => {
                log::error!("Invalid state: status code has not yet been sent; cannot start body");
                panic!("Invalid state: status code has not yet been sent; cannot start body")
            }
            ResponseState::Headers => write!(self.stream, "\r\n")?,
            ResponseState::Body => (),
        }
        self.state = ResponseState::Body;
        std::io::copy(&mut reader, &mut self.stream)?;
        log::trace!("Copied body to response");

        Ok(())
    }

    pub fn send_response(&mut self, response: Response) -> Result<(), io::Error> {
        assert!(
            self.state == ResponseState::Status,
            "Invalid state: response has already started"
        );
        log::trace!("Whole response - processing parts...");

        self.set_status(response.status)?;
        self.set_headers(response.headers)?;

        if let Some(mut body_stream) = response.stream {
            log::trace!("Got body stream");
            self.stream_body(&mut body_stream)?;
        }

        Ok(())
    }

    pub fn raw_writer(&mut self) -> &mut dyn Write {
        &mut self.stream
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
        Response {
            status: 200,
            stream: None,
            headers: Headers::default(),
        }
    }

    pub fn err(code: u16) -> Self {
        Response {
            status: code,
            stream: None,
            headers: Headers::default(),
        }
    }

    pub fn builder(status: u16) -> ResponseBuilder {
        ResponseBuilder {
            status,
            stream: None,
            headers: Headers::default(),
        }
    }
}

impl ResponseBuilder {
    pub fn content_type(mut self, content_type: &str) -> Self {
        self.headers
            .set(Header::ContentType, content_type.as_bytes().to_vec());
        self
    }

    pub fn send_file(mut self, f: std::fs::File) -> Self {
        self.headers.set(
            Header::ContentLength,
            f.metadata()
                .expect("metadata")
                .len()
                .to_string()
                .as_bytes()
                .to_vec(),
        );
        self.stream = Some(Box::new(f));
        self
    }

    pub fn body(mut self, b: Vec<u8>) -> Self {
        if self.headers.get(Header::ContentLength).is_none() {
            self.headers.set(
                Header::ContentLength,
                b.len().to_string().as_bytes().to_vec(),
            );
        }
        self.stream = Some(Box::new(Cursor::new(b)));
        self
    }

    pub fn body_from_string(self, s: &str) -> Self {
        let bs = s.as_bytes().to_vec();
        self.body(bs)
    }

    pub fn build(self) -> Response {
        Response {
            status: self.status,
            headers: self.headers,
            stream: self.stream,
        }
    }
}

fn read_until_limited<R>(
    reader: &mut BufReader<R>,
    needle: u8,
    line_len_limit: usize,
) -> Result<Vec<u8>, HttpError>
where
    R: Read,
{
    let mut buf = Vec::new();
    let mut found = false;
    while buf.len() < line_len_limit {
        let cur = reader.fill_buf()?;
        if let Some(newline) = cur.iter().position(|&b| b == needle) {
            buf.extend_from_slice(&cur[..newline]);
            reader.consume(newline + 1);
            found = true;
            break;
        } else {
            let to_consume = cur.len().min(line_len_limit - buf.len());
            buf.extend_from_slice(&cur[..to_consume]);
            reader.consume(to_consume);
        }
    }

    if found {
        Ok(buf)
    } else {
        Err(HttpError::ClientError(400, "Header line too long"))
    }
}

fn parse_request<R>(base_url: &Url, stream: R) -> Result<ReceivedRequest, HttpError>
where
    R: Read + Send + 'static,
{
    let mut stream = BufReader::new(stream);

    let (method, offset) = {
        let buf = stream.fill_buf()?;

        match &buf[0..3] {
            b"GET" => (Method::Get, 4),
            b"PUT" => (Method::Put, 4),
            b"POS" => (Method::Post, 5),
            b"DEL" => (Method::Delete, 7),
            b"OPT" => (Method::Option, 7),
            _ => return Err(HttpError::ServerError("Unable to parse method".into())),
        }
    };
    stream.consume(offset);
    log::trace!("Determined method: {:?}", method);

    let (used, path) = {
        let rest = stream.fill_buf()?;

        let sp_idx = rest
            .iter()
            .position(|&b| b == b' ')
            .ok_or(HttpError::ClientError(
                400,
                "Not enough data in first line - no path",
            ))?;

        (
            sp_idx + 1,
            String::from_utf8_lossy(&rest[..sp_idx]).to_string(),
        )
    };
    stream.consume(used);

    let (used, http_version) = {
        let rest = stream.fill_buf()?;
        let mut sp_idx =
            rest.iter()
                .position(|b| b.is_ascii_whitespace())
                .ok_or(HttpError::ClientError(
                    400,
                    "Http version not specified",
                ))?;

        let http_ver = match &rest[..sp_idx] {
            b"HTTP/1.0" => HttpProtocolVersion::H1_0,
            b"HTTP/1.1" => HttpProtocolVersion::H1_1,
            _ => {
                return Err(HttpError::ClientError(
                    400,
                    "Unrecognized HTTP version",
                ))
            }
        };

        while sp_idx < rest.len() && rest[sp_idx].is_ascii_whitespace() {
            sp_idx += 1;
        }

        (sp_idx, http_ver)
    };
    stream.consume(used);

    log::trace!(
        "Incoming request has http protocol version: {:?}",
        http_version
    );

    let mut headers = Headers::default();

    loop {
        if headers.len() > 100 {
            log::warn!("Stopping after first 100 headers");
            return Err(HttpError::ClientError(400, "Too many headers"));
        }

        let line = read_until_limited(&mut stream, b'\n', 1000)?;
        if line.is_empty() || line.iter().all(|b| b.is_ascii_whitespace()) {
            break;
        }

        let (key, value) = {
            let mut parts = line.splitn(2, |&b| b == b':');
            let key = parts
                .next()
                .ok_or(HttpError::ClientError(400, "Bad header name"))?;
            if key.iter().any(|b| b.is_ascii_whitespace()) {
                return Err(HttpError::ClientError(
                    400,
                    "Whitespace found in header name",
                ));
            }
            let mut value = parts
                .next()
                .ok_or(HttpError::ClientError(400, "Bad header value"))?;
            while !value.is_empty() && value[0].is_ascii_whitespace() {
                value = &value[1..];
            }
            while !value.is_empty() && value[value.len() - 1].is_ascii_whitespace() {
                value = &value[..value.len() - 1];
            }
            if value.is_empty() {
                return Err(HttpError::ClientError(400, "Empty header value"));
            }
            (key, value)
        };

        let header = match key.to_ascii_lowercase().as_slice() {
            b"host" => Header::Host,
            b"accept" => Header::Accept,
            b"content-type" => Header::ContentType,
            b"content-length" => Header::ContentLength,
            b"authorization" => Header::Authorization,
            b"user-agent" => Header::UserAgent,
            _ => Header::Other(Cow::from(key.to_vec())),
        };

        headers.set(header, value.to_vec());
    }

    let body = {
        if let Some(len) = headers.get(Header::ContentLength) {
            let content_len: u64 = String::from_utf8_lossy(len)
                .parse()
                .map_err(|_| HttpError::ClientError(400, "Bad Content-Length"))?;
            if content_len > 10_000 {
                return Err(HttpError::ClientError(400, "Oversized Entity Body"));
            }

            let already_read = Cursor::new(stream.buffer().to_vec());
            let read_buf = stream.into_inner();
            let body: Box<dyn BufRead + Send> = Box::new(BufReader::new(
                already_read.chain(read_buf).take(content_len),
            ));
            Some(body)
        } else {
            None
        }
    };

    if body.as_ref().is_some() {
        log::trace!("Got body");
    }

    let url = base_url
        .join(&path)
        .map_err(|_| HttpError::ClientError(400, "Invalid path"))?;

    Ok(ReceivedRequest { method, headers, url, body })
}

#[cfg(test)]
mod test {
    use super::*;
    use std::io::Cursor;

    fn new_response_writer_for_ref<'a, UnderlyingStream>(
        s: UnderlyingStream,
    ) -> ResponseWriter<'a, BufWriter<UnderlyingStream>>
    where
        UnderlyingStream: Write + Send + 'a,
    {
        ResponseWriter {
            stream: BufWriter::new(s),
            state: ResponseState::Status,
            _lifetime: PhantomData,
            status: None,
        }
    }

    fn base_url() -> Url {
        Url::parse("http://localhost:2000").unwrap()
    }

    #[test]
    fn parse_empty_get_can_extract_path_and_well_known_headers() {
        let req = Cursor::new(
            b"GET /hello HTTP/1.1\r\n\
            Host: localhost:8080\r\n\
            User-Agent: curl/7.71.1\r\n\
            Accept: */*\r\n\
            \r\n",
        );

        let result = parse_request(&base_url(), req);

        let result = result.unwrap();

        assert_eq!(
            result.headers.get(Header::Host),
            Some(b"localhost:8080" as &[u8])
        );
        assert_eq!(
            result.headers.get(Header::UserAgent),
            Some(b"curl/7.71.1" as &[u8])
        );
        assert_eq!(result.headers.get(Header::Accept), Some(b"*/*" as &[u8]));
    }


    #[test]
    fn returns_query_parameters() {
        let req = Cursor::new(
            b"GET /hello?hello=world&hello=again HTTP/1.1\r\n\
            Host: localhost:8080\r\n\
            User-Agent: curl/7.71.1\r\n\
            Accept: */*\r\n\
            \r\n",
        );

        let result = parse_request(&base_url(), req);

        let result = result.unwrap();

        let qps = result.query_pairs();
        let mut query_pairs = qps.iter();
        assert_eq!(query_pairs.next(), Some(&(Cow::from("hello"), Cow::from("world")) ));
        assert_eq!(query_pairs.next(), Some(&(Cow::from("hello"), Cow::from("again")) ));
        assert_eq!(query_pairs.next(), None);

        assert_eq!(result.query_first_value("hello"), Some(Cow::from("world")));

        assert_eq!(result.query_string(), Some("hello=world&hello=again"));

    }

    #[test]
    fn parse_put_with_body_loads_body() {
        let req = Cursor::new(
            b"PUT /hello HTTP/1.1\r\n\
            Host: localhost:8080\r\n\
            User-Agent: curl/7.71.1\r\n\
            Accept: */*\r\n\
            Content-Length: 11\r\n\
            \r\n\
            Hello world",
        );

        let mut result = parse_request(&base_url(), req).unwrap();

        let mut body = result.take_body().unwrap();
        let mut result = Vec::new();
        body.read_to_end(&mut result).unwrap();

        assert_eq!(&result, b"Hello world");
    }

    #[test]
    fn writes_empty_headers_and_body_when_responding_with_okay() {
        let mut bytes = Vec::new();
        let mut output = Cursor::new(&mut bytes);

        {
            let mut response = new_response_writer_for_ref(&mut output);
            let _ = response
                .send_response(
                    Response::builder(200)
                        .body_from_string("Hello world")
                        .build(),
                )
                .unwrap();
        }

        let result = output.get_ref().as_slice();

        assert_eq!(
            result,
            b"HTTP/1.0 200\r\n\
            Content-Length: 11\r\n\
            \r\n\
            Hello world"
        );
    }

    #[test]
    fn read_line_limited_returns_line_excl_newline() {
        let mut input = BufReader::new(Cursor::new(b"line 1\r\nline 2\r\nline 3\n"));

        let result = read_until_limited(&mut input, b'\n', 30).unwrap();
        assert_eq!(result, b"line 1\r".to_vec());

        let result = read_until_limited(&mut input, b'\n', 30).unwrap();
        assert_eq!(result, b"line 2\r");

        let result = read_until_limited(&mut input, b'\n', 30).unwrap();
        assert_eq!(result, b"line 3");
    }
}
