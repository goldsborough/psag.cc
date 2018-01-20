extern crate chrono;
extern crate futures_cpupool;
extern crate futures;
extern crate handlebars;
extern crate hyper;
extern crate num_cpus;
extern crate postgres;
extern crate pretty_env_logger;
extern crate r2d2_diesel;
extern crate r2d2;
extern crate rand;
extern crate serde;
extern crate url;

#[macro_use]
extern crate log;

#[macro_use]
extern crate diesel;

use std::env;

mod service;
mod url_shortener;
mod db;

fn main() {
    pretty_env_logger::init().unwrap();
    let address = env::var("HOST_PORT")
        .unwrap_or(String::from("0.0.0.0:80"))
        .parse()
        .unwrap();
    let server = hyper::server::Http::new()
        .bind(&address, || Ok(service::UrlShortener::new()))
        .unwrap();
    info!("Starting psag.cc service @ http://{}", address);
    server.run().unwrap();
}
