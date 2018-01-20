// std
use std::io;
use std::collections::HashMap;
use std::sync::Arc;
use std::env;
use std::ops::Deref;

// futures
use futures;
use futures::future::{Future, FutureResult};
use futures::Stream;
use futures_cpupool::CpuPool;

// hyper
use hyper;
use hyper::{Chunk, StatusCode};
use hyper::server::{Request, Response, Service};
use hyper::Method::{Get, Post};
use hyper::header::{ContentLength, Location};

// db
use r2d2;
use r2d2_diesel::ConnectionManager;
use diesel::pg::PgConnection;

// miscellaneous
use num_cpus;
use url;

// self
use url_shortener::ResourceManager;
use url_shortener::shorten;
use url_shortener::resolve;

const DEFAULT_DB_URL: &'static str = "postgresql://postgres@localhost:5432";
const LONG_DOMAIN: &'static str = "www.goldsborough.me";
const SHORT_DOMAIN: &'static str = "www.psag.cc";
const PAGES: &[&'static str] = &["index", "resolve-error", "404"];
const PARTIALS: &[&'static str] = &["head"];

pub struct UrlShortener {
    thread_pool: CpuPool,
    resource_manager: Arc<ResourceManager>,
    db_pool: r2d2::Pool<ConnectionManager<PgConnection>>,
}

impl UrlShortener {
    pub fn new() -> UrlShortener {
        let db_url = env::var("DATABASE_URL").unwrap_or(String::from(DEFAULT_DB_URL));
        info!("Connecting to database {}", db_url);
        let db_manager = ConnectionManager::<PgConnection>::new(db_url);
        info!("Creating threadpool with {} threads", num_cpus::get());
        UrlShortener {
            thread_pool: CpuPool::new(num_cpus::get()),
            resource_manager: Arc::new(ResourceManager::new(PAGES, PARTIALS)),
            db_pool: r2d2::Pool::builder().build(db_manager).unwrap(),
        }
    }
}

impl Service for UrlShortener {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;
    type Future = Box<Future<Item = Self::Response, Error = Self::Error>>;
    fn call(&self, request: Request) -> Self::Future {
        // This clone is partly to work around the borrow checker.
        let method = request.method().clone();
        let path = String::from(request.path());
        match (method, &path[..]) {
            // permanent redirect to long domain
            (Get, "/") => {
                let response = Response::new()
                    .with_status(StatusCode::PermanentRedirect)
                    .with_header(Location::new(LONG_DOMAIN));
                Box::new(futures::future::ok(response))
            }
            // main interface
            (Get, "/shorten") => {
                let page = self.resource_manager.get_page("index");
                Box::new(make_response(StatusCode::Ok, page))
            }
            // shorten requests
            (Post, "/shorten") => {
                let db_pool = self.db_pool.clone();
                let future = self.thread_pool.spawn_fn(move || {
                    request
                        .body()
                        .concat2()
                        .and_then(parse_url_from_form)
                        .and_then(move |long_url| {
                            info!("Request to shorten {}", long_url);
                            shorten::get_hash(long_url, db_pool.get().unwrap().deref())
                        })
                        .then(|result| shorten::make_response(SHORT_DOMAIN, result))
                });
                Box::new(future)
            }
            // resolution requests
            (Get, _) if is_valid_hash(&path[1..]) => {
                info!("Request to resolve {}{}", SHORT_DOMAIN, path);
                let db_pool = self.db_pool.clone();
                let resource_manager = self.resource_manager.clone();
                let future = self.thread_pool.spawn_fn(move || {
                    resolve::resolve_url(&path[1..], db_pool.get().unwrap().deref())
                        .then(move |long_url| {
                            resolve::make_response(&resource_manager, long_url)
                        })
                });
                Box::new(future)
            }
            // static resources
            (Get, _) if path.starts_with("/www/") => {
                // Serving static content should be done by NGINX or smth.
                // ENABLE WITH FLAG
                info!("Requesting resource {}", path);
                let resource = ResourceManager::read_resource_from_disk(&path[1..]);
                Box::new(make_response(StatusCode::Ok, resource))
            }
            // 404
            (method, _) => {
                info!("{} request for unknown resource {}", method, path);
                let page = self.resource_manager.get_page("404");
                Box::new(make_response(StatusCode::NotFound, page))
            }
        }
    }
}

fn parse_url_from_form(form_chunk: Chunk) -> FutureResult<String, hyper::Error> {
    let mut form = url::form_urlencoded::parse(form_chunk.as_ref())
        .into_owned()
        .collect::<HashMap<String, String>>();
    debug!("Received request with form data: {:?}", form);
    if let Some(long_url) = form.remove("url") {
        debug!("Found URL in form: {}", long_url);
        futures::future::ok(long_url)
    } else {
        error!("Received POST request at /shorten but with no URL");
        futures::future::err(hyper::Error::from(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Missing form field 'url'",
        )))
    }
}

fn is_valid_hash(hash: &str) -> bool {
    hash.chars().all(char::is_alphanumeric)
}

fn make_response(status: StatusCode, body: String) -> FutureResult<hyper::Response, hyper::Error> {
    futures::future::ok(
        Response::new()
            .with_status(status)
            .with_header(ContentLength(body.len() as u64))
            .with_body(body),
    )
}
