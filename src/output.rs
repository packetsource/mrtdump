use std::io::Write;
use crate::*;

// Equivalent for Juniper is something like:
// inet.0: 1066108 destinations, 8538665 routes (1065578 active, 1 holddown, 78503 hidden)
pub fn cisco_show_ip_bgp_header(writer: &mut dyn Write,
                                version: u32,
                                collector_id: IpAddr,
                                view_name: &String) -> std::io::Result<()> {
    writeln!(writer, "BGP table version is {}, local router ID is {}, view is \"{}\"",
             version, collector_id, view_name)?;
    writeln!(writer, "Status codes: s suppressed, d damped, h history, * valid, > best, i - internal")?;
    writeln!(writer, "Origin codes: i - IGP, e - EGP, ? - incomplete")?;
    writeln!(writer, "")?;
    writeln!(writer, "  {:24}{:24}\t{} {} {} {}",
             "Network",
             "Next Hop",
             "Metric",
             "LocPrf",
             "Weight",
             "Path"
    )?;
    Ok(())
}

// Prefix all use of this with the header.
pub fn cisco_show_ip_bgp(writer: &mut dyn Write,
                         prefix: &IpAddr,
                         plen: u8,
                         route_entries: &Vec<MrtRibEntry>) -> std::io::Result<()> {
    let mut count: u64 = 0;
    for rt in route_entries {
        if count==0 {
            writeln!(writer, "* {:24}{:24}\t{}\t{}\t{}\t{} {}",
                     format!("{}/{}", prefix, plen),
                     rt.get_nexthop(),
                     rt.get_med().map(|x| x.to_string()).unwrap_or(String::new()),
                     rt.get_local_pref().unwrap_or(DEFAULT_LOCAL_PREF),
                     CISCO_DEFAULT_WEIGHT,
                     rt.get_aspath().unwrap_or(&AsPath::default()).to_string(),
                    rt.get_origin_char()
            )?;
        } else {
            writeln!(writer, "* {:24}{:24}\t{}\t{}\t{}\t{} {}",
                     String::new(),
                     rt.get_nexthop(),
                     rt.get_med().map(|x| x.to_string()).unwrap_or(String::new()),
                     rt.get_local_pref().unwrap_or(DEFAULT_LOCAL_PREF),
                     CISCO_DEFAULT_WEIGHT,
                     rt.get_aspath().unwrap_or(&AsPath::default()).to_string(),
                     rt.get_origin_char()
            )?;
        }
        count += 1;
    }
    Ok(())
}

// Don't use the header with this, it's not a table.
pub fn cisco_show_ip_bgp_detail(writer: &mut dyn Write,
                                prefix: &IpAddr,
                                plen: u8,
                                route_entries: &Vec<MrtRibEntry>) -> std::io::Result<()> {

    writeln!(writer, "BGP routing table entry for {}/{}", prefix, plen)?;
    writeln!(writer, "Paths: ({} available)", route_entries.len())?;
    writeln!(writer, "  Not advertised to any peer")?;

    for rt in route_entries {
        writeln!(writer, "  {}", rt.get_aspath().unwrap_or(&AsPath::default()).to_string())?;
        writeln!(writer, "    {} from {} ({})",
                 rt.get_nexthop(),
                 &rt.peer.peer_address,
                 &rt.peer.peer_id)?;

        let mut rt_text = Vec::<String>::new();
        rt_text.push(format!("Origin {}", match rt.get_origin() {
            0 => "IGP",
            1 => "EGP",
            2 => "incomplete",
            _ => "unknown"
        }));
        if let Some(med) = rt.get_med() {
            rt_text.push(format!("metric {}", med));
        }
        rt_text.push(format!("localpref {}", rt.get_local_pref().unwrap_or(DEFAULT_LOCAL_PREF)));
        rt_text.push(String::from("weight 32768"));
        rt_text.push(String::from("valid"));

        writeln!(writer, "      {}", rt_text.join(", "))?;
        if let Some(community) = rt.get_community() {
            writeln!(writer, "      Community: {}", &community)?;
        }
    }
    Ok(())
}

pub fn juniper_show_route_header(writer: &mut dyn Write,
                                table_name: &str,
                                 num_destinations: u64,
                                 num_routes: u64) -> std::io::Result<()> {
    writeln!(writer, "{}: {} destinations, {} routes ({} active, 0 holddown, 0 hidden)",
        table_name, num_destinations, num_routes, num_routes)?;
    writeln!(writer, "+ = Active Route, - = Last Active, * = Both")?;
    writeln!(writer, "")?;

    Ok(())
}

pub fn juniper_show_route(writer: &mut dyn Write,
                          prefix: &IpAddr,
                          plen: u8,
                          route_entries: &Vec<MrtRibEntry>) -> std::io::Result<()> {
    let mut count: u64 = 0;
    for rt in route_entries {
        let age = rt.origin_time.elapsed().unwrap_or_default();
        let mut rt_text:Vec<String> = vec![format!("*[BGP/170] {}", util::friendly_duration(age))];
        if let Some(med) = rt.get_med() {
            rt_text.push(format!("MED {}", med));
        }
        rt_text.push(format!("localpref {}", rt.get_local_pref().unwrap_or(DEFAULT_LOCAL_PREF)));
        rt_text.push(format!("from {}", rt.peer.peer_address));

        if count==0 {
            writeln!(writer, "{}/{}\t\t{}",
                     prefix,
                     plen, rt_text.join(", ")
            )?;
        } else {
            writeln!(writer, "\t\t\t{}",
                     rt_text.join(", ")
            )?;
        }
        writeln!(writer, "\t\t\tAS path: {} {}",
                 rt.get_aspath().unwrap_or(&AsPath::default()).to_string(),
                 rt.get_origin_char())?;
        if let Some(communities) = rt.get_community() {
            writeln!(writer, "\t\t\tCommunities: {}", &communities)?;
        }

        writeln!(writer, "\t\t\t> to {}", rt.get_nexthop())?;
        count += 1;
    }
    Ok(())
}

pub fn csv_show_route_header(writer: &mut dyn Write) -> std::io::Result<()> {
    writeln!(writer, "route/plen|neighbor|next_hop|med|localpref|aspath|communities")?;
    Ok(())
}

pub fn csv_show_route(writer: &mut dyn Write,
                      prefix: &IpAddr,
                      plen: u8,
                      route_entries: &Vec<MrtRibEntry>) -> std::io::Result<()> {

    for rt in route_entries {
        writeln!(writer, "{}/{}|{}|{}|{}|{}|{} {}|{}",
            prefix, plen,
            rt.peer.peer_address,
            rt.get_nexthop(),
            rt.get_med().map_or(String::from(""), |x| x.to_string()),
            rt.get_local_pref().unwrap_or(DEFAULT_LOCAL_PREF),
            rt.get_aspath().unwrap_or(&AsPath::default()).to_string(), rt.get_origin_char(),
            rt.get_community().unwrap_or(String::from(""))
        )?;
    }
    Ok(())
}
