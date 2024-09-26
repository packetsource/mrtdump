use std::fmt::{Display, Formatter};
use std::io::Read;
use std::net::IpAddr;
use byteorder::{BigEndian, ReadBytesExt};
use time::OffsetDateTime;

use crate::*;
// MRT Header structure
#[derive(Debug)]
pub struct Mrt {
    pub timestamp: u32,
    pub mrt_type: u16,
    pub mrt_subtype: u16,
    pub length: u32,
    pub data: MrtRecord,
}

impl Display for Mrt {
    fn fmt(&self, f: &mut Formatter<'_>) -> anyhow::Result<(), std::fmt::Error> {
        write!(f, "{} {}/{}: {}",
               OffsetDateTime::from_unix_timestamp(self.timestamp.into()).unwrap_or(OffsetDateTime::UNIX_EPOCH),
               &self.mrt_type, &self.mrt_subtype, match &self.data {
                MrtRecord::PeerIndexTable(table) => {
                    format!("{}: {}: {} peer(s): {}",
                            &table.collector_id,
                            &table.view_name,
                            &table.peer_count,
                            &table.peers.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", "))
                },
                MrtRecord::RibIpv4Unicast(nlri) => {
                    format!("{}/{}: {}",
                            &nlri.prefix,
                            &nlri.plen,
                            &nlri.rib_entries.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", "))
                },
                _ => "".to_string()
            })
    }
}

// MRT Record structure
#[derive(Debug)]
pub enum MrtRecord {
    PeerIndexTable(MrtPeerIndexTable),
    RibIpv4Unicast(MrtNlri),
    RibIpv4Multicast,
    RibIpv6Unicast,
    RibIpv6Multicast,
    RibGeneric,
}

impl Mrt {

    pub fn parse<R: Read + BufRead>(reader: &mut R) -> anyhow::Result<Mrt> {
        let timestamp = reader.read_u32::<BigEndian>()?;
        let mrt_type = reader.read_u16::<BigEndian>()?;
        let mrt_subtype = reader.read_u16::<BigEndian>()?;
        let length = reader.read_u32::<BigEndian>()?;

        let mut data = vec![0u8; length as usize];
        reader.read_exact(&mut data)?;
        let mut slice = data.as_slice();

        let data: MrtRecord = match (mrt_type, mrt_subtype) {
            (13, 1) => {
                MrtRecord::PeerIndexTable(MrtPeerIndexTable::parse(&mut slice)?)
            },
            (13, 2) => {
                MrtRecord::RibIpv4Unicast(MrtNlri::parse(&mut slice)?)
            },
            _ => {
                eprintln!("Unknown MRT record: {}/{}", mrt_type, mrt_subtype);
                MrtRecord::RibGeneric
            }
        };
        // reader.consume(length as usize);
        Ok(Mrt { timestamp, mrt_type, mrt_subtype, length, data })
    }

}