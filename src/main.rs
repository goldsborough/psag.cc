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
use hyper::header::{ContentLength, Location};

use diesel::prelude::*;
use diesel::pg::PgConnection;
use r2d2_diesel::ConnectionManager;

use url::Url;
use handlebars::Handlebars;
use serde::ser::Serialize;

pub mod schema;
pub mod models;

const DEFAULT_DB_URL: &'static str = "postgresql://goldsborough@localhost:5432";
const NUMBER_OF_HASH_ATTEMPTS: usize = 100;

const LONG_DOMAIN: &'static str = "http://www.goldsborough.me";
const SHORT_DOMAIN: &'static str = "http://www.psag.cc";

const INDEX_PAGE: &'static str = "index.html";
const SHORTEN_SUCCESS_PAGE: &'static str = "shorten-success.html";
const SHORTEN_ERROR_PAGE: &'static str = "shorten-error.html";
const EXPAND_ERROR_PAGE: &'static str = "expand-error.html";
const NOT_FOUND_PAGE: &'static str = "404.html";
const ALL_PAGES: &[&'static str] = &[
    INDEX_PAGE,
    SHORTEN_SUCCESS_PAGE,
    SHORTEN_ERROR_PAGE,
    EXPAND_ERROR_PAGE,
    NOT_FOUND_PAGE,
];

struct PageManager {
    pages: Handlebars,
}

impl PageManager {
    fn new(page_names: &[&'static str]) -> PageManager {
        let mut pages = Handlebars::new();
        for page_name in page_names {
            let page = PageManager::read_page_from_disk(&page_name);
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

    fn read_page_from_disk(page_name: &'static str) -> String {
        let mut page = String::new();
        let path = format!("www/{}", page_name);
        info!("Reading page {} into memory", path);
        let mut file = File::open(&path[..]).expect(&format!("Error opening {}", path));
        file.read_to_string(&mut page).expect(&format!(
            "Error reading {} from disk",
            path
        ));
        page
    }
}

struct ShortenResult {
    short_url: String,
    already_existed: bool,
}

struct ExpandResult {
    short_url: String,
    long_url: String,
}

fn parse_url_from_form(form_chunk: Chunk) -> FutureResult<String, hyper::Error> {
    let form = url::form_urlencoded::parse(form_chunk.as_ref())
        .into_owned()
        .collect::<HashMap<String, String>>();
    if let Some(long_url) = form.get("url") {
        futures::future::ok(long_url.clone())
    } else {
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
    let maybe_hash = maybe_hash.or_else(move || {
        for attempt in 1..NUMBER_OF_HASH_ATTEMPTS + 1 {
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

fn expand_url(
    short_url: String,
    db_connection: &PgConnection,
) -> FutureResult<ExpandResult, hyper::Error> {
    use schema::urls;

    // Safe to unwrap here because we already validated the URL earlier.
    let hash = String::from(Url::parse(&short_url[..]).unwrap().path());
    let query_result = urls::table
        .select(urls::long_url)
        .filter(urls::hash.eq(hash))
        .get_result(db_connection);

    match query_result {
        Ok(long_url) => futures::future::ok(ExpandResult {
            short_url: short_url,
            long_url: long_url,
        }),
        Err(_) => futures::future::err(hyper::Error::from(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Missing form field 'url'",
        ))),
    }
}

fn make_expand_response(
    page_manager: &PageManager,
    result: Result<ExpandResult, hyper::Error>,
) -> Result<hyper::Response, hyper::Error> {
    match result {
        Ok(expand_result) => {
            Ok(make_redirect_response(
                &expand_result.short_url[..],
                &expand_result.long_url[..],
            ))
        }
        Err(error) => {
            // vec![error.description()]
            let page = page_manager.render(EXPAND_ERROR_PAGE, ());
            let response = Response::new()
                .with_header(ContentLength(page.len() as u64))
                .with_body(page);
            Ok(response)
        }
    }
}

fn make_shorten_response(
    page_manager: &PageManager,
    result: Result<ShortenResult, hyper::Error>,
) -> Result<hyper::Response, hyper::Error> {
    let page: String;
    match result {
        Ok(response) => {
            // vec![&response.short_url[..]]
            page = page_manager.render(SHORTEN_SUCCESS_PAGE, ());
        }
        Err(error) => {
            // vec![error.description()]
            page = page_manager.render(SHORTEN_ERROR_PAGE, ());
        }
    }
    let response = Response::new()
        .with_header(ContentLength(page.len() as u64))
        .with_body(page);
    Ok(response)
}

fn make_redirect_response(source_url: &str, target_url: &str) -> hyper::Response {
    info!("Redirecting {} to {}", source_url, target_url);
    Response::new()
        .with_status(StatusCode::PermanentRedirect)
        .with_header(Location::new(String::from(target_url)))
}

fn is_valid_short_url(short_url: &String) -> bool {
    Url::parse(short_url)
        .map(|url| url.path().chars().all(char::is_alphanumeric))
        .is_ok()
}

struct UrlShortener {
    thread_pool: CpuPool,
    page_manager: Arc<PageManager>,
    db_pool: r2d2::Pool<ConnectionManager<PgConnection>>,
}

impl UrlShortener {
    fn new() -> UrlShortener {
        let db_url = env::var("DATABASE_URL").unwrap_or(String::from(DEFAULT_DB_URL));
        info!("Connecting to database @ {}", db_url);
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
        let path = String::from(request.path());
        match (request.method(), &path[..]) {
            (&Get, "/") => {
                Box::new(futures::future::ok(
                    make_redirect_response("/", LONG_DOMAIN),
                ))
            }
            (&Get, "/shorten") => {
                let page = self.page_manager.get(INDEX_PAGE);
                Box::new(futures::future::ok(
                    Response::new()
                        .with_header(ContentLength(page.len() as u64))
                        .with_body(page),
                ))
            }
            (&Post, "/shorten") => {
                let db_pool = self.db_pool.clone();
                let page_manager = self.page_manager.clone();
                let future = self.thread_pool.spawn_fn(move || {
                    request
                        .body()
                        .concat2()
                        .and_then(parse_url_from_form)
                        .and_then(move |long_url| {
                            shorten_url(long_url, db_pool.get().unwrap().deref())
                        })
                        .then(move |result| make_shorten_response(&page_manager, result))
                });
                Box::new(future)
            }
            (&Get, _) if is_valid_short_url(&path) => {
                let db_pool = self.db_pool.clone();
                let page_manager = self.page_manager.clone();
                let future = self.thread_pool.spawn_fn(move || {
                    expand_url(path, db_pool.get().unwrap().deref()).then(
                        move |result| make_expand_response(&page_manager, result),
                    )
                });
                Box::new(future)
            }
            _ => {
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
