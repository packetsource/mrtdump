//! Command handlers and the message of the day.
//!
//! Handlers are synchronous and write to a `&mut dyn Write`. The shell runs
//! them on tokio's blocking pool, with the writer bridged to the SSH channel
//! (see `sink`), so a long table scan neither blocks the async runtime nor
//! buffers its whole output. A handler should propagate write errors with `?`:
//! a `BrokenPipe` means the user aborted the output or disconnected.

use std::collections::HashSet;
use std::io::{self, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use regex::Regex;

use crate::community::Community;
use crate::filter::Filter;
use crate::output::{cisco_show_ip_bgp, cisco_show_ip_bgp_detail, cisco_show_ip_bgp_header, csv_show_route, csv_show_route_header, juniper_show_route, juniper_show_route_header};
use crate::prefix::Prefix;
use crate::rib::{MrtNlri, MrtRibEntry};
use crate::routing_table::RoutingTable;

use super::commands::{Command, OutputFormat};

type Table = RoutingTable<MrtRibEntry>;

// ─── Message of the day ───────────────────────────────────────────────────────

/// Counts taken once at server start-up, for the MOTD.
pub struct TableSummary {
    pub v4_prefixes: u64,
    pub v6_prefixes: u64,
    pub paths: u64,
}

impl TableSummary {
    pub fn from_table(table: &Table) -> Self {
        let mut s = TableSummary { v4_prefixes: 0, v6_prefixes: 0, paths: 0 };
        for (prefix, _, entries) in table.iter() {
            if prefix.is_ipv4() {
                s.v4_prefixes += 1;
            } else {
                s.v6_prefixes += 1;
            }
            s.paths += entries.len() as u64;
        }
        s
    }
}

/// Printed once to each new interactive session.
pub fn motd(out: &mut dyn Write, summary: &TableSummary, peer: Option<SocketAddr>) -> io::Result<()> {
    writeln!(out)?;
    writeln!(out, "--- BFORT {} mrtdump", env!("CARGO_PKG_VERSION"))?;
    writeln!(out, "    running with {} IPv4 prefixes, {} IPv6 prefixes, {} paths",
             summary.v4_prefixes, summary.v6_prefixes, summary.paths
    )?;
    if let Some(peer) = peer {
        writeln!(out, "    connected established from {peer}")?;
    }
    writeln!(out)?;
    Ok(())
}

// ─── Dispatch ─────────────────────────────────────────────────────────────────

/// Run a table query. `output`, `terminal length` and `exit` change session
/// state and are handled by the shell itself, never reaching here.
pub fn dispatch(
    table: &Table,
    format: OutputFormat,
    cmd: Command,
    out: &mut dyn Write,
) -> io::Result<()> {
    match cmd {
        Command::ShowRouteAddr(addr) => show_route_addr(table, format, addr, out),
        Command::ShowRoutePrefix(prefix) => show_route_prefix(table, format, &prefix, out),
        Command::ShowRouteCommunity(c) => show_route_community(table, format, &c, out),
        Command::ShowRouteCommunityRegex(re) => show_route_community_regex(table, format, &re, out),
        Command::ShowRouteAsPathRegex((re, re_modified)) => show_route_as_path_regex(table, format, &re, re_modified, out),
        Command::Output(_) | Command::TerminalLength(_) | Command::Exit => Ok(()),
    }
}

// ─── Query handlers (stubs) ───────────────────────────────────────────────────

/// `show route W.X.Y.Z` / `show route X:X::X` - longest-prefix match.
/// Query the routing table for the destination, and then summarise
/// the AS Path attributes in the response to return a count
pub fn show_route_addr(
    table: &Table,
    format: OutputFormat,
    addr: IpAddr,
    out: &mut dyn Write,
) -> io::Result<()> {

    match format {
        OutputFormat::Juniper => {
            juniper_show_route_header(out, "inet.0", 0u64, 0u64)?;
        },
        OutputFormat::Cisco => {
            // No - don't use the header here, because we have a detailed view
        },
        OutputFormat::Csv => {
            csv_show_route_header(out)?;
        }
    }


    match table.get(&addr) {
        Some((prefix, plen, entries)) => {

            let mut distinct_asns = HashSet::<u32>::new();
            MrtRibEntry::count_distinct_asns(&mut distinct_asns, entries);

            match format {
                OutputFormat::Juniper => {
                    juniper_show_route(out, &prefix, plen, entries)?;
                    writeln!(out, "{} path(s), {} distinct ASNs",
                             MrtRibEntry::count_paths(&entries), distinct_asns.len() as u64)?
                },
                OutputFormat::Cisco => {
                    cisco_show_ip_bgp_detail(out, &prefix, plen, entries)?;
                    writeln!(out, "{} path(s), {} distinct ASNs",
                             MrtRibEntry::count_paths(&entries), distinct_asns.len() as u64)?
                }
                OutputFormat::Csv => {
                    csv_show_route(out, &prefix, plen, entries)?
                },
            }
        }
        None => writeln!(out, "% Not found: {}", addr)?
    };
    out.flush()?;
    Ok(())
}

/// `show route W.X.Y.Z/N` - exact prefix and length.
/// Same as LPM, but we want an exact match on the prefix and length
pub fn show_route_prefix(
    table: &Table,
    format: OutputFormat,
    prefix: &Prefix,
    out: &mut dyn Write,
) -> io::Result<()> {

    match format {
        OutputFormat::Juniper => {
            juniper_show_route_header(out, "inet.0", 0u64, 0u64)?;
        },
        OutputFormat::Cisco => {
            // No - don't use the header here, because we have a detailed view
        },
        OutputFormat::Csv => {
            csv_show_route_header(out)?;
        }
    }

    match table.get_exact(&prefix.prefix, prefix.len) {
        Some((prefix, plen, entries)) => {

            let mut distinct_asns = HashSet::<u32>::new();
            MrtRibEntry::count_distinct_asns(&mut distinct_asns, entries);

            match format {
                OutputFormat::Juniper => {
                    juniper_show_route(out, &prefix, plen, entries)?;
                    writeln!(out, "{} path(s), {} distinct ASNs",
                             MrtRibEntry::count_paths(&entries), distinct_asns.len() as u64)?
                },
                OutputFormat::Cisco => {
                    cisco_show_ip_bgp_detail(out, &prefix, plen, entries)?;
                    writeln!(out, "{} path(s), {} distinct ASNs",
                             MrtRibEntry::count_paths(&entries), distinct_asns.len() as u64)?

                }
                OutputFormat::Csv => {
                    csv_show_route(out, &prefix, plen, entries)?
                },
            }
        }
        None => writeln!(out, "% Not found: {}", prefix.to_string())?
    };
    out.flush()?;
    Ok(())

}

/// `show route community NNNN:NNNN` / `NNNN:NNNN:NNNN`.
/// Use the Filter framework to check for a given BGP community attribute.
pub fn show_route_community(
    table: &Table,
    format: OutputFormat,
    community: &Community,
    out: &mut dyn Write,
) -> io::Result<()> {
    let mut count_prefix_v4: u64 = 0;
    let mut count_prefix_v6: u64 = 0;
    let mut distinct_asns = HashSet::<u32>::new();

    match format {
        OutputFormat::Juniper => {
            juniper_show_route_header(out, "inet.0", 0u64, 0u64)?;
        },
        OutputFormat::Cisco => {
            cisco_show_ip_bgp_header(out, 0, IpAddr::V4(Ipv4Addr::UNSPECIFIED), &String::from(
                "bfort-default"
            ))?;
        },
        OutputFormat::Csv => {
            csv_show_route_header(out)?;
        }
    }

    let filter = Filter::Community(community.clone());

    for (prefix, plen, entries) in table.iter() {

        // We have to copy the RIB entries here because the filter may modify them
        // For the use case of loading into a trie, that's reasonable, but perhaps
        // it's not smart for the case of printing routes to the terminal and
        // counting...
        let mut nlri = MrtNlri {
            sequence: 0,
            plen,
            prefix,
            entry_count: entries.len() as u16,
            rib_entries: entries.to_vec(),      // ouch!
        };
        if filter.eval(&mut nlri) {
            if prefix.is_ipv4() {
                count_prefix_v4 += 1;
            } else if prefix.is_ipv6() {
                count_prefix_v6 += 1;
            }
            MrtRibEntry::count_distinct_asns(&mut distinct_asns, entries);
            match format {
                OutputFormat::Juniper => {
                    juniper_show_route(out, &prefix, plen, &nlri.rib_entries)?;
                },
                OutputFormat::Cisco => {
                    cisco_show_ip_bgp(out, &prefix, plen, &nlri.rib_entries)?;
                },
                OutputFormat::Csv => {
                    csv_show_route(out, &prefix, plen, &nlri.rib_entries)?;
                },
            }
        }
    }

    if format != OutputFormat::Csv {
        writeln!(out, "{} IPv4 route(s), {} IPv6 routes(s), {} distinct ASNs",
                 count_prefix_v4, count_prefix_v6, distinct_asns.len())?;
    }

    Ok(())
}

/// `show route community <regex>` - matched against each community's text form.
/// Manual filter to render the community attributes to a string and check whether
/// a regular expression matches
pub fn show_route_community_regex(
    table: &Table,
    format: OutputFormat,
    re: &Regex,
    out: &mut dyn Write,
) -> io::Result<()> {

    let mut count_prefix_v4: u64 = 0;
    let mut count_prefix_v6: u64 = 0;
    let mut distinct_asns = HashSet::<u32>::new();

    match format {
        OutputFormat::Juniper => {
            juniper_show_route_header(out, "inet.0", 0u64, 0u64)?;
        },
        OutputFormat::Cisco => {
            cisco_show_ip_bgp_header(out, 0, IpAddr::V4(Ipv4Addr::UNSPECIFIED), &String::from(
                "bfort-default"
            ))?;
        },
        OutputFormat::Csv => {
            csv_show_route_header(out)?;
        }
    }

    let filter = Filter::CommunityRegex(re.clone());

    for (prefix, plen, entries) in table.iter() {
        let mut nlri = MrtNlri {
            sequence: 0,
            plen,
            prefix,
            entry_count: entries.len() as u16,
            rib_entries: entries.to_vec(),      // ouch!
        };
        if filter.eval(&mut nlri) {
            if prefix.is_ipv4() {
                count_prefix_v4 += 1;
            } else if prefix.is_ipv6() {
                count_prefix_v6 += 1;
            }
            MrtRibEntry::count_distinct_asns(&mut distinct_asns, entries);
            match format {
                OutputFormat::Juniper => {
                    juniper_show_route(out, &prefix, plen, &nlri.rib_entries)?;
                },
                OutputFormat::Cisco => {
                    cisco_show_ip_bgp(out, &prefix, plen, &nlri.rib_entries)?;
                },
                OutputFormat::Csv => {
                    csv_show_route(out, &prefix, plen, &nlri.rib_entries)?;
                },
            }
        }
    }

    if format != OutputFormat::Csv {
        writeln!(out, "{} IPv4 route(s), {} IPv6 routes(s), {} distinct ASNs",
                 count_prefix_v4, count_prefix_v6, distinct_asns.len())?;
    }

    Ok(())
}

/// `show route as-path "<regex>"` - matched against the AS path text form.
pub fn show_route_as_path_regex(
    table: &Table,
    format: OutputFormat,
    re: &Regex,
    re_modified: bool,
    out: &mut dyn Write,
) -> io::Result<()> {

    let mut count_prefix_v4: u64 = 0;
    let mut count_prefix_v6: u64 = 0;
    let mut distinct_asns = HashSet::<u32>::new();

    match format {
        OutputFormat::Juniper => {
            if re_modified {
                writeln!(out, "Warning: AS Path regular expression rewritten (_) -> (^| |$)")?;
            }
            juniper_show_route_header(out, "inet.0", 0u64, 0u64)?;
        },
        OutputFormat::Cisco => {
            if re_modified {
                writeln!(out, "Warning: AS Path regular expression rewritten (_) -> (^| |$)")?;
            }
            cisco_show_ip_bgp_header(out, 0, IpAddr::V4(Ipv4Addr::UNSPECIFIED), &String::from(
                "bfort-default"
            ))?;
        },
        OutputFormat::Csv => {
            csv_show_route_header(out)?;
        }
    }

    let filter = Filter::AsPathRegex(re.clone());

    for (prefix, plen, entries) in table.iter() {
        let mut nlri = MrtNlri {
            sequence: 0,
            plen,
            prefix,
            entry_count: entries.len() as u16,
            rib_entries: entries.to_vec(),      // ouch!
        };
        if filter.eval(&mut nlri) {
            if prefix.is_ipv4() {
                count_prefix_v4 += 1;
            } else if prefix.is_ipv6() {
                count_prefix_v6 += 1;
            }
            MrtRibEntry::count_distinct_asns(&mut distinct_asns, entries);
            match format {
                OutputFormat::Juniper => {
                    juniper_show_route(out, &prefix, plen, &nlri.rib_entries)?;
                },
                OutputFormat::Cisco => {
                    cisco_show_ip_bgp(out, &prefix, plen, &nlri.rib_entries)?;
                },
                OutputFormat::Csv => {
                    csv_show_route(out, &prefix, plen, &nlri.rib_entries)?;
                },
            }
        }
    }

    if format != OutputFormat::Csv {
        writeln!(out, "{} IPv4 route(s), {} IPv6 routes(s), {} distinct ASNs",
                 count_prefix_v4, count_prefix_v6, distinct_asns.len())?;
    }


    Ok(())
}
