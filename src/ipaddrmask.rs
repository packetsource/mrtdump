use crate::*;

pub trait IpAddrMask {
    fn mask(&self, len: u8) -> IpAddr;
}

// Apply a prefix-length based mask to an IpAddr
// to ease comparison operations, look-ups etc
impl IpAddrMask for IpAddr {
    fn mask(&self, len: u8) -> IpAddr {
        let mask_v4: u32 = if len > 0 && len <= 32 {
            u32::MAX << (32 - len)
        } else {
            0u32
        };
        let mask_v6: u128 = if len > 0 && len <= 128 {
            u128::MAX << (128 - len)
        } else {
            0u128
        };
        match self {
            IpAddr::V4(ip) => IpAddr::V4(((<Ipv4Addr as Into<u32>>::into(*ip)) & mask_v4).into()),
            IpAddr::V6(ip) => IpAddr::V6(((<Ipv6Addr as Into<u128>>::into(*ip)) & mask_v6).into()),
        }
    }
}
