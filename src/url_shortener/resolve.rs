use std::io;
use std::collections::HashMap;
use std::error::Error;

use futures;
use futures::future::FutureResult;

use hyper;
use hyper::StatusCode;
use hyper::server::Response;
use hyper::header::{ContentLength, Location};

use diesel;
use diesel::prelude::*;
use diesel::pg::PgConnection;

use url_shortener::resource_manager::ResourceManager;

fn increment_access_count(hash: &str, db_connection: &PgConnection) {
    use db::schema::urls;
    let access_count: i32 = diesel::update(urls::table)
        .filter(urls::hash.eq(hash))
        .set(urls::access_count.eq(urls::access_count + 1))
        .returning(urls::access_count)
        .get_result(db_connection)
        .unwrap();
    debug!("Incremented access count of {} to {}", hash, access_count);
}

pub fn resolve_url(hash: &str, db_connection: &PgConnection) -> FutureResult<String, hyper::Error> {
    use db::schema::urls;

    debug!("Querying DB to resolve hash '{}'", hash);
    let query_result = urls::table
        .select(urls::long_url)
        .filter(urls::hash.eq(hash))
        .get_result(db_connection);

    match query_result {
        Ok(long_url) => {
            increment_access_count(hash, db_connection);
            futures::future::ok(long_url)
        }
        Err(_) => {
            let error = format!("Could not resolve {}", hash);
            error!("{}", error);
            futures::future::err(hyper::Error::from(
                io::Error::new(io::ErrorKind::InvalidInput, error),
            ))
        }
    }
}

pub fn make_response(
    resource_manager: &ResourceManager,
    result: Result<String, hyper::Error>,
) -> Result<hyper::Response, hyper::Error> {
    let response = match result {
        Ok(long_url) => {
            Response::new()
                .with_status(StatusCode::PermanentRedirect)
                .with_header(Location::new(long_url))
        }
        Err(error) => {
            let mut values = HashMap::new();
            values.insert("why", error.description());
            let page = resource_manager.render_page("resolve-error", values);
            Response::new()
                .with_header(ContentLength(page.len() as u64))
                .with_body(page)
        }
    };
    Ok(response)
}
