use std::collections::HashMap;
use std::sync::Arc;
use std::borrow::Cow;
use std::io::BufRead;

use url::Url;

pub mod server;

#[derive(Debug)]
pub enum HttpProtocolVersion {
    H1_0,
    H1_1,
}

#[derive(Debug, Clone, Copy)]
pub enum Method {
    GET,
    PUT,
    DELETE,
    POST,
    OPTION,
}

impl Method {
    pub fn as_str(&self) -> &str {
        match self {
            Method::GET => "GET",
            Method::PUT => "PUT",
            Method::DELETE => "DELETE",
            Method::POST => "POST",
            Method::OPTION => "OPTION",
        }
    }
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

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum Header {
    Host,
    UserAgent,
    Accept,
    ContentType,
    ContentLength,
    Authorization,
    CacheControl,
    Other(Cow<'static, [u8]>),
}

impl Header {
    pub fn as_header_string(&self) -> Cow<[u8]> {
        match self {
            Header::Host => Cow::Borrowed(b"Host"),
            Header::UserAgent => Cow::Borrowed(b"User-Agent"),
            Header::ContentType => Cow::Borrowed(b"Content-Type"),
            Header::ContentLength => Cow::Borrowed(b"Content-Length"),
            Header::Accept=> Cow::Borrowed(b"Accept"),
            Header::Authorization => Cow::Borrowed(b"Authorization"),
            Header::CacheControl => Cow::Borrowed(b"Cache-Control"),
            Header::Other(s) => s.clone(),
        }
    }
}

#[derive(Default, Clone)]
pub struct Headers {
    data: HashMap<Header, Cow<'static, [u8]>>,
}

impl Headers {
    pub fn set<V>(&mut self, key: Header, value: V)
        where V : Into<Cow<'static, [u8]>> {
        self.data.insert(key, value.into());
    }

    pub fn get(&self, key: Header) -> Option<&[u8]> {
        self.data.get(&key).map(|cow| cow.as_ref())
    }

    pub fn iter(&self) -> impl Iterator<Item=(&Header, &[u8])> {
        self.data.iter().map(|(name, val)| (name, val.as_ref()))
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }
}


pub struct Request {
    method: Method,
    headers: Headers,
    url: Url,
    body: Option<Box<dyn BufRead+Send>>,
}

impl Request {
    pub fn method(&self) -> Method {
        self.method
    }

    pub fn path(&self) -> &str {
        &self.url.path()
    }

    pub fn query_pairs<'a>(&'a self) -> Vec<(Cow<str>, Cow<str>)> {
        let mut pairs = Vec::new();
        let mut i = 0;
        for p in self.url.query_pairs() {
            pairs.push((p.0.clone(), p.1.clone()));
            i += 1;
            println!("{}", i);
        }
        pairs
    }

    pub fn headers(&self) -> &Headers {
        &self.headers
    }

    pub fn body(&mut self) -> Result<Option<Vec<u8>>, std::io::Error> {
        if let Some(mut b) = self.body.take() {
            let mut buff = Vec::new();
            b.read_to_end(&mut buff)?;
            Ok(Some(buff))
        } else {
            Ok(None)
        }
    }
}