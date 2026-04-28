use std::error::Error;
use std::fs::File;
use std::io;
use std::io::{BufRead, BufReader};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;
use std::str::FromStr;
use std::time::Instant;

use crate::*;

pub struct RoutingTable<V> {
    pub v4: Trie<Ipv4Addr, V>,
    pub v6: Trie<Ipv6Addr, V>,
}

// pub struct RoutingTableIterator<'a> {
//     pub table: &'a RoutingTable,
//
// }

impl<V> RoutingTable<V>
// where
//     T: std::fmt::Display,
{
    pub fn get(&self, ip: &IpAddr) -> Option<(IpAddr, u8, &Vec<V>)> {
        match ip {
            IpAddr::V4(ip) => match self.v4.get(ip, 32) {
                Some((route, plen, desc)) => Some((IpAddr::V4(route), plen, desc)),
                None => None,
            },
            IpAddr::V6(ip) => match self.v6.get(ip, 128) {
                Some((route, plen, desc)) => Some((IpAddr::V6(route), plen, desc)),
                None => None,
            },
        }
    }
}

impl<V> RoutingTable<V> {
    pub fn new() -> RoutingTable<V> {
        RoutingTable {
            v4: Trie::new(),
            v6: Trie::new(),
        }
    }
}

