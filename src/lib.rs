#![crate_type = "dylib"]

extern crate kitten;
extern crate irc;
extern crate hyper;
extern crate regex;
extern crate mime;
extern crate rand;
extern crate rusqlite;
extern crate tempdir;
extern crate md5;
extern crate time;

use std::io::Result as IoResult;
use std::fs::File;
use std::env::temp_dir;
use std::path::Path;

use kitten::{Plugin, KittenServer};
use regex::Regex;
use hyper::{Client, Url};
use hyper::header::ContentType;
use rand::{Rng, thread_rng};
use rusqlite::SqliteConnection as DbConnection;
use mime::Mime;
use mime::TopLevel::{Image, Video};
use mime::SubLevel::{Jpeg, Png, Gif, Ext};


struct Scraper {
    db: DbConnection,
}

fn random_string(length: usize) -> String {
    thread_rng().gen_ascii_chars().take(length).collect()
}

fn calc_md5(file: &mut File) -> md5::Digest {
    use std::io::Read;
    // 4 mbytes = 4194304
    let mut contents = Vec::with_capacity(4_194_304);
    println!("read: {:?}", file.read_to_end(&mut contents));
    md5::compute(&contents)
}

fn md5_hexdigest(digest: &md5::Digest) -> String {
    format!("{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7],
            digest[8], digest[9], digest[10], digest[11], digest[12], digest[13], digest[14], digest[15])
}

fn magic_file(path: &Path) -> Option<Mime> {
    use std::process::Command;
    Command::new("file")
            .arg("-b")
            .arg("--mime-type")
            .arg(path.as_os_str())
            .output()
            .ok().and_then(|out| (&String::from_utf8_lossy(&out.stdout).trim()).parse().ok())
}

fn create_thumbnail(path: &Path, name: &str, ext: &str) {
    use std::process::Command;
    let mut command = Command::new("convert");
            command
            .arg(format!("{}{}.{}[0]", path.display(), name, ext))
            .arg("-resize").arg("100x200")
            .arg(format!("{}{}_thumb.{}", path.display(), name, ext));//.output().unwrap();
    println!("{:?}", command);
    println!("{:?}", command.status());
    let out = command.output().unwrap();
    println!("{:?}", String::from_utf8_lossy(&out.stdout));
}

impl Plugin for Scraper {
    fn process_privmsg(&self,
                       _: &KittenServer,
                       source: &str,
                       target: &str,
                       message: &str) -> Option<String>
    {

        let regex = r"https?://[^\s/$.?#].[^\s]*";
        let url_regex = Regex::new(regex).unwrap();
        let time_now = time::now();
        let timestamp_string = format!("{}", time_now.strftime("%F %T").unwrap());
        let scrape_path = Path::new("./scrape_img/");
        for url_string in url_regex.captures_iter(message) {
            let url_string = url_string.at(0).unwrap();
            if let Ok(url) = Url::parse(url_string) {
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
                    // First check if the server even pretends to give us an image/webm
                    match content_type {
                        Some(ContentType(Mime(Image, Jpeg, _))) |
                        Some(ContentType(Mime(Image, Png, _))) |
                        Some(ContentType(Mime(Image, Gif, _))) => {
                            if let Ok(mut file) = temp_file {
                                // Read the body into the temp file
                                std::io::copy(&mut response, &mut file);
                                let mime = magic_file(&temp_file_path);
                                match mime {
                                    Some(Mime(Image, Jpeg, _)) | Some(Mime(Image, Png, _)) | Some(Mime(Image, Gif, _)) => {
                                        let hexdigest = md5_hexdigest(&calc_md5(&mut File::open(temp_file_path.clone()).unwrap()));
                                        let mut stmt = self.db.prepare("SELECT locnam FROM scrape WHERE hash = $1").unwrap();
                                        let mut local_rows = stmt.query_map(&[&hexdigest], |row| {
                                            row.get(0)
                                        }).unwrap().collect::<Vec<_>>();
                                        let query = "INSERT INTO scrape (timestamp, nick, msg, url, chan, locnam, hash) VALUES ($1, $2, $3, $4, $5, $6, $7)";
                                        if local_rows.len() > 0 {
                                            // it's a local image
                                            let locnam: String = local_rows.swap_remove(0).unwrap();
                                            self.db.execute(query, &[&timestamp_string, &source, &message, &url_string, &target, &locnam, &hexdigest]);
                                        } else {
                                            // it's a new image
                                            let file_ext = if let Some(Mime(_, Jpeg, _)) = mime { "jpg" } else if let Some(Mime(_, Png, _)) = mime { "png" } else { "gif" };
                                            let mut idrows = self.db.prepare("SELECT MAX(id) FROM scrape").unwrap().query_map(&[], |row| row.get(0)).unwrap().collect::<Vec<_>>();
                                            let max_id: i32 = idrows.swap_remove(0).unwrap();
                                            let new_id = max_id + 1;

                                            let locnam = format!("{}.{}", new_id, file_ext);
                                            let mut final_path = scrape_path.to_path_buf();
                                            final_path.push(locnam.clone());
                                            std::fs::copy(temp_file_path, final_path);
                                            create_thumbnail(scrape_path, &new_id.to_string(), &file_ext);
                                            self.db.execute(query, &[&timestamp_string, &source, &message, &url_string, &target, &locnam, &hexdigest]);
                                        }
                                    },
                                    _ => {
                                    }
                                }

                                /*
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
                                */
                            }
                        },
                        /*
                        Some(ct) if *ct == gif_type => {

                        }
                        Some(ct) if *ct == webm_type => {

                        }
                        */
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
    // let cookie = Cookie::open(magic::flags::MIME_TYPE);
    let db = DbConnection::open(&"scrape.db");
    if let Ok(db) = db {
        Ok(Box::new(Scraper { db: db }))
    }
    else { Err("Unable to open db".to_owned()) }
    /*
    match (cookie, db) {
        (Ok(cookie), Ok(db)) => {
            match cookie.load(&vec![Path::new("/usr/share/file/magic.mgc")]) {
                Ok(()) => Ok(Box::new(Scraper { db: db, cookie: cookie })),
                // TODO: fall back to internal defs
                Err(e) => Err(format!("Unable to load magic definitions file: {}", e))
            }
        },
        _ => {
            Err("NO :(".to_owned())
        }
    }
    */
}
