//use core::slice::SlicePattern;
use std::fmt::{Display, Formatter};
use std::net::IpAddr;
use std::time::SystemTime;

use crate::*;

#[derive(Debug)]
pub struct MrtRibEntry {
    pub peer_id: u16,
    pub origin_time: SystemTime,
    pub attributes: Vec<MrtAttribute>
}

impl MrtRibEntry {
    pub fn get_aspath(&self) -> String {
        let empty = String::new();
        for attrib in &self.attributes {
            if let MrtAttribute::AsPath(ref aspath) = attrib {
                return aspath.to_string();
            }
        }
        empty
    }

    pub fn get_community(&self) -> Option<String> {
        for attrib in &self.attributes {
            if let MrtAttribute::Community(ref community_list) = attrib {
                return Some(community_list
                    .iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<String>>().join(" "));
            }
        }
        None
    }


    pub fn aspath_contains(&self, asn: u32) -> bool {
        for attrib in &self.attributes {
            if let MrtAttribute::AsPath(ref aspath) = attrib {
                if aspath.contains(asn) {
                    return true;
                }
            }
        }
        false
    }

    pub fn community_contains(&self, community: &Community) -> bool {
        for attrib in &self.attributes {
            if let MrtAttribute::Community(ref community_list) = attrib {
                if community_list.contains(community) {
                    return true;
                }
            }
        }
        false
    }
    pub fn get_med(&self) -> Option<u32> {
        for attrib in &self.attributes {
            if let MrtAttribute::MultiExitDisc(med) = attrib {
                return Some(*med);
            }
        }
        None
    }
    pub fn get_local_pref(&self) -> Option<u32> {
        for attrib in &self.attributes {
            if let MrtAttribute::LocalPref(local_pref) = attrib {
                return Some(*local_pref);
            }
        }
        None
    }
    pub fn get_nexthop(&self) -> IpAddr {
        for attrib in &self.attributes {
            if let MrtAttribute::NextHop(nh) = attrib {
                return *nh;
            }
        }
        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
    }
    pub fn get_origin(&self) -> u8 {
        for attrib in &self.attributes {
            if let MrtAttribute::Origin(origin) = attrib {
                return *origin;
            }
        }
        255
    }

    pub fn get_origin_char(&self) -> char {
        match self.get_origin() {
            0 => {
                if GETOPT.juniper_output {
                    'I'
                } else {
                    'i'
                }
            },
            1 => {
                if GETOPT.juniper_output {
                    'E'
                } else {
                    'e'
                }
            },
            2 => '?',
            _ => '!'
        }
    }
}

impl Display for MrtRibEntry {
    fn fmt(&self, f: &mut Formatter<'_>) -> anyhow::Result<(), std::fmt::Error> {
        write!(f, "{} \"{} {}\"", self.get_nexthop(), self.get_aspath(), self.get_origin_char())
    }
}

#[derive(Debug)]
pub struct MrtNlri {
    pub sequence: u32,
    pub plen: u8,
    pub prefix: IpAddr,
    pub entry_count: u16,
    pub rib_entries: Vec<MrtRibEntry>
}

impl Display for MrtNlri {
    fn fmt(&self, f: &mut Formatter<'_>) -> anyhow::Result<(), std::fmt::Error> {
        write!(f, "{}/{}: {}",
               &self.prefix,
               &self.plen,
               self.rib_entries
                   .iter()
                   .map(|x| x.to_string())
                   .collect::<Vec<String>>()
                   .join(", "))
    }
}

impl MrtNlri {
    pub fn parse_rib_entry<R: Read + BufRead>(reader: &mut R) -> Result<MrtRibEntry> {

        let peer_id = reader.read_u16::<BigEndian>()?;
        let origin_time = reader.read_u32::<BigEndian>()?;
        let attribute_length = reader.read_u16::<BigEndian>()?;

        let mut attributes: &[u8] = &reader.fill_buf()?[..attribute_length as usize];
        let attributes = MrtAttribute::parse(&mut attributes)?;
        reader.consume(attribute_length as usize);

        Ok(
            MrtRibEntry {
                peer_id,
                origin_time: UNIX_EPOCH.checked_add(Duration::from_secs(origin_time as u64)).unwrap_or(UNIX_EPOCH),
                attributes
            }
        )
    }

    pub fn parse_v4<R: Read + BufRead>(reader: &mut R) -> Result<MrtNlri> {
        let mut addr_buf: [u8; 4] = [0u8; 4];
        let sequence = reader.read_u32::<BigEndian>()?;

        let plen: u8 = reader.read_u8()?;
        reader.read_exact(&mut addr_buf[..((plen + 7)/8) as usize])?;
        let mut slice = &addr_buf[..];
        let prefix = IpAddr::V4(Ipv4Addr::from_bits(slice.read_u32::<BigEndian>()?));

        let entry_count = reader.read_u16::<BigEndian>()?;
        let mut rib_entries = vec![];
        for _ in 0..entry_count {
            rib_entries.push(Self::parse_rib_entry(reader)?);
        }

        Ok(MrtNlri { sequence, plen, prefix, entry_count, rib_entries })
    }

    pub fn parse_v6<R: Read + BufRead>(reader: &mut R) -> Result<MrtNlri> {
        let mut addr_buf: [u8; 16] = [0u8; 16];
        let sequence = reader.read_u32::<BigEndian>()?;

        let plen: u8 = reader.read_u8()?;
        reader.read_exact(&mut addr_buf[..((plen + 7)/8) as usize])?;
        let mut slice = &addr_buf[..];
        let prefix = IpAddr::V6(Ipv6Addr::from_bits(slice.read_u128::<BigEndian>()?));

        let entry_count = reader.read_u16::<BigEndian>()?;
        let mut rib_entries = vec![];
        for _ in 0..entry_count {
            rib_entries.push(Self::parse_rib_entry(reader)?);
        }

       Ok(MrtNlri { sequence, plen, prefix, entry_count, rib_entries })
    }

}
