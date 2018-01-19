extern crate chrono;
extern crate futures_cpupool;
extern crate futures;
extern crate handlebars;
extern crate hyper;
extern crate postgres;
extern crate rand;
extern crate r2d2_diesel;
extern crate r2d2;
extern crate serde;
extern crate url;

extern crate pretty_env_logger;
#[macro_use]
extern crate log;

#[macro_use]
extern crate diesel;

use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::error::Error;
use std::env;
use std::ops::Deref;

use futures::future::{Future, FutureResult};
use futures::Stream;
use futures_cpupool::CpuPool;

use hyper::{Chunk, StatusCode};
use hyper::server::{Http, Request, Response, Service};
use hyper::Method::{Get, Post};
use hyper::header::{ContentLength, ContentType, Location};

use diesel::prelude::*;
use diesel::pg::PgConnection;
use r2d2_diesel::ConnectionManager;

use handlebars::Handlebars;
use serde::ser::Serialize;

pub mod schema;
pub mod models;

const DEFAULT_DB_URL: &'static str = "postgresql://goldsborough@localhost:5432";
const NUMBER_OF_HASH_ATTEMPTS: usize = 100;

const LONG_DOMAIN: &'static str = "www.goldsborough.me";
const SHORT_DOMAIN: &'static str = "www.psag.cc";

const INDEX_PAGE: &'static str = "index.html";
const SHORTEN_SUCCESS_PAGE: &'static str = "shorten-success.html";
const SHORTEN_ERROR_PAGE: &'static str = "shorten-error.html";
const RESOLVE_ERROR_PAGE: &'static str = "resolve-error.html";
const NOT_FOUND_PAGE: &'static str = "404.html";
const ALL_PAGES: &[&'static str] = &[
    INDEX_PAGE,
    SHORTEN_SUCCESS_PAGE,
    SHORTEN_ERROR_PAGE,
    RESOLVE_ERROR_PAGE,
    NOT_FOUND_PAGE,
];

fn read_resource_from_disk(path: &str) -> String {
    let mut resource = String::new();
    info!("Reading resource {} into memory", path);
    let mut file = File::open(&path[..]).expect(&format!("Error opening file {}", path));
    file.read_to_string(&mut resource).expect(&format!(
        "Error reading {} from disk",
        path
    ));
    resource
}

struct PageManager {
    pages: Handlebars,
}

impl PageManager {
    fn new(page_names: &[&'static str]) -> PageManager {
        let mut pages = Handlebars::new();
        for page_name in page_names {
            let page = read_resource_from_disk(&format!("www/{}", page_name));
            pages.register_template_string(page_name, page).unwrap();
        }
        PageManager { pages }
    }

    fn get(&self, name: &'static str) -> String {
        self.render(name, ())
    }

    fn render<T: Serialize>(&self, name: &str, values: T) -> String {
        self.pages.render(name, &values).unwrap()
    }
}

struct ShortenResult {
    short_url: String,
    already_existed: bool,
}

struct ResolveResult {
    short_url: String,
    long_url: String,
}

fn parse_url_from_form(form_chunk: Chunk) -> FutureResult<String, hyper::Error> {
    let mut form = url::form_urlencoded::parse(form_chunk.as_ref())
        .into_owned()
        .collect::<HashMap<String, String>>();
    info!("Received request with form data: {:?}", form);
    if let Some(long_url) = form.remove("url") {
        info!("Found URL in form: {}", long_url);
        futures::future::ok(long_url)
    } else {
        error!("Received POST request at /shorten but with no URL");
        futures::future::err(hyper::Error::from(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Missing form field 'url'",
        )))
    }
}

fn shorten_url(
    long_url: String,
    db_connection: &PgConnection,
) -> FutureResult<ShortenResult, hyper::Error> {
    use schema::urls;

    debug!("Querying DB to see if long URL already exists");
    let existing_url: QueryResult<models::Url> = urls::table
        .filter(urls::long_url.eq(long_url.clone()))
        .get_result(db_connection);
    let maybe_hash = match existing_url {
        Ok(url) => Some(url.hash),
        Err(diesel::result::Error::NotFound) => None,
        Err(error) => {
            return futures::future::err(hyper::Error::from(
                io::Error::new(io::ErrorKind::Other, error.description()),
            ));
        }
    };

    let already_existed = maybe_hash.is_some();
    debug!("URL {} was already present: {}", long_url, already_existed);
    let maybe_hash = maybe_hash.or_else(|| {
        for attempt in 1..NUMBER_OF_HASH_ATTEMPTS + 1 {
            debug!("Inserting URL {} into DB", long_url);
            let result = diesel::insert_into(urls::table)
                .values(&models::NewUrl::new(&long_url))
                .returning(urls::hash)
                .get_result(db_connection);
            match result {
                Ok(hash) => return Some(hash),
                Err(_) => {
                    warn!("Attempt #{} to find hash for {} failed", attempt, long_url);
                }
            }

        }
        None
    });

    match maybe_hash {
        Some(hash) => {
            let short_url = format!("{}/{}", SHORT_DOMAIN, hash);
            info!("Short URL for {} is {}", long_url, short_url);
            futures::future::ok(ShortenResult {
                short_url,
                already_existed,
            })
        }
        None => {
            futures::future::err(hyper::Error::from(io::Error::new(
                io::ErrorKind::Other,
                "Could not find hash for URL",
            )))
        }
    }
}

fn resolve_url(
    hash: &str,
    db_connection: &PgConnection,
) -> FutureResult<ResolveResult, hyper::Error> {
    use schema::urls;

    debug!("Querying DB to resolve hash '{}'", hash);
    let query_result = urls::table
        .select(urls::long_url)
        .filter(urls::hash.eq(hash))
        .get_result(db_connection);

    let short_url = format!("{}/{}", SHORT_DOMAIN, hash);
    match query_result {
        Ok(long_url) => futures::future::ok(ResolveResult {
            short_url: short_url,
            long_url: long_url,
        }),
        Err(_) => {
            let error = format!("Could not resolve {}", short_url);
            error!("{}", error);
            futures::future::err(hyper::Error::from(
                io::Error::new(io::ErrorKind::InvalidInput, error),
            ))
        }
    }
}

fn make_resolve_response(
    page_manager: &PageManager,
    result: Result<ResolveResult, hyper::Error>,
) -> Result<hyper::Response, hyper::Error> {
    let response = match result {
        Ok(resolve_result) => {
            make_redirect_response(&resolve_result.short_url[..], &resolve_result.long_url[..])
        }
        Err(error) => {
            let mut values = HashMap::new();
            values.insert("why", error.description());
            let page = page_manager.render(RESOLVE_ERROR_PAGE, values);
            Response::new()
                .with_header(ContentLength(page.len() as u64))
                .with_body(page)
        }
    };
    Ok(response)
}

fn make_shorten_response(
    maybe_result: Result<ShortenResult, hyper::Error>,
) -> FutureResult<hyper::Response, hyper::Error> {
    let (status, payload) = match maybe_result {
        Ok(result) => {
            let payload = format!(
                r#"{{
                    "shortUrl": "{}",
                    "alreadyExisted": {}
                }}"#,
                result.short_url,
                result.already_existed
            );
            (StatusCode::Ok, payload)
        }
        Err(error) => {
            let payload = format!(r#"{{"error": "{}"}}"#, error.description());
            (StatusCode::InternalServerError, payload)
        }
    };
    let response = Response::new()
        .with_status(status)
        .with_header(ContentLength(payload.len() as u64))
        .with_header(ContentType::json())
        .with_body(payload);
    debug!("{:?}", response);
    futures::future::ok(response)
}

fn make_redirect_response(source_url: &str, target_url: &str) -> hyper::Response {
    info!("Redirecting {} to {}", source_url, target_url);
    Response::new()
        .with_status(StatusCode::PermanentRedirect)
        .with_header(Location::new(target_url.to_string()))
}

fn is_valid_hash(hash: &str) -> bool {
    hash.chars().all(char::is_alphanumeric)
}

struct UrlShortener {
    thread_pool: CpuPool,
    page_manager: Arc<PageManager>,
    db_pool: r2d2::Pool<ConnectionManager<PgConnection>>,
}

impl UrlShortener {
    fn new() -> UrlShortener {
        let db_url = env::var("DATABASE_URL").unwrap_or(String::from(DEFAULT_DB_URL));
        info!("Connecting to database {}", db_url);
        let db_manager = ConnectionManager::<PgConnection>::new(db_url);
        UrlShortener {
            thread_pool: CpuPool::new(4),
            page_manager: Arc::new(PageManager::new(ALL_PAGES)),
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
        // This copy is partly to work around the borrow checker.
        let method = request.method().clone();
        let path = String::from(request.path());
        match (method, &path[..]) {
            (Get, "/") => {
                Box::new(futures::future::ok(
                    make_redirect_response("/", LONG_DOMAIN),
                ))
            }
            (Get, "/shorten") => {
                let page = self.page_manager.get(INDEX_PAGE);
                let future = futures::future::ok(
                    Response::new()
                        .with_header(ContentLength(page.len() as u64))
                        .with_body(page),
                );
                Box::new(future)
            }
            (Post, "/shorten") => {
                let db_pool = self.db_pool.clone();
                let future = self.thread_pool.spawn_fn(move || {
                    request
                        .body()
                        .concat2()
                        .and_then(parse_url_from_form)
                        .and_then(move |long_url| {
                            shorten_url(long_url, db_pool.get().unwrap().deref())
                        })
                        .then(make_shorten_response)
                });
                Box::new(future)
            }
            (Get, _) if is_valid_hash(&path[1..]) => {
                let db_pool = self.db_pool.clone();
                let page_manager = self.page_manager.clone();
                let future = self.thread_pool.spawn_fn(move || {
                    resolve_url(&path[1..], db_pool.get().unwrap().deref())
                        .then(move |result| make_resolve_response(&page_manager, result))
                });
                Box::new(future)
            }
            (Get, _) if path.starts_with("/www/") => {
                // Serving static content should be done by NGINX or smth.
                // ENABLE WITH FLAG
                info!("Requesting resource {}", path);
                let resource = read_resource_from_disk(&path[1..]);
                let future = futures::future::ok(
                    Response::new()
                        .with_header(ContentLength(resource.len() as u64))
                        .with_body(resource),
                );
                Box::new(future)
            }
            (method, _) => {
                info!("{} request for unknown resource {}", method, path);
                let page = self.page_manager.get(NOT_FOUND_PAGE);
                Box::new(futures::future::ok(
                    Response::new()
                        .with_status(StatusCode::NotFound)
                        .with_header(ContentLength(page.len() as u64))
                        .with_body(page),
                ))
            }
        }
    }
}

fn main() {
    pretty_env_logger::init().unwrap();
    let addr = "127.0.0.1:3000".parse().unwrap();
    let server = Http::new().bind(&addr, || Ok(UrlShortener::new())).unwrap();
    info!("Starting UrlShortener service @ http://{}", addr);
    server.run().unwrap();
}
