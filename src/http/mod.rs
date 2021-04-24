use std::collections::HashMap;

pub mod server;


#[derive(Debug, Clone, Copy)]
pub enum Method {
    GET,
    PUT,
    DELETE,
    POST,
}
#[derive(Debug, PartialEq, Eq, Hash)]
pub enum MediaType {
    TextPlain,
    TextHtml,
    ApplicationJson,
    Other(String),
}
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct AcceptField {
    media_type: MediaType,
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum Header {
    Host,
    UserAgent,
    Accept,
    ContentType,
    ContentLength,
    Authorization,
}

#[derive(Default)]
pub struct Headers {
    data: HashMap<Header, String>,
}

impl Headers {
    pub fn set(&mut self, key: Header, value: String) {
        self.data.insert(key, value);
    }

    pub fn get(&self, key: Header) -> Option<&String> {
        self.data.get(&key)
    }
}


pub struct Request {
    method: Method,
    headers: Headers,
    path: String,
}

impl Request {
    pub fn method(&self) -> Method {
        self.method
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn headers(&self) -> &Headers {
        &self.headers
    }
}