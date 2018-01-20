use std::env;
use std::fs::File;
use std::io::prelude::*;

use handlebars::Handlebars;
use serde::ser::Serialize;

const DEFAULT_WWW_DIR: &'static str = "/var/www/psag.cc/static/www";

pub struct ResourceManager {
    templates: Handlebars,
}

impl ResourceManager {
    pub fn new(template_names: &[&'static str], partials: &[&'static str]) -> ResourceManager {
        let mut templates = Handlebars::new();
        let www = env::var("WWW_DIR").unwrap_or(String::from(DEFAULT_WWW_DIR));
        for partial_name in partials {
            let path = format!("{}/{}.partial.html", www, partial_name);
            let template = ResourceManager::read_resource_from_disk(&path);
            templates.register_partial(partial_name, template).unwrap();
        }
        for template_name in template_names {
            let path = format!("{}/{}.html", www, template_name);
            let template = ResourceManager::read_resource_from_disk(&path);
            templates
                .register_template_string(template_name, template)
                .unwrap();
        }
        ResourceManager { templates }
    }

    pub fn get_template(&self, name: &'static str) -> String {
        self.render_template(name, ())
    }

    pub fn render_template<T: Serialize>(&self, name: &str, values: T) -> String {
        self.templates.render(name, &values).unwrap()
    }

    pub fn read_resource_from_disk(path: &str) -> String {
        let mut resource = String::new();
        info!("Reading resource {} into memory", path);
        let mut file = File::open(&path[..]).expect(&format!("Error opening file {}", path));
        file.read_to_string(&mut resource).expect(&format!(
            "Error reading {} from disk",
            path
        ));
        resource
    }
}
