use crate::*;

pub fn get_url_if_not_modified(url: &str) -> Result<Vec<u8>> {

    let base_pathname = {
        let url = Url::parse(url)?;
        let filename = url.path_segments().unwrap().last().unwrap();
        filename.to_string()
    };
    dbg!(&base_pathname);

    let mtime = match fs::metadata(&base_pathname) {
        Ok(metadata) => Some(metadata.modified()?),
        _ => None,
    };

    // If we have an existing file with a modified time, then add
    // a header to the outgoing request telling the server we only
    // want modified versions please
    let client = Client::new();
    let req = {
        let req = client.get(url);
        if let Some(mtime) = mtime {
            let modified_time = OffsetDateTime::from(mtime);
            req.header(reqwest::header::IF_MODIFIED_SINCE, modified_time.format(&Rfc2822)?)
        } else {
            req
        }
    };

    // Send the request
    let mut resp = req.send()?;

    let data = match resp.status() {
        reqwest::StatusCode::OK =>  {
            println!("Downloading {} to local file {}", &url, &base_pathname);
            let modified_time = resp.headers().get("last-modified").unwrap().to_str()?;
            let modified_time = OffsetDateTime::parse(&modified_time, &Rfc2822)?;
            let mut data = Vec::<u8>::new();
            resp.read_to_end(&mut data)?;
            {
                let mut file = File::options()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&base_pathname)?;
                file.write(&data)?;
                file.set_modified(SystemTime::from(modified_time))?;
            }
            data
        },

        reqwest::StatusCode::NOT_MODIFIED => {
            println!("Using cached version at {}", &base_pathname);
            std::fs::read(&base_pathname)?
        },

        s => {
            eprintln!("Unexpected HTTP status code {}", &s);
            return Err(anyhow::anyhow!("Unexpected HTTP status code {}", &s));
        }
    };

    Ok(data)
}
