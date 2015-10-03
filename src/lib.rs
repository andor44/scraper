#![crate_type = "dylib"]

extern crate kitten;
extern crate irc;
extern crate hyper;
extern crate regex;
extern crate mime;
extern crate rand;
extern crate rusqlite;
extern crate tempdir;
extern crate magic;

use std::io::Result as IoResult;
use std::fs::File;
use std::env::temp_dir;
use std::path::Path;

use kitten::{Plugin, KittenServer};
use regex::Regex;
use hyper::{Client, Url};
use hyper::header::ContentType;
use mime::Mime;
use rand::{Rng, thread_rng};
use rusqlite::SqliteConnection as DbConnection;
use magic::Cookie;


struct Scraper {
    db: DbConnection,
    cookie: Cookie,
}

fn random_string(length: usize) -> String {
    thread_rng().gen_ascii_chars().take(length).collect()
}

fn upsert(image_path: i8, db: &DbConnection) -> bool {
    false
}

// Some predefined MIMEs
const jpg_type: ContentType = ContentType("image/jpeg".parse().unwrap());
const png_type: ContentType = ContentType("image/png".parse().unwrap());
const gif_type: ContentType = ContentType("image/gif".parse().unwrap());
const webm_type: ContentType = ContentType("video/webm".parse().unwrap());

impl Plugin for Scraper {
    fn process_privmsg(&self,
                       _: &KittenServer,
                       source: &str,
                       target: &str,
                       message: &str) -> Option<String>
    {

        let regex = r"https?://[^\s/$.?#].[^\s]*";
        let url_regex = Regex::new(regex).unwrap();
        for url in url_regex.captures_iter(message) {
            if let Ok(url) = Url::parse(url.at(0).unwrap()) {
                println!("Found URL: {}", url.serialize());
                let client = Client::new();
                let result = client.get(url.clone()).send();
                if let Ok(mut response) = result {
                    // Have to clone because don't want to hold a ref to response
                    let content_type = response.headers.get::<ContentType>().map(|x| (*x).clone());
                    println!("Content type is: {:?}", content_type);
                    // TODO: return here
                    let temp_dir = tempdir::TempDir::new(&format!("scraper{}", random_string(10))).unwrap();
                    let temp_file_path = temp_dir.path().with_file_name(random_string(10));
                    let temp_file = File::create(temp_file_path.clone());
                    match content_type.as_ref() {
                        Some(ct) if *ct == jpg_type || *ct == png_type => {
                            if let Ok(mut file) = temp_file {
                                std::io::copy(&mut response, &mut file);
                                if let Ok(mime_str) = self.cookie.file(&temp_file_path) {
                                    // XXX: assuming libmagic never shits the bed, bad idea though
                                    let real_type = ContentType(mime_str.parse().unwrap());
                                    if real_type == jpg_type || real_type == png_type {
                                        // Hooray, good type!
                                        // println!("GOOD TYPE! {}", mime_str);
                                        // Calc MD5
                                        let _ = "INSERT INTO scrape (timestamp, nick, msg, url, chan, locnam, hash) VALUES ($1, $2, $3, $4, $5, $6, $7)";
                                        // self.db.execute(query, &[&TIMESTAMP, &source, &message, &url, &target, &LOCNAM, &HASH]); 
                                    }
                                }
                            }
                        },
                        Some(ct) if *ct == gif_type => {

                        }
                        Some(ct) if *ct == webm_type => {

                        }
                        _ => ()
                    }
                }
            }
        }
        None
    }
}

#[no_mangle]
pub extern fn init_plugin() -> Result<Box<Plugin>, String> {
    let cookie = Cookie::open(magic::flags::MIME_TYPE);
    let db = DbConnection::open(&"scraper.db");
    match (cookie, db) {
        (Ok(cookie), Ok(db)) => {
            if let Ok(()) = cookie.load(&vec![Path::new("/usr/share/file/misc/magic.mgc")][..]) {
                Ok(Box::new(Scraper { db: db, cookie: cookie }))
            } else {
                // TODO: fall back to internal defs
                Err("Unable to load magic definitions file".to_string())
            }
        },
        _ => {
            Err(format!("DB status: {} libmagic status: {}", cookie.is_ok(), db.is_ok()))
        }
    }
}
