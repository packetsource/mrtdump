use std::error::Error;
use std::fs::File;
use std::io;
use std::io::{BufRead, BufReader};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;
use std::str::FromStr;
use std::time::Instant;

use crate::*;

pub struct RoutingTable<V: std::cmp::PartialEq> {
    pub v4: RadixTrie<Ipv4Addr, V>,
    pub v6: RadixTrie<Ipv6Addr, V>,
}

impl<V: std::cmp::PartialEq> RoutingTable<V>
// where
//     T: std::fmt::Display,
{
    pub fn iter(&self) -> impl Iterator<Item = (IpAddr, u8, &Vec<V>)> {
        let v4 = self.v4.iter()
            .filter_map(|(a, len, v)| v.map(|v| (IpAddr::V4(a), len, v)));
        let v6 = self.v6.iter()
            .filter_map(|(a, len, v)| v.map(|v| (IpAddr::V6(a), len, v)));
        v4.chain(v6)
    }

    pub fn get(&self, ip: &IpAddr) -> Option<(IpAddr, u8, &Vec<V>)> {
        match ip {
            IpAddr::V4(ip) => match self.v4.get(*ip, 32) {
                Some((route, plen, desc)) => Some((IpAddr::V4(route), plen, desc)),
                None => None,
            },
            IpAddr::V6(ip) => match self.v6.get(*ip, 128) {
                Some((route, plen, desc)) => Some((IpAddr::V6(route), plen, desc)),
                None => None,
            },
        }
    }
    pub fn get_exact(&self, ip: &IpAddr, len: u8) -> Option<(IpAddr, u8, &Vec<V>)> {
        match ip {
            IpAddr::V4(ip) => match self.v4.get(*ip, len.min(32)) {
                Some((route, plen, desc)) if plen == len => Some((IpAddr::V4(route), plen, desc)),
                _ => None,
            },
            IpAddr::V6(ip) => match self.v6.get(*ip, len.min(128)) {
                Some((route, plen, desc)) if plen == len => Some((IpAddr::V6(route), plen, desc)),
                _ => None,
            },
        }
    }

}

impl<V: std::cmp::PartialEq> RoutingTable<V> {
    pub fn new() -> RoutingTable<V> {
        RoutingTable {
            v4: RadixTrie::new(Ipv4Addr::UNSPECIFIED, 0),
            v6: RadixTrie::new(Ipv6Addr::UNSPECIFIED, 0),
        }
    }
}

