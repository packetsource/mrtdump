use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

pub trait IpAddrMask {
    fn mask(&self, len: u8) -> Self;
    fn next_bit(&self, len: u8) -> u8;
    fn common_len(&self, len1: u8, other: &Self, len2: u8) -> u8;
    fn max_len(&self) -> u8;
}

// Apply a prefix-length based mask to an IpAddr to ease comparison operations, look-ups etc
impl IpAddrMask for IpAddr {
    fn mask(&self, len: u8) -> IpAddr {
        match self {
            IpAddr::V4(ip) => IpAddr::V4(ip.mask(len)),
            IpAddr::V6(ip) => IpAddr::V6(ip.mask(len))
        }
    }
    fn next_bit(&self, len: u8) -> u8 {
        match self {
            IpAddr::V4(ip) => ip.next_bit(len),
            IpAddr::V6(ip) => ip.next_bit(len)
        }
    }
    fn common_len(&self, len1: u8, other: &Self, len2: u8) -> u8 {
        match (self, other) {
            (IpAddr::V4(ip), IpAddr::V4(other)) => ip.common_len(len1, other, len2),
            (IpAddr::V6(ip), IpAddr::V6(other)) => ip.common_len(len1, other, len2),
            _ => panic!("IpAddrMask::common_len(): incompatible IPAddr types")
        }
    }
    fn max_len(&self) ->u8 {
        match self {
            IpAddr::V4(ip) => ip.max_len(),
            IpAddr::V6(ip) => ip.max_len(),
        }
    }
    
}

impl IpAddrMask for Ipv4Addr {
    fn mask(&self, len: u8) -> Ipv4Addr {
        let mask: u32 = if len > 0 && len <= 32 {
            u32::MAX << (32 - len)
        } else {
            0u32
        };
        (<Ipv4Addr as Into<u32>>::into(*self) & mask).into()
    }
    fn next_bit(&self, len: u8) -> u8 {
        ((self.to_bits() >> (((std::mem::size_of::<Ipv4Addr>() * 8) as u8) - len - 1)) & 1u32) as u8
    }
    fn common_len(&self, len1: u8, other: &Self, len2: u8) -> u8 {
        let shortest = len1.min(len2).min(32);
        ((self.to_bits() ^ other.to_bits()).leading_zeros() as u8).min(shortest)
        // let shortest_mask = len1.min(len2);
        // let common_len: u8 = 32 - (self.mask(shortest_mask).to_bits() ^ other.mask(shortest_mask).to_bits()).trailing_zeros() as u8;
        // if common_len > 0 {
        //     common_len - 1
        // } else {
        //     0
        // }
    }
    fn max_len(&self) -> u8 { 32 }
}
impl IpAddrMask for Ipv6Addr {
    fn mask(&self, len: u8) -> Ipv6Addr {
        let mask: u128 = if len > 0 && len <= 128 {
            u128::MAX << (128 - len)
        } else {
            0u128
        };
        (<Ipv6Addr as Into<u128>>::into(*self) & mask).into()
    }
    fn next_bit(&self, len: u8) -> u8 {
        ((self.to_bits() >> (((std::mem::size_of::<Ipv6Addr>() * 8) as u8) - len - 1)) & 1u128) as u8
    }
    fn common_len(&self, len1: u8, other: &Self, len2: u8) -> u8 {
        let shortest = len1.min(len2).min(128);
        ((self.to_bits() ^ other.to_bits()).leading_zeros() as u8).min(shortest)
    }
    fn max_len(&self) -> u8 { 128 }

}
