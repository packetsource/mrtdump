use regex::Regex;
use crate::*;
#[derive(Debug)]
pub enum Filter {
    LPM(IpAddr),
    Prefix(Prefix),
    As(u32),
    AsPath(Vec<u32>),
    Community(Community),
    AsPathRegex(Regex),
    CommunityRegex(Regex),
    Other(String)
}

impl FromStr for Filter {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Filter, Self::Err> {
        match Prefix::from_str(s) {
            Ok(prefix) => return Ok(Filter::Prefix(prefix)),
            _ => {}
        }
        match IpAddr::from_str(s) {
            Ok(ipaddr) => return Ok(Filter::LPM(ipaddr)),
            _ => {}
        }

        if s.contains(',') {
            let parts: Result<Vec<u32>, _> = s.split(',').map(|p| p.trim().parse::<u32>()).collect();
            if let Ok(seq) = parts {
                return Ok(Filter::AsPath(seq));
            }
        }

        match u32::from_str(s) {
            Ok(asn) => return Ok(Filter::As(asn)),
            _ => {}
        }

        let community_pair = s.split(":").collect::<Vec<&str>>();
        if community_pair.len() == 2 {
            return Ok(Filter::Community(
                Community::Standard((
                    community_pair.get(0).context("community MSB")?.parse::<u16>()?,
                    community_pair.get(1).context("community LSB")?.parse::<u16>()?
                ))
            ));
        }

        return Err(anyhow!("Invalid filter specification"));
    }
}

impl Filter {
    // True or false to say if the NLRI matches the expression
    pub fn eval(&self, nlri: &mut MrtNlri) -> bool {

        match self {

            // Return true if the assessed NLRI is contained
            // by the specific filter prefix term, eg. NLRI 192.0.2.0/24 would
            // match Prefix(192.0.0.0, 8).
            Filter::Prefix(p) => {
                if nlri.prefix.mask(p.len)==p.prefix.mask(p.len) && nlri.plen >= p.len {
                    true
                } else {
                    false
                }
            },

            // Return true if the filter term IP address is within the
            // longest-prefix-match routing scope of the NLRI eg.
            Filter::LPM(ipaddr) => {
                if ipaddr.mask(nlri.plen)==nlri.prefix {
                    true
                } else {
                    false
                }
            }

            // Filter (and modify) the NLRI rib entries to include only
            // the paths with the specific ASN
            Filter::As(asn) => {
                nlri.rib_entries.retain(|x| x.aspath_contains(*asn));
                !nlri.rib_entries.is_empty()
            },

            Filter::AsPath(seq) => {
                nlri.rib_entries.retain(|x| x.aspath_contains_sequence(seq));
                !nlri.rib_entries.is_empty()
            },

            Filter::Community(comm) => {
                nlri.rib_entries.retain(|x| x.community_contains(comm));
                !nlri.rib_entries.is_empty()
            }

            Filter::AsPathRegex(re_aspath) => {
                nlri.rib_entries.retain(|x| re_aspath.is_match(&x.get_aspath_str()));
                !nlri.rib_entries.is_empty()
            },

            Filter::CommunityRegex(re_comm) => {
                nlri.rib_entries.retain(|x| x.community_contains_regex(re_comm));
                !nlri.rib_entries.is_empty()
            }

            _ => false
        }
    }
}

