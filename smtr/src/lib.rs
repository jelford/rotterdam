use std::collections::HashMap;
use std::sync::Arc;

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

impl Header {
    pub fn as_header_string(&self) -> &'static str {
        match self {
            Header::ContentType => "Content-Type",
            Header::ContentLength => "Content-Length",
            _ => {panic!("didn't expect to have to render {:?}", self);}
        }
    }
}

#[derive(Default)]
pub struct Headers {
    data: HashMap<Header, String>,
}

impl Headers {
    pub fn set<V>(&mut self, key: Header, value: V)
        where V : Into<String> {
        self.data.insert(key, value.into());
    }

    pub fn get(&self, key: Header) -> Option<&String> {
        self.data.get(&key)
    }

    pub fn iter(&self) -> impl Iterator<Item=(&Header, &String)> {
        self.data.iter()
    }
}


pub struct Request {
    method: Method,
    headers: Headers,
    path: Arc<String>,
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