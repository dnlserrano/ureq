use std::io::Read;
use std::sync::{Arc, Mutex};

use lazy_static::lazy_static;
use qstring::QString;
use url::Url;

use crate::agent::{self, Agent, AgentState};
use crate::body::Payload;
use crate::error::Error;
use crate::header::{self, Header};
use crate::pool;
use crate::unit::{self, Unit};
use crate::Response;

#[cfg(feature = "json")]
use super::SerdeValue;

lazy_static! {
    static ref URL_BASE: Url =
        { Url::parse("http://localhost/").expect("Failed to parse URL_BASE") };
}

#[derive(Copy, Clone, Debug)]
pub enum IpVersion {
    V4,
    V6,
}

impl Default for IpVersion {
    fn default() -> Self { IpVersion::V6 }
}

/// Request instances are builders that creates a request.
///
/// ```
/// let mut request = ureq::get("https://www.google.com/");
///
/// let response = request
///     .query("foo", "bar baz") // add ?foo=bar%20baz
///     .call();                 // run the request
/// ```
#[derive(Clone, Default)]
pub struct Request {
    pub(crate) agent: Arc<Mutex<Option<AgentState>>>,

    // via agent
    pub(crate) method: String,
    path: String,

    // from request itself
    pub(crate) headers: Vec<Header>,
    pub(crate) query: QString,
    pub(crate) timeout_connect: u64,
    pub(crate) timeout_read: u64,
    pub(crate) timeout_write: u64,
    pub(crate) redirects: u32,
    pub(crate) preferred_ip_version: IpVersion,
}

impl ::std::fmt::Debug for Request {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::result::Result<(), ::std::fmt::Error> {
        let (path, query) = self
            .to_url()
            .map(|u| {
                let query = unit::combine_query(&u, &self.query, true);
                (u.path().to_string(), query)
            })
            .unwrap_or_else(|_| ("BAD_URL".to_string(), "BAD_URL".to_string()));
        write!(
            f,
            "Request({} {}{}, {:?})",
            self.method, path, query, self.headers
        )
    }
}

impl Request {
    pub(crate) fn new(agent: &Agent, method: String, path: String) -> Request {
        Request {
            agent: Arc::clone(&agent.state),
            method,
            path,
            headers: agent.headers.clone(),
            redirects: 5,
            ..Default::default()
        }
    }

    /// "Builds" this request which is effectively the same as cloning.
    /// This is needed when we use a chain of request builders, but
    /// don't want to send the request at the end of the chain.
    ///
    /// ```
    /// let r = ureq::get("/my_page")
    ///     .set("X-Foo-Bar", "Baz")
    ///     .build();
    /// ```
    pub fn build(&self) -> Request {
        self.clone()
    }

    /// Executes the request and blocks the caller until done.
    ///
    /// Use `.timeout_connect()` and `.timeout_read()` to avoid blocking forever.
    ///
    /// ```
    /// let r = ureq::get("/my_page")
    ///     .timeout_connect(10_000) // max 10 seconds
    ///     .call();
    ///
    /// println!("{:?}", r);
    /// ```
    pub fn call(&mut self) -> Response {
        self.do_call(Payload::Empty)
    }

    fn do_call(&mut self, payload: Payload) -> Response {
        self.to_url()
            .and_then(|url| {
                let reader = payload.into_read();
                let unit = Unit::new(&self, &url, true, &reader);
                unit::connect(&self, unit, true, 0, reader, false)
            })
            .unwrap_or_else(|e| e.into())
    }

    /// Send data a json value.
    ///
    /// Requires feature `ureq = { version = "*", features = ["json"] }`
    ///
    /// The `Content-Length` header is implicitly set to the length of the serialized value.
    ///
    /// ```
    /// #[macro_use]
    /// extern crate ureq;
    ///
    /// fn main() {
    /// let r = ureq::post("/my_page")
    ///     .send_json(json!({ "name": "martin", "rust": true }));
    /// println!("{:?}", r);
    /// }
    /// ```
    #[cfg(feature = "json")]
    pub fn send_json(&mut self, data: SerdeValue) -> Response {
        self.do_call(Payload::JSON(data))
    }

    /// Send data as bytes.
    ///
    /// The `Content-Length` header is implicitly set to the length of the serialized value.
    ///
    /// ```
    /// #[macro_use]
    /// extern crate ureq;
    ///
    /// fn main() {
    /// let body = b"Hello world!";
    /// let r = ureq::post("/my_page")
    ///     .send_bytes(body);
    /// println!("{:?}", r);
    /// }
    /// ```
    pub fn send_bytes(&mut self, data: &[u8]) -> Response {
        self.do_call(Payload::Bytes(data.to_owned()))
    }

    /// Send data as a string.
    ///
    /// The `Content-Length` header is implicitly set to the length of the serialized value.
    /// Defaults to `utf-8`
    ///
    /// ## Charset support
    ///
    /// Requires feature `ureq = { version = "*", features = ["charset"] }`
    ///
    /// If a `Content-Type` header is present and it contains a charset specification, we
    /// attempt to encode the string using that character set. If it fails, we fall back
    /// on utf-8.
    ///
    /// ```
    /// // this example requires features = ["charset"]
    ///
    /// let r = ureq::post("/my_page")
    ///     .set("Content-Type", "text/plain; charset=iso-8859-1")
    ///     .send_string("Hällo Wörld!");
    /// println!("{:?}", r);
    /// ```
    pub fn send_string(&mut self, data: &str) -> Response {
        let text = data.into();
        let charset =
            crate::response::charset_from_content_type(self.header("content-type")).to_string();
        self.do_call(Payload::Text(text, charset))
    }

    /// Send data from a reader.
    ///
    /// The `Content-Length` header is not set because we can't know the length of the reader.
    ///
    /// ```
    /// use std::io::Cursor;
    ///
    /// let text = "Hello there!\n";
    /// let read = Cursor::new(text.to_string().into_bytes());
    ///
    /// let resp = ureq::post("/somewhere")
    ///     .set("Content-Type", "text/plain")
    ///     .send(read);
    /// ```
    pub fn send(&mut self, reader: impl Read + 'static) -> Response {
        self.do_call(Payload::Reader(Box::new(reader)))
    }

    /// Set a header field.
    ///
    /// ```
    /// let r = ureq::get("/my_page")
    ///     .set("X-API-Key", "foobar")
    ///     .set("Accept", "text/plain")
    ///     .call();
    ///
    ///  if r.ok() {
    ///      println!("yay got {}", r.into_string().unwrap());
    ///  } else {
    ///      println!("Oh no error!");
    ///  }
    /// ```
    pub fn set(&mut self, header: &str, value: &str) -> &mut Request {
        header::add_header(&mut self.headers, Header::new(header, value));
        self
    }

    /// Set IP version to use.
    ///
    /// ```
    /// let req = ureq::get("/my_page")
    ///     .set_preferred_ip_version(IpVersion::V4)
    ///     .call();
    ///
    /// assert_eq!(IpVersion::V4, req.preferred_api_version);
    /// ```
    pub fn set_preferred_ip_version(&mut self, ip_version: IpVersion) -> &mut Request {
        self.preferred_ip_version = ip_version;
        self
    }

    /// Returns the value for a set header.
    ///
    /// ```
    /// let req = ureq::get("/my_page")
    ///     .set("X-API-Key", "foobar")
    ///     .build();
    /// assert_eq!("foobar", req.header("x-api-Key").unwrap());
    /// ```
    pub fn header<'a>(&self, name: &'a str) -> Option<&str> {
        header::get_header(&self.headers, name)
    }

    /// A list of the set header names in this request. Lowercased to be uniform.
    ///
    /// ```
    /// let req = ureq::get("/my_page")
    ///     .set("X-API-Key", "foobar")
    ///     .set("Content-Type", "application/json")
    ///     .build();
    /// assert_eq!(req.header_names(), vec!["x-api-key", "content-type"]);
    /// ```
    pub fn header_names(&self) -> Vec<String> {
        self.headers
            .iter()
            .map(|h| h.name().to_ascii_lowercase())
            .collect()
    }

    /// Tells if the header has been set.
    ///
    /// ```
    /// let req = ureq::get("/my_page")
    ///     .set("X-API-Key", "foobar")
    ///     .build();
    /// assert_eq!(true, req.has("x-api-Key"));
    /// ```
    pub fn has<'a>(&self, name: &'a str) -> bool {
        header::has_header(&self.headers, name)
    }

    /// All headers corresponding values for the give name, or empty vector.
    ///
    /// ```
    /// let req = ureq::get("/my_page")
    ///     .set("X-Forwarded-For", "1.2.3.4")
    ///     .set("X-Forwarded-For", "2.3.4.5")
    ///     .build();
    /// assert_eq!(req.all("x-forwarded-for"), vec![
    ///     "1.2.3.4",
    ///     "2.3.4.5",
    /// ]);
    /// ```
    pub fn all<'a>(&self, name: &'a str) -> Vec<&str> {
        header::get_all_headers(&self.headers, name)
    }

    /// Set a query parameter.
    ///
    /// For example, to set `?format=json&dest=/login`
    ///
    /// ```
    /// let r = ureq::get("/my_page")
    ///     .query("format", "json")
    ///     .query("dest", "/login")
    ///     .call();
    ///
    /// println!("{:?}", r);
    /// ```
    pub fn query(&mut self, param: &str, value: &str) -> &mut Request {
        self.query.add_pair((param, value));
        self
    }

    /// Set query parameters as a string.
    ///
    /// For example, to set `?format=json&dest=/login`
    ///
    /// ```
    /// let r = ureq::get("/my_page")
    ///     .query_str("?format=json&dest=/login")
    ///     .call();
    /// println!("{:?}", r);
    /// ```
    pub fn query_str(&mut self, query: &str) -> &mut Request {
        self.query.add_str(query);
        self
    }

    /// Timeout for the socket connection to be successful.
    ///
    /// The default is `0`, which means a request can block forever.
    ///
    /// ```
    /// let r = ureq::get("/my_page")
    ///     .timeout_connect(1_000) // wait max 1 second to connect
    ///     .call();
    /// println!("{:?}", r);
    /// ```
    pub fn timeout_connect(&mut self, millis: u64) -> &mut Request {
        self.timeout_connect = millis;
        self
    }

    /// Timeout for the individual reads of the socket.
    ///
    /// The default is `0`, which means it can block forever.
    ///
    /// ```
    /// let r = ureq::get("/my_page")
    ///     .timeout_read(1_000) // wait max 1 second for the read
    ///     .call();
    /// println!("{:?}", r);
    /// ```
    pub fn timeout_read(&mut self, millis: u64) -> &mut Request {
        self.timeout_read = millis;
        self
    }

    /// Timeout for the individual writes to the socket.
    ///
    /// The default is `0`, which means it can block forever.
    ///
    /// ```
    /// let r = ureq::get("/my_page")
    ///     .timeout_write(1_000)   // wait max 1 second for sending.
    ///     .call();
    /// println!("{:?}", r);
    /// ```
    pub fn timeout_write(&mut self, millis: u64) -> &mut Request {
        self.timeout_write = millis;
        self
    }

    /// Basic auth.
    ///
    /// These are the same
    ///
    /// ```
    /// let r1 = ureq::get("http://localhost/my_page")
    ///     .auth("martin", "rubbermashgum")
    ///     .call();
    ///  println!("{:?}", r1);
    ///
    /// let r2 = ureq::get("http://martin:rubbermashgum@localhost/my_page").call();
    /// println!("{:?}", r2);
    /// ```
    pub fn auth(&mut self, user: &str, pass: &str) -> &mut Request {
        let pass = agent::basic_auth(user, pass);
        self.auth_kind("Basic", &pass)
    }

    /// Auth of other kinds such as `Digest`, `Token` etc.
    ///
    /// ```
    /// let r = ureq::get("http://localhost/my_page")
    ///     .auth_kind("token", "secret")
    ///     .call();
    /// println!("{:?}", r);
    /// ```
    pub fn auth_kind(&mut self, kind: &str, pass: &str) -> &mut Request {
        let value = format!("{} {}", kind, pass);
        self.set("Authorization", &value);
        self
    }

    /// How many redirects to follow.
    ///
    /// Defaults to `5`. Set to `0` to avoid redirects and instead
    /// get a response object with the 3xx status code.
    ///
    /// If the redirect count hits this limit (and it's > 0), a synthetic 500 error
    /// response is produced.
    ///
    /// ```
    /// let r = ureq::get("/my_page")
    ///     .redirects(10)
    ///     .call();
    /// println!("{:?}", r);
    /// ```
    pub fn redirects(&mut self, n: u32) -> &mut Request {
        self.redirects = n;
        self
    }

    // pub fn retry(&self, times: u16) -> Request {
    //     unimplemented!()
    // }
    // pub fn sortQuery(&self) -> Request {
    //     unimplemented!()
    // }
    // pub fn sortQueryBy(&self, by: Box<Fn(&str, &str) -> usize>) -> Request {
    //     unimplemented!()
    // }
    // pub fn ca<S>(&self, accept: S) -> Request
    //     where S: Into<String> {
    //     unimplemented!()
    // }
    // pub fn cert<S>(&self, accept: S) -> Request
    //     where S: Into<String> {
    //     unimplemented!()
    // }
    // pub fn key<S>(&self, accept: S) -> Request
    //     where S: Into<String> {
    //     unimplemented!()
    // }
    // pub fn pfx<S>(&self, accept: S) -> Request // TODO what type? u8?
    //     where S: Into<String> {
    //     unimplemented!()
    // }

    /// Get the method this request is using.
    ///
    /// Example:
    /// ```
    /// let req = ureq::post("/somewhere")
    ///     .build();
    /// assert_eq!(req.get_method(), "POST");
    /// ```
    pub fn get_method(&self) -> &str {
        &self.method
    }

    /// Get the url this request was created with.
    ///
    /// This value is not normalized, it is exactly as set.
    /// It does not contain any added query parameters.
    ///
    /// Example:
    /// ```
    /// let req = ureq::post("https://cool.server/innit")
    ///     .build();
    /// assert_eq!(req.get_url(), "https://cool.server/innit");
    /// ```
    pub fn get_url(&self) -> &str {
        &self.path
    }

    /// Normalizes and returns the host that will be used for this request.
    ///
    /// Example:
    /// ```
    /// let req1 = ureq::post("https://cool.server/innit")
    ///     .build();
    /// assert_eq!(req1.get_host().unwrap(), "cool.server");
    ///
    /// let req2 = ureq::post("/some/path")
    ///     .build();
    /// assert_eq!(req2.get_host().unwrap(), "localhost");
    /// ```
    pub fn get_host(&self) -> Result<String, Error> {
        self.to_url()
            .map(|u| u.host_str().unwrap_or(pool::DEFAULT_HOST).to_string())
    }

    /// Returns the scheme for this request.
    ///
    /// Example:
    /// ```
    /// let req = ureq::post("https://cool.server/innit")
    ///     .build();
    /// assert_eq!(req.get_scheme().unwrap(), "https");
    /// ```
    pub fn get_scheme(&self) -> Result<String, Error> {
        self.to_url().map(|u| u.scheme().to_string())
    }

    /// The complete query for this request.
    ///
    /// Example:
    /// ```
    /// let req = ureq::post("https://cool.server/innit?foo=bar")
    ///     .query("format", "json")
    ///     .build();
    /// assert_eq!(req.get_query().unwrap(), "?foo=bar&format=json");
    /// ```
    pub fn get_query(&self) -> Result<String, Error> {
        self.to_url()
            .map(|u| unit::combine_query(&u, &self.query, true))
    }

    /// The normalized path of this request.
    ///
    /// Example:
    /// ```
    /// let req = ureq::post("https://cool.server/innit")
    ///     .build();
    /// assert_eq!(req.get_path().unwrap(), "/innit");
    /// ```
    pub fn get_path(&self) -> Result<String, Error> {
        self.to_url().map(|u| u.path().to_string())
    }

    fn to_url(&self) -> Result<Url, Error> {
        URL_BASE
            .join(&self.path)
            .map_err(|e| Error::BadUrl(format!("{}", e)))
    }
}
