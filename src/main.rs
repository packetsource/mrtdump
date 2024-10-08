#![allow(dead_code, unused_imports)]
use std::env;
use std::io::{self, Read, BufReader, BufRead, ErrorKind, Write};
use std::str::FromStr;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use byteorder::{BigEndian, ReadBytesExt};
use bzip2::read::BzDecoder;
use anyhow::{Result, anyhow, Context};
use std::fmt::{Display, Formatter};
use std::process;
use std::time::{Instant, Duration, SystemTime, UNIX_EPOCH};
use anyhow::__private::kind::TraitKind;
use time::OffsetDateTime;
use std::collections::HashMap;
use std::rc::Rc;
use lazy_static::lazy_static;

mod mrt; use mrt::*;
mod rib; use rib::*;
mod attribute; use attribute::*;
mod aspath; use aspath::*;
mod peer;
mod getopt;
mod util; use util::*;
mod filter; use filter::*;
mod ipaddrmask; use ipaddrmask::*;
mod output; use output::*;

mod routing_table; use routing_table::*;
mod trie;
mod prefix;
mod community; use community::*;

use prefix::*;

use trie::*;

use peer::*;

lazy_static! { static ref GETOPT: getopt::Getopt = getopt::getopt(); }

const CISCO_DEFAULT_WEIGHT: u32 = 32768;
const DEFAULT_LOCAL_PREF: u32 = 100;

pub fn usage() {
    eprintln!("Usage: mrtdump [-v] [-j] [-i] [-f filter] filename");
    eprintln!("       -v     verbose/debug (troubleshooting)");
    eprintln!("       -f     filter the routes loaded: (filters are ANDed, with initial default permit-all)");
    eprintln!("                 A.B.C.D/X - any routes equal or more specific");
    eprintln!("                 12345     - any routes with path containing the ASN (not full AS Path regex)");
    eprintln!("                 12345:100 - any routes with attached community attribute");
    eprintln!("       -j     use Juniper-style \"show route\" output (rather than Cisco \"show ip bgp\")");
    eprintln!("       -i     run interactive shell for IP address queries after loading (default if no load filter)");
    process::exit(1);
}

fn main() -> Result<()> {
    if GETOPT.verbose {
        dbg!(&*GETOPT);
    }

    // let filename = GETOPT.args.get(0).expect("Expected input MRT filename");

    // Global
    let mut routing_table = RoutingTable::new();
    let mut peers: HashMap<(IpAddr, String, u16), Rc<MrtPeer>> = HashMap::new();

    for filename in &GETOPT.args {

        let mut count: u64 = 0;
        let start_time = Instant::now();

        let mut reader: Box<dyn BufRead> = {
            if filename.ends_with(".bz2") {
                Box::new(BufReader::new(BzDecoder::new(BufReader::new(std::fs::File::open(filename)?)))) as Box<dyn BufRead>
            } else {
                Box::new(BufReader::new(std::fs::File::open(filename)?)) as Box<dyn BufRead>
            }
        };

        // For each file
        let mut peer_index_table: MrtPeerIndexTable = MrtPeerIndexTable::default();

        // For each MRT message
        loop {
            match Mrt::parse(&mut reader, &peer_index_table) {
                Ok(mrt) => {
                    match mrt.data {
                        MrtRecord::PeerIndexTable(table) => {
                            let collector_id = table.collector_id.clone();
                            let view_name = table.view_name.clone();

                            peer_index_table = table;   // store the table

                            // Load all the peers into the global table
                            for (index, peer) in peer_index_table.peers.iter().enumerate() {
                                if index < u16::MAX.into() {
                                    peers.insert((collector_id, view_name.clone(), index as u16), Rc::clone(peer));
                                }
                            }

                            // If the filter is empty, or we are in verbose mode, then
                            // show the Cisco header, because we will print summary routes
                            // as we go
                            if !GETOPT.filter.is_empty() {
                                if GETOPT.juniper_output == false && GETOPT.terse_output == false {
                                    cisco_show_ip_bgp_header(mrt.timestamp,
                                                             &peer_index_table);
                                }
                            }
                        }
                        MrtRecord::RibIpv4Unicast(nlri) => {
                            if load_nlri(nlri, &mut routing_table) {
                                count += 1;
                            }
                        },
                        MrtRecord::RibIpv6Unicast(nlri) => {
                            if load_nlri(nlri, &mut routing_table) {
                                count += 1;
                            }
                        },

                        _ => {},
                    }
                }
                Err(e) => {
                    // what a ball-ache just to catch EOF as a non-error - do better, Adam!
                    if let Some(e) = e.downcast_ref::<std::io::Error>() {
                        if e.kind() == ErrorKind::UnexpectedEof {
                            eprintln!("{} entries from {} in {:?}", count, &filename, start_time.elapsed());
                            break;
                        }
                    }
                    println!("Encountered error while reading {}: {}", &filename, &e);
                    break;
                }
            }
        }
    }

    // Take interactive queries on the loaded routing table if there are
    // no filters present, or if the interactive switch is requested
    if GETOPT.interactive || GETOPT.filter.is_empty() {
        let mut reader = io::stdin().lock();
        loop {
            let mut query = String::new();
            print!("> "); let _ = io::stdout().flush();
            match reader.read_line(&mut query) {
                Ok(usize) if usize > 0 => {
                    trim_newline(&mut query);
                    if query.is_empty() {
                        continue;
                    }
                    match IpAddr::from_str(&query) {
                        Ok(ipaddr) => {
                            let result = routing_table.get(&ipaddr);
                            if let Some((ipaddr, plen, route_entries)) = result {
                                if GETOPT.juniper_output {
                                    juniper_show_route(&ipaddr, plen, &route_entries);
                                } else if GETOPT.terse_output {
                                    csv_show_route(&ipaddr, plen, &route_entries);
                                } else {
                                    cisco_show_ip_bgp_detail(&ipaddr, plen, &route_entries);
                                }
                            } else {
                                println!("Not found: {}", &query);
                            }
                        },
                        _ => {
                            println!("Invalid IP address: {}", &query);
                        }
                    }
                },
                _ => { break; }
            }
        }
    }

    // Load the MRT peer table for the file into the global hash

    #[allow(unreachable_code)]
    Ok(())
}

// Before loading the NLRI into the routing table,
// execute any specified load filters, in the order
// defined.
//
// Load filters will return whether the filter
// should be continued to be processed (true),
// or discarded (false), and any matched NLRIs will
// be printed using the selected dialect (Cisco/Juniper)
//
// We start with permit (true) logic, so no filters means
// all routes of course
//
// The NLRI is consumed by this operation
pub fn load_nlri(mut nlri: MrtNlri,
                    routing_table: &mut RoutingTable) -> bool {

    let matched: bool = GETOPT.filter.iter().fold(true, |x, f| {
        if x {
            f.eval(&mut nlri)
        } else {
            x
        }
    });

    if matched {

        // Display the matched route if there are filters in play
        // or if verbose  is enabled
        if GETOPT.verbose || GETOPT.filter.len() > 0 {
            if GETOPT.juniper_output {
                juniper_show_route(&nlri.prefix, nlri.plen, &nlri.rib_entries);
            } else if GETOPT.terse_output {
                csv_show_route(&nlri.prefix, nlri.plen, &nlri.rib_entries);
            } else {
                cisco_show_ip_bgp(&nlri.prefix, nlri.plen, &nlri.rib_entries);
            }
        }

        match nlri.prefix {
            IpAddr::V4(ipv4) => {
                routing_table.v4.add(&ipv4, nlri.plen, nlri.rib_entries);
            },
            IpAddr::V6(ipv6) => {
                routing_table.v6.add(&ipv6, nlri.plen, nlri.rib_entries);
            }
        }
    }
    matched
}