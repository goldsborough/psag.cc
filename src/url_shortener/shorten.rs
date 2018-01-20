use std::io;
use std::error::Error;

use futures;
use futures::future::FutureResult;

use hyper;
use hyper::StatusCode;
use hyper::server::Response;
use hyper::header::{ContentLength, ContentType};

use diesel;
use diesel::prelude::*;
use diesel::pg::PgConnection;

use db;

const NUMBER_OF_HASH_ATTEMPTS: usize = 100;

pub struct ShortenResult {
    hash: String,
    already_existed: bool,
}

fn query_db_for_hash(
    long_url: &String,
    db_connection: &PgConnection,
) -> Result<Option<String>, io::Error> {
    use db::schema::urls;
    debug!("Querying DB to see if long URL already exists");
    let existing_url: QueryResult<db::models::Url> = urls::table
        .filter(urls::long_url.eq(long_url.clone()))
        .get_result(db_connection);
    match existing_url {
        Ok(url) => Ok(Some(url.hash)),
        Err(diesel::result::Error::NotFound) => Ok(None),
        Err(error) => Err(io::Error::new(io::ErrorKind::Other, error.description())),
    }
}

fn generate_and_insert_hash_into_db(
    long_url: &String,
    db_connection: &PgConnection,
) -> Option<String> {
    use db::schema::urls;
    for attempt in 1..NUMBER_OF_HASH_ATTEMPTS + 1 {
        debug!("Inserting URL {} into DB", long_url);
        let result = diesel::insert_into(urls::table)
            .values(&db::models::NewUrl::new(&long_url))
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
}

pub fn get_hash(
    long_url: String,
    db_connection: &PgConnection,
) -> FutureResult<ShortenResult, hyper::Error> {
    let maybe_hash = match query_db_for_hash(&long_url, db_connection) {
        Ok(maybe_hash) => maybe_hash,
        Err(error) => return futures::future::err(hyper::Error::from(error)),
    };

    let already_existed = maybe_hash.is_some();
    debug!("URL {} was already present: {}", long_url, already_existed);

    let maybe_hash = maybe_hash.or_else(|| {
        generate_and_insert_hash_into_db(&long_url, db_connection)
    });

    match maybe_hash {
        Some(hash) => {
            info!("Hash for {} is {}", long_url, hash);
            futures::future::ok(ShortenResult {
                hash,
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

pub fn make_response(
    short_domain: &str,
    maybe_result: Result<ShortenResult, hyper::Error>,
) -> FutureResult<hyper::Response, hyper::Error> {
    let (status, payload) = match maybe_result {
        Ok(result) => {
            let short_url = format!("{}/{}", short_domain, result.hash);
            let payload = format!(
                r#"{{"shortUrl": "{}", "alreadyExisted": {}}}"#,
                short_url,
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
    info!("{:?}", response);
    futures::future::ok(response)
}
