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

mod service;
mod url_shortener;
mod db;

fn main() {
    pretty_env_logger::init().unwrap();
    let addr = "127.0.0.1:3000".parse().unwrap();
    let server = hyper::server::Http::new()
        .bind(&addr, || Ok(service::UrlShortener::new()))
        .unwrap();
    info!("Starting UrlShortener service @ http://{}", addr);
    server.run().unwrap();
}
