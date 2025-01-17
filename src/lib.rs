#![warn(clippy::all)]
//! ureq is a minimal request library.
//!
//! The goals of this library are:
//!
//! * Minimal dependency tree
//! * Obvious API
//!
//! ```
//! // requires feature: `ureq = { version = "*", features = ["json"] }`
//! #[macro_use]
//! extern crate ureq;
//!
//! fn main() {
//!     // sync post request of some json.
//!     let resp = ureq::post("https://myapi.acme.com/ingest")
//!         .set("X-My-Header", "Secret")
//!         .send_json(json!({
//!             "name": "martin",
//!             "rust": true
//!         }));
//!
//!     // .ok() tells if response is 200-299.
//!     if resp.ok() {
//!       // ....
//!     }
//! }
//! ```
//!
//! # Plain requests
//!
//! Most standard methods (GET, POST, PUT etc), are supported as functions from the
//! top of the library ([`ureq::get`](fn.get.html), [`ureq::post`](fn.post.html),
//! [`ureq::put`](fn.put.html), etc).
//!
//! These top level http method functions create a [Request](struct.Request.html) instance
//! which follows a build pattern. The builders are finished using:
//!
//! * [`.call()`](struct.Request.html#method.call) without a request body.
//! * [`.send()`](struct.Request.html#method.send) with a request body as `Read`.
//! * [`.send_string()`](struct.Request.html#method.send_string) body as string.
//!
//! # JSON
//!
//! By enabling the `ureq = { version = "*", features = ["json"] }` feature,
//! the library supports serde json.
//!
//! * [`request.send_json()`](struct.Request.html#method.send_json) send body as serde json.
//! * [`response.into_json()`](struct.Response.html#method.into_json) transform response to json.
//!
//! # Agents
//!
//! To maintain a state, cookies, between requests, you use an [agent](struct.Agent.html).
//! Agents also follow the build pattern. Agents are created with
//! [`ureq::agent()`](struct.Agent.html).
//!
//! # Content-Length
//!
//! The library will set the content length on the request when using
//! [`.send_string()`](struct.Request.html#method.send_string) or
//! [`.send_json()`](struct.Request.html#method.send_json). In other cases the user
//! can optionally `request.set("Content-Length", 1234)`.
//!
//! For responses, if the `Content-Length` header is present, the methods that reads the
//! body (as string, json or read trait) are all limited to the length specified in the header.
//!
//! # Transfer-Encoding: chunked
//!
//! Dechunking is a response body is done automatically if the response headers contains
//! a `Transfer-Encoding` header.
//!
//! Sending a chunked request body is done by setting the header prior to sending a body.
//!
//! ```
//! let resp = ureq::post("http://my-server.com/ingest")
//!     .set("Transfer-Encoding", "chunked")
//!     .send_string("Hello world");
//! ```
//!
//! # Character encoding
//!
//! By enabling the `ureq = { version = "*", features = ["charset"] }` feature,
//! the library supports sending/receiving other character sets than `utf-8`.
//!
//! For [`response.into_string()`](struct.Response.html#method.into_string) we read the
//! header `Content-Type: text/plain; charset=iso-8859-1` and if it contains a charset
//! specification, we try to decode the body using that encoding. In the absence of, or failing
//! to interpret the charset, we fall back on `utf-8`.
//!
//! Similarly when using [`request.send_string()`](struct.Request.html#method.send_string),
//! we first check if the user has set a `; charset=<whatwg charset>` and attempt
//! to encode the request body using that.
//!

mod agent;
mod body;
mod error;
mod header;
mod pool;
mod request;
mod response;
mod stream;
mod unit;

#[cfg(feature = "json")]
mod serde_macros;

#[cfg(test)]
mod test;

pub use crate::agent::Agent;
pub use crate::error::Error;
pub use crate::header::Header;
pub use crate::request::{Request, IpVersion};
pub use crate::response::Response;

// re-export
#[cfg(feature = "cookie")]
pub use cookie::Cookie;
#[cfg(feature = "json")]
pub use serde_json::{to_value as serde_to_value, Map as SerdeMap, Value as SerdeValue};

/// Agents are used to keep state between requests.
pub fn agent() -> Agent {
    Agent::new().build()
}

/// Make a request setting the HTTP method via a string.
///
/// ```
/// ureq::request("GET", "https://www.google.com").call();
/// ```
pub fn request(method: &str, path: &str) -> Request {
    Agent::new().request(method, path)
}

/// Make a GET request.
pub fn get(path: &str) -> Request {
    request("GET", path)
}

/// Make a HEAD request.
pub fn head(path: &str) -> Request {
    request("HEAD", path)
}

/// Make a POST request.
pub fn post(path: &str) -> Request {
    request("POST", path)
}

/// Make a PUT request.
pub fn put(path: &str) -> Request {
    request("PUT", path)
}

/// Make a DELETE request.
pub fn delete(path: &str) -> Request {
    request("DELETE", path)
}

/// Make a TRACE request.
pub fn trace(path: &str) -> Request {
    request("TRACE", path)
}

/// Make an OPTIONS request.
pub fn options(path: &str) -> Request {
    request("OPTIONS", path)
}

/// Make an CONNECT request.
pub fn connect(path: &str) -> Request {
    request("CONNECT", path)
}

/// Make an PATCH request.
pub fn patch(path: &str) -> Request {
    request("PATCH", path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_http_google() {
        let resp = get("http://www.google.com/").call();
        assert_eq!(
            "text/html; charset=ISO-8859-1",
            resp.header("content-type").unwrap()
        );
        assert_eq!("text/html", resp.content_type());
    }

    #[test]
    #[cfg(feature = "tls")]
    fn connect_https_google() {
        let resp = get("https://www.google.com/").call();
        assert_eq!(
            "text/html; charset=ISO-8859-1",
            resp.header("content-type").unwrap()
        );
        assert_eq!("text/html", resp.content_type());
    }
}
