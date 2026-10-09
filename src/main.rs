#![allow(dead_code, unused_imports)]

// RIPE RIS URL format:   https://data.ris.ripe.net/rrcXX/YYYY.MM/TYPE.YYYYMMDD.HHmm.gz
// Routeviews URL format: https://archive.routeviews.org/route-views.linx/bgpdata/
// Routeviews URL format: https://archive.routeviews.org/route-views.linx/bgpdata/2004.03/RIBS/


use std::env;
use std::io::{self, Read, BufReader, BufRead, ErrorKind, Write};
use std::fs::{self, File};
use std::str::FromStr;
use std::path::Path;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::fmt::{Display, Formatter};
use std::process;
use std::time::{Instant, Duration, SystemTime, UNIX_EPOCH};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use byteorder::{BigEndian, ReadBytesExt};
use bzip2::read::BzDecoder;
use flate2::bufread::GzDecoder;
use anyhow::{Result, anyhow, Context};

use bytes::Bytes;
use lazy_static::lazy_static;
use time::format_description::{self, well_known::Rfc2822};
use time::OffsetDateTime;
use reqwest::blocking::Client;
use url::Url;
use serde::{Serialize, Deserialize};
use serde_json;
use uuid::Uuid;
use time;

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

mod http; use http::*;

mod routing_table; use routing_table::*;
mod radixtrie;
mod prefix;
mod community;
mod cache;
mod sources;
mod sshserver;
mod cli;

use community::*;
use prefix::*;
use radixtrie::*;
use peer::*;
use cache::*;
use sources::*;

lazy_static! { static ref GETOPT: getopt::Getopt = getopt::getopt(); }

const CISCO_DEFAULT_WEIGHT: u32 = 32768;
const DEFAULT_LOCAL_PREF: u32 = 100;

pub fn usage() {
    eprintln!("Usage: mrtdump [-v] [-j] [-t] [-i] [-f filter] filename");
    eprintln!("       -v     verbose/debug (troubleshooting)");
    eprintln!("       -S RIPE | RV");
    eprintln!("              load all known MRT files for yesterday from specified source");
    eprintln!("       -f     filter the routes loaded: (filters are ANDed, with initial default ");
    eprintln!("              permit-all)");
    eprintln!("                 A.B.C.D   - any routes equal or less-specific than the IP address");
    eprintln!("                 A.B.C.D/X - any routes equal or more specific");
    eprintln!("                 12345     - any routes with path containing the ASN");
    eprintln!("                                            (not full AS Path regex)");
    eprintln!("                 12345:100 - any routes with attached community attribute");
    eprintln!("       -j     use Juniper-style \"show route\" output");
    eprintln!("       -t     use Oliver-style |-delimitered output");
    eprintln!("       -i     store observed routes in a routing trie and run interactive shell");
    eprintln!("              to interrogate after loading");
    eprintln!("       -s PORT start SSH server on PORT after loading (no authentication);");
    eprintln!("              use: ssh -o StrictHostKeyChecking=no localhost -p PORT");
    process::exit(1);
}

fn main() -> Result<()> {
    if GETOPT.verbose {
        dbg!(&*GETOPT);
    }
    
    // Global
    let mut cache = Cache::load(&format!("{}/{}", env::var("HOME")?, ".mrtdump-cache"));
    let mut routing_table: RoutingTable<MrtRibEntry> = RoutingTable::new();
    let mut peers: HashMap<(IpAddr, String, u16), Arc<MrtPeer>> = HashMap::new();

    let mut total_route_count: u64 = 0;
    let mut total_path_count: u64 = 0;
    let mut distinct_asns = HashSet::<u32>::new();

    let start_time = Instant::now();

    // For each file
    for filename in &GETOPT.args {

        let mut route_count: u64 = 0;
        let mut path_count: u64 = 0;

        let file_start_time = Instant::now();

        // Read the data, either from disk or network
        let data = {
            if filename.starts_with("https://") || filename.starts_with("http://") {
                match cache.get_url(&filename) {
                    Ok(data) => {
                        cache.save()?;
                        data
                    },
                    Err(e) => {
                        eprintln!("HTTP(S) download error: {}", e);
                        continue;
                    }
                }
            } else {
                std::fs::read(&filename)?
            }
        };

        let mut reader = {
            if filename.ends_with(".bz2") {
                Box::new(BufReader::new(BzDecoder::new(data.as_slice()))) as Box<dyn BufRead>
            } else if filename.ends_with(".gz") {
                Box::new(BufReader::new(GzDecoder::new(data.as_slice()))) as Box<dyn BufRead>
            } else {
                Box::new(BufReader::new(data.as_slice())) as Box<dyn BufRead>
            }
        };

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
                                    peers.insert((collector_id, view_name.clone(), index as u16), Arc::clone(peer));
                                }
                            }
                        }
                        MrtRecord::RibIpv4Unicast(mut nlri) => {
                            if filter_nlri(&mut nlri) {
                                route_count += 1;
                                total_route_count += 1;
                                path_count += MrtRibEntry::count_paths(&nlri.rib_entries);
                                total_path_count += MrtRibEntry::count_paths(&nlri.rib_entries);
                                MrtRibEntry::count_distinct_asns(&mut distinct_asns, &nlri.rib_entries);
                                load_nlri(nlri, &mut routing_table);
                            }
                        },
                        MrtRecord::RibIpv6Unicast(mut nlri) => {
                            if filter_nlri(&mut nlri) {
                                route_count += 1;
                                total_route_count += 1;
                                path_count += MrtRibEntry::count_paths(&nlri.rib_entries);
                                total_path_count += MrtRibEntry::count_paths(&nlri.rib_entries);
                                MrtRibEntry::count_distinct_asns(&mut distinct_asns, &nlri.rib_entries);
                                load_nlri(nlri, &mut routing_table);
                            }
                        },
                        _ => {},
                    }
                }
                Err(e) => {
                    // what a ball-ache just to catch EOF as a non-error - do better, Adam!
                    if let Some(e) = e.downcast_ref::<std::io::Error>() {
                        if e.kind() == ErrorKind::UnexpectedEof {
                            if GETOPT.verbose {
                                eprintln!("{} route(s), {} path(s) from file {} ({}:{}) in {:?}",
                                          route_count,
                                          path_count,
                                          &filename,
                                          &peer_index_table.collector_id,
                                          &peer_index_table.view_name,
                                          file_start_time.elapsed());
                            }
                            break;
                        }
                    }
                    println!("Encountered error while reading {}: {}", &filename, &e);
                    break;
                }
            }
        }
    }

    eprintln!("Read {} route(s), {} path(s), {} distinct ASNs in {:?}",
        total_route_count,
        total_path_count,
        distinct_asns.len(),
        start_time.elapsed()
    );

    if let Some(port) = GETOPT.ssh_port {
        let table = Arc::new(routing_table);
        tokio::runtime::Runtime::new()?
            .block_on(sshserver::run(table, port, GETOPT.juniper_output, GETOPT.terse_output))?;
        return Ok(());
    }

    // Interactive shell on the local terminal (or stdin) over the loaded table
    if GETOPT.interactive {
        return cli::local::run(
            Arc::new(routing_table),
            cli::commands::OutputFormat::from_flags(GETOPT.juniper_output, GETOPT.terse_output),
        );
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
pub fn load_nlri(nlri: MrtNlri,
                    routing_table: &mut RoutingTable<MrtRibEntry>) {

    if GETOPT.interactive {
        match nlri.prefix {
            IpAddr::V4(ipv4) => {
                routing_table.v4.add_vec(ipv4, nlri.plen, nlri.rib_entries);
            },
            IpAddr::V6(ipv6) => {
                routing_table.v6.add_vec(ipv6, nlri.plen, nlri.rib_entries);
            }
        }
    } else {
        let mut stdout = io::stdout();
        if GETOPT.juniper_output {
            juniper_show_route(&mut stdout, &nlri.prefix, nlri.plen, &nlri.rib_entries).ok();
        } else if GETOPT.terse_output {
            csv_show_route(&mut stdout, &nlri.prefix, nlri.plen, &nlri.rib_entries).ok();
        } else {
            cisco_show_ip_bgp(&mut stdout, &nlri.prefix, nlri.plen, &nlri.rib_entries).ok();
        }
    }
}

pub fn filter_nlri(nlri: &mut MrtNlri) -> bool {
    GETOPT.filter.iter().fold(true, |x, f| {
        if x {
            f.eval(nlri)
        } else {
            x
        }
    })
}
