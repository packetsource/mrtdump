
use crate::*;

#[derive(Serialize, Deserialize, Debug)]
pub struct CacheEntry {
    pub local_filename: String,
    pub length: usize,
    pub mtime: Option<SystemTime>
}
pub struct Cache {
    root: String,   // root of the cache, eg. .mrt-cache
    sub: String,    // session based prefix for cache entries

    // Url -> (filename, size, mtime)
    data: HashMap<String, CacheEntry>,
}

const INDEX_FILE: &str = "index.dat";

impl Cache {
    pub fn load(root: &str) -> Cache {

        let sub= Uuid::new_v4().to_string();

        let working_dir = format!("{}/{}", root, sub);

        if ! ( Path::new(&working_dir).exists() && Path::new(&working_dir).is_dir() ) {
            fs::create_dir_all(&working_dir)
                .expect(&format!("create cache sub-directory: {}", &working_dir));
        }
        let index_file = format!("{}/{}", root, INDEX_FILE);
        let cache_data = match fs::OpenOptions::new()
            .read(true)
            .open(&index_file) {
            Ok(mut file) => {
                let mut data = String::new();
                file.read_to_string(&mut data)
                    .expect(&format!("reading index file {}", &index_file));
                serde_json::from_str(&mut data).expect(&format!("cache index read error: {}", &index_file))
            },
            Err(_e) => {
                HashMap::new()
            }
        };

        Cache {
            root: root.to_string(),
            sub,
            data: cache_data
        }
    }

    pub fn save(&self) -> io::Result<()> {
        let index_file = format!("{}/{}", self.root, INDEX_FILE);
        std::fs::write(&index_file, serde_json::to_string(&self.data)?)
    }

    pub fn get_url(&mut self, url: &str) -> Result<Vec<u8>> {

        let cache_entry = self.data.entry(String::from(url))
            .or_insert(CacheEntry {
                local_filename: format!("{}/{}", self.sub, Uuid::new_v4()),
                length: 0,
                mtime: None,    // None means not a valid cache entry
            });

        // If we have an existing file with a modified time, then add
        // a header to the outgoing request telling the server we only
        // want modified versions please
        let client = Client::new();
        let req = {
            let req = client.get(url);
            if let Some(mtime) = cache_entry.mtime {
                let dt = OffsetDateTime::from(mtime);
                req.header(reqwest::header::IF_MODIFIED_SINCE, dt.format(&Rfc2822)?)
            } else {
                req
            }
        };

        // Send the request
        let mut resp = req.send()?;

        let filename = &format!("{}/{}",
                                self.root,
                                &cache_entry.local_filename);

        let data = match resp.status() {
            reqwest::StatusCode::OK => {
                if GETOPT.verbose {
                    eprintln!("Downloading {} to local file {}", &url, filename);
                }
                let modified_time: &str = match resp.headers().get("last-modified") {
                    Some(x) => {
                        x.to_str()?
                    },
                    None => {
                        return Err(anyhow!("Couldn't find Last-Modified header found in server response!\n{:?}", resp.headers()))
                    }
                };

                let modified_time = OffsetDateTime::parse(&modified_time, &Rfc2822)?;
                let mtime = SystemTime::from(modified_time);
                let mut data = Vec::<u8>::new();
                resp.read_to_end(&mut data)?;
                {
                    let mut file = File::options()
                        .write(true)
                        .create(true)
                        .truncate(true)
                        .open(&filename)?;
                    file.write(&data)?;
                    file.set_modified(mtime)?;
                }
                cache_entry.length = data.len();
                cache_entry.mtime = Some(mtime);    // Some(mtime) means valid entry
                data
            }

            reqwest::StatusCode::NOT_MODIFIED => {
                if GETOPT.verbose {
                    eprintln!("Using cached version of {} at {}",
                             &url,
                             &filename);
                }
                std::fs::read(&filename)?
            }

            s => {
                //eprintln!("Unexpected HTTP status code {} for URL {}", &s, &url);
                return Err(anyhow::anyhow!("Unexpected HTTP status code {} for {}", &s, &url));
            }
        };

        Ok(data)
    }
}
