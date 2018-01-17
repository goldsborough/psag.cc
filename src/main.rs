extern crate futures;
extern crate futures_cpupool;
extern crate hyper;
extern crate url;
extern crate postgres;

extern crate pretty_env_logger;
#[macro_use]
extern crate log;

#[macro_use]
extern crate diesel;

use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::error::Error;
use std::env;

use futures::future::{Future, FutureResult};
use futures::Stream;
use futures_cpupool::CpuPool;

use hyper::{Chunk, StatusCode};
use hyper::server::{Http, Request, Response, Service};
use hyper::Method::{Get, Post};
use hyper::header::{ContentLength, Location};

use diesel::prelude::*;
use diesel::pg::PgConnection;
use postgres::{Connection, TlsMode};

use url::Url;

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
    pages: HashMap<&'static str, String>,
}

impl PageManager {
    fn new(page_names: &[&'static str]) -> PageManager {
        let mut pages: HashMap<&'static str, String> = HashMap::with_capacity(2);
        page_names.iter().for_each(|page| {
            pages.insert(page, PageManager::read_page_from_disk(&page));
        });
        PageManager { pages }
    }

    fn get(&self, name: &'static str) -> Option<String> {
        self.pages.get(name).map(|page| page.clone())
    }

    fn render(&self, name: &'static str, values: Vec<&str>) -> Option<String> {
        let template = self.get(name)?;
        // render ...
        Some(template)
    }

    fn read_page_from_disk(page_name: &'static str) -> String {
        let mut page = String::new();
        let path = format!("www/{}", page_name);
        info!("Reading page {} into memory", path);
        let mut file = File::open(path).unwrap();
        file.read_to_string(&mut page).unwrap();
        page
    }
}

struct ShortenResult {
    short_url: String,
    already_existed: bool,
}

struct ExpandResult {
    id_hash: String,
    short_url: String,
    long_url: String,
}

fn parse_form(form_chunk: Chunk) -> FutureResult<String, hyper::Error> {
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

fn maybe_insert(url: String) -> (u64, bool) {
    (1u64, true)
}

fn make_id_hash(id: u64) -> String {
    String::from("adf")
}

fn get_long_url_from_db(id_hash: &String) -> Option<String> {
    Some(String::from("a"))
}

fn shorten_url(long_url: String) -> Result<ShortenResult, hyper::Error> {
    let (id, already_existed) = maybe_insert(long_url);
    let id_hash = make_id_hash(id);
    let short_url = format!("{}/{}", SHORT_DOMAIN, id_hash);
    Ok(ShortenResult {
        short_url,
        already_existed,
    })
}

fn expand_url(short_url: String, id_hash: String) -> FutureResult<ExpandResult, hyper::Error> {
    match get_long_url_from_db(&id_hash) {
        Some(long_url) => futures::future::ok(ExpandResult {
            short_url: short_url,
            id_hash: id_hash,
            long_url: long_url,
        }),
        None => futures::future::err(hyper::Error::from(io::Error::new(
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
            let page = page_manager
                .render(EXPAND_ERROR_PAGE, vec![error.description()])
                .unwrap();
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
            page = page_manager
                .render(SHORTEN_SUCCESS_PAGE, vec![&response.short_url[..]])
                .unwrap();
        }
        Err(error) => {
            page = page_manager
                .render(SHORTEN_ERROR_PAGE, vec![error.description()])
                .unwrap();
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

fn parse_id_hash(short_url: &String) -> FutureResult<String, hyper::Error> {
    // We've already validated the url at this point.
    futures::future::ok(String::from(Url::parse(short_url).unwrap().path()))
}

fn is_valid_short_url(short_url: &String) -> bool {
    Url::parse(short_url)
        .map(|url| url.path().chars().all(char::is_alphanumeric))
        .is_ok()
}

struct UrlShortener {
    thread_pool: CpuPool,
    page_manager: Arc<RwLock<PageManager>>,
}

impl UrlShortener {
    fn new() -> UrlShortener {
        UrlShortener {
            thread_pool: CpuPool::new(4),
            page_manager: Arc::new(RwLock::new(PageManager::new(ALL_PAGES))),
        }
    }

    fn get_page(&self, name: &'static str) -> Option<String> {
        self.page_manager.read().unwrap().get(name)
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
                let page = self.get_page(INDEX_PAGE).unwrap();
                Box::new(futures::future::ok(
                    Response::new()
                        .with_header(ContentLength(page.len() as u64))
                        .with_body(page),
                ))
            }
            (&Post, "/shorten") => {
                let page_manager = self.page_manager.clone();
                let future = self.thread_pool.spawn_fn(move || {
                    request
                        .body()
                        .concat2()
                        .and_then(parse_form)
                        .and_then(shorten_url)
                        .then(move |result| {
                            let page_manager = page_manager.read().unwrap();
                            make_shorten_response(&page_manager, result)
                        })
                });
                Box::new(future)
            }
            (&Get, _) if is_valid_short_url(&path) => {
                let page_manager = self.page_manager.clone();
                let future = self.thread_pool.spawn_fn(move || {
                    parse_id_hash(&path)
                        .and_then(|id_hash| expand_url(path, id_hash))
                        .then(move |result| {
                            let page_manager = page_manager.read().unwrap();
                            make_expand_response(&page_manager, result)
                        })
                });
                Box::new(future)
            }
            _ => {
                let page = self.get_page(NOT_FOUND_PAGE).unwrap();
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

pub mod schema;
pub mod models;

#[derive(Queryable)]
struct Url {
    id: i64,
    long_url: String,
    creation_time: String,
    access_count: i32
}

fn main() {
}

// id integer primary key,
// long_url varchar(256),
// creation_time timestamp,
// read_count integer,

// fn main() {
//     pretty_env_logger::init().unwrap();
//     let addr = "127.0.0.1:3000".parse().unwrap();
//     let server = Http::new().bind(&addr, || Ok(UrlShortener::new())).unwrap();
//     info!("Starting UrlShortener service @ {}", addr);
//     server.run().unwrap();
// }
