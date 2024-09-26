// use core::slice::SlicePattern;
use std::io::{ErrorKind, Read};
use std::net::IpAddr;
use anyhow::anyhow;
use byteorder::{BigEndian, ReadBytesExt};

use crate::*;

#[derive(Debug, PartialEq)]
pub enum MrtAttribute {
    Unknown,
    Origin(u8),
    AsPath(AsPath),
    NextHop(IpAddr),
    MultiExitDisc(u32),
    LocalPref(u32),
    AtomicAggregate,
    Community(Vec<Community>)
}

impl MrtAttribute {
    pub fn parse<R: Read + BufRead>(reader: &mut R) -> anyhow::Result<Vec<MrtAttribute>> {
        let mut attributes: Vec<MrtAttribute> = vec![];
        loop {
            let flags: u8 = match reader.read_u8() {
                Ok(flags) => Ok(flags),
                Err(e) if e.kind() == ErrorKind::UnexpectedEof => break, /* we are done*/
                Err(e) => Err(e),
            }?;
            let code: u8 = reader.read_u8()?;
            let _flag_optional: bool = (flags & (1<<7)) > 0;
            let _flag_transitive: bool = (flags & (1<<6)) > 0;
            let _flag_partial: bool = (flags & (1<<5)) > 0;
            let flag_extended: bool = (flags & (1<<4)) > 0;
            //dbg!(flags, flag_extended);

            let length = {
                if flag_extended {
                    reader.read_u16::<BigEndian>()? as usize
                } else {
                    reader.read_u8()? as usize
                }
            };

            // Take the attribute data (backed by a Vec and at least as long as the
            // whole MRT message), and cap it to the reported attribute length
            let mut data: &[u8] = &reader.fill_buf()?[..length];

            let attribute: MrtAttribute = match code {
                1 => {
                    if length != 1 {
                        return Err(anyhow!("ORIGIN attribute: expected length 1, got {}", length));
                    }
                   // MrtAttribute::Origin(data.as_slice().read_u8()?)
                    MrtAttribute::Origin(data.read_u8()?)
                },
                2 => {
                    // MrtAttribute::AsPath(AsPath::parse(&mut data.as_slice())?)
                    MrtAttribute::AsPath(AsPath::parse(&mut data)?)
                },
                3 => {
                    if length == 4 {
                        // let nexthop = data.as_slice().read_u32::<BigEndian>()?;
                        let nexthop = data.read_u32::<BigEndian>()?;
                        MrtAttribute::NextHop(IpAddr::V4(Ipv4Addr::from_bits(nexthop)))
                    } else if length == 16 {
                        // let nexthop = data.as_slice().read_u128::<BigEndian>()?;
                        let nexthop = data.read_u128::<BigEndian>()?;
                        MrtAttribute::NextHop(IpAddr::V6(Ipv6Addr::from_bits(nexthop)))
                    } else {
                        return Err(anyhow!("NEXT_HOP attribute: expected length 4 or 16, got {}", length));
                    }
                },
                4 => {
                    if length != 4 {
                        return Err(anyhow!("MULTI_EXIT_DISC attribute: expected length 4, got {}", length));
                    }
                   // MrtAttribute::MultiExitDisc(data.as_slice().read_u32::<BigEndian>()?)
                    MrtAttribute::MultiExitDisc(data.read_u32::<BigEndian>()?)

                },
                5 => {
                    if length != 4 {
                        return Err(anyhow!("LOCAL_PREF attribute: expected length 4, got {}", length));
                    }
                    // MrtAttribute::LocalPref(data.as_slice().read_u32::<BigEndian>()?)
                    MrtAttribute::LocalPref(data.read_u32::<BigEndian>()?)
                },
                6 => {
                    if length != 0 {
                        return Err(anyhow!("ATOMIC_AGGREGATE attribute: expected length 0, got {}", length));
                    }
                    MrtAttribute::AtomicAggregate
                },
                8 => {
                    if length % 4 != 0 {
                        return Err(anyhow!("COMMUNITY attribute: expected length divisible by 4, got {}", length));
                    }
                    MrtAttribute::Community(Community::parse(&mut data, length / 4)?)
                }

                // unknown attribute, don't read data, ignore
                _ => {
                    MrtAttribute::Unknown
                }
            };

            // Advance the cursor by the length of the attribute
            // (whether we have understood it or not)
            reader.consume(length);

            // Store the attribute in the RibEntry attribute list
            if attribute != MrtAttribute::Unknown {
                attributes.push(attribute);
            }
        }
        Ok(attributes)
    }
}
