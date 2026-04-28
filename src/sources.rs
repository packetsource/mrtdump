use crate::*;

// https://ris.ripe.net/docs/route-collectors/

pub fn expand_source(source: &str) -> Result<Vec<String>> {

    let result: Vec<String> = match source {
        "RIPE" => {
            let mut result = Vec::<String>::new();
            for i in 1..=25 {
                let s = expand_datetime(&format!("https://data.ris.ripe.net/rrc{:02}/YYYY.MM/bview.YYYYMMDD.0000.gz", i));
                //eprintln!("RIPE: {}", s);
                result.push(s);
            }
            result
        },
        "routeviews" | "routeviews.org" | "rv" | "RV" => {
            vec!["http://foo/", "foo"].iter().map(|x| x.to_string()).collect::<Vec<String>>()
        },
        _ => {
            return Err(anyhow!("Unknown source: {}", &source))
        }
    };

    Ok(result)
}

pub fn expand_datetime(s: &str) -> String {
    let datetime = OffsetDateTime::from(SystemTime::now()) - Duration::from_secs(86400);

    let s1 = datetime.format(&format_description::parse("[year][month][day]")
        .expect("preparing format [year][month][day]"))
        .expect("formatting date as [year][month][day]");
    let s2 = datetime.format(&format_description::parse("[year].[month]")
        .expect("preparing format [year].[month]"))
        .expect("formatting date as [year].[month]");

    String::from(s).replace("YYYYMMDD", &s1).replace("YYYY.MM", &s2)
}