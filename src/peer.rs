use std::fmt::{Display, Formatter};
use std::net::IpAddr;

use crate::*;

#[derive(Debug, Clone)]
pub struct MrtPeer {
    pub peer_type_a: bool,
    pub peer_type_i: bool,
    pub peer_id: IpAddr,
    pub peer_address: IpAddr,
    pub peer_as: u32
}

#[derive(Debug)]
pub struct MrtPeerIndexTable {
    pub collector_id: IpAddr,
    pub view_name: String,
    pub peer_count: u16,
    pub peers: Vec<Rc<MrtPeer>>
}


impl Display for MrtPeer {
    fn fmt(&self, f: &mut Formatter<'_>) -> anyhow::Result<(), std::fmt::Error> {
        write!(f, "{} (AS{})", &self.peer_address, &self.peer_as)
    }
}

impl Default for MrtPeer {
    fn default() -> MrtPeer {
        MrtPeer {
            peer_type_a: true,
            peer_type_i: false,
            peer_id: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            peer_address: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            peer_as: 0,
        }
    }
}

impl MrtPeerIndexTable {
    pub fn parse<R: Read>(reader: &mut R) -> Result<MrtPeerIndexTable> {
        let collector_id = IpAddr::V4(Ipv4Addr::from_bits(reader.read_u32::<BigEndian>()?));
        let view_name_size = reader.read_u16::<BigEndian>()?;
        let mut view_name = vec!(0u8; view_name_size as usize);
        reader.read_exact(&mut view_name)?;
        let view_name = String::from_utf8(view_name)?;
        let peer_count = reader.read_u16::<BigEndian>()?;

        let mut peer_index_table = MrtPeerIndexTable {
            collector_id,
            view_name,
            peer_count,
            ..Default::default()
        };
        for _ in 0..peer_count {
            let peer_type = reader.read_u8()?;

            let peer_type_a: bool = (peer_type & (1<<1)) > 0;
            let peer_type_i: bool = (peer_type & (1<<0)) > 0;

            let peer_id = IpAddr::V4(Ipv4Addr::from_bits(reader.read_u32::<BigEndian>()?));
            let peer_address = if peer_type_i {
                IpAddr::V6(Ipv6Addr::from_bits(reader.read_u128::<BigEndian>()?))
            } else {
                IpAddr::V4(Ipv4Addr::from_bits(reader.read_u32::<BigEndian>()?))
            };
            let peer_as= if peer_type_a {
                reader.read_u32::<BigEndian>()?
            } else {
                reader.read_u16::<BigEndian>()? as u32
            };
            let peer = MrtPeer { peer_type_a, peer_type_i, peer_id, peer_address, peer_as };
            // dbg!(&peer);
            peer_index_table.peers.push(Rc::new(peer));
        }
        Ok(peer_index_table)
    }
}

impl Default for MrtPeerIndexTable {
    fn default() -> MrtPeerIndexTable {
        MrtPeerIndexTable {
            collector_id: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            view_name: String::new(),
            peer_count: 0,
            peers: Vec::<std::rc::Rc<peer::MrtPeer>>::new()
        }
    }
}


// Don't want a panicking index operator in the middle of an MRT file, thank you
//
// impl std::ops::Index<usize> for MrtPeerIndexTable {
//     type Output = Rc<MrtPeer>;

    //
    // fn index(&self, index: usize) -> &Self::Output {
    //     if index < self.peers.len() {
    //         & self.peers[index]
    //     } else {
    //         panic!("peer index out of range!");
    //     }
    // }
// }