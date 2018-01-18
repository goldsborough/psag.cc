use std::time::SystemTime;
use super::schema::urls;
use rand;

#[derive(Queryable, Debug)]
pub struct Url {
    pub hash: String,
    pub long_url: String,
    pub creation_time: SystemTime,
    pub access_count: i32,
}

#[derive(Insertable, Debug)]
#[table_name = "urls"]
pub struct NewUrl<'a> {
    pub hash: String,
    pub long_url: &'a String,
}

impl<'a> NewUrl<'a> {
    pub fn new(long_url: &'a String) -> Self {
        let hash = format!("{:08x}", rand::random::<u32>());
        NewUrl { hash, long_url }
    }
}
