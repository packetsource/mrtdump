use crate::*;
pub fn cisco_show_ip_bgp_header(version: u32, peers: &Option<crate::peer::MrtPeerIndexTable>) {
    let (collector_id, view_name): (IpAddr, &String) = {
        if let Some(peers) = peers {
            (peers.collector_id, &peers.view_name)
        } else {
            (IpAddr::V4(Ipv4Addr::UNSPECIFIED), &String::from("default"))
        }
    };
    println!("BGP table version is {}, local router ID is {}, view is \"{}\"",
             version, collector_id, view_name);
    println!("Status codes: s suppressed, d damped, h history, * valid, > best, i - internal");
    println!("Origin codes: i - IGP, e - EGP, ? - incomplete");
    println!("");
    println!("  {:24}{:24}\t{} {} {} {}",
             "Network",
             "Next Hop",
             "Metric",
             "LocPrf",
             "Weight",
             "Path"
    );
}
pub fn cisco_show_ip_bgp(_peers: &Option<MrtPeerIndexTable>, route: &MrtNlri) {
    let mut count: u64 = 0;
    for rt in &route.rib_entries {
        if count==0 {
            println!("* {:24}{:24}\t{}\t{}\t{}\t{} {}",
                     format!("{}/{}", route.prefix, route.plen),
                     rt.get_nexthop(),
                     rt.get_med().map(|x| x.to_string()).unwrap_or(String::new()),
                     rt.get_local_pref().unwrap_or(DEFAULT_LOCAL_PREF),
                     CISCO_DEFAULT_WEIGHT,
                     rt.get_aspath(),
                    rt.get_origin_char()
            );
        } else {
            println!("* {:24}{:24}\t{}\t{}\t{}\t{} {}",
                     String::new(),
                     rt.get_nexthop(),
                     rt.get_med().map(|x| x.to_string()).unwrap_or(String::new()),
                     rt.get_local_pref().unwrap_or(DEFAULT_LOCAL_PREF),
                     CISCO_DEFAULT_WEIGHT,
                     rt.get_aspath(),
                     rt.get_origin_char()
            );
        }
        count += 1;
    }
}

pub fn cisco_show_ip_bgp_detail(peers: &Option<MrtPeerIndexTable>, route: &MrtNlri) {
    let peers = peers.as_ref().unwrap();
    println!("BGP routing table entry for {}/{}", route.prefix, route.plen);
    println!("Paths: ({} available)", route.rib_entries.len());
    println!("  Not advertised to any peer");   // standard Cisco gubbins

    for rt in &route.rib_entries {
        println!("  {}", rt.get_aspath());
        println!("    {} from {} ({})",
                 rt.get_nexthop(),
                 &peers[rt.peer_id as usize].peer_address,
                 &peers[rt.peer_id as usize].peer_id);

        let mut rt_text = Vec::<String>::new();
        rt_text.push(format!("Origin {}", match rt.get_origin() {
            0 => "IGP",
            1 => "EGP",
            2 => "Incomplete",
            _ => "Unknown"
        }));
        if let Some(med) = rt.get_med() {
            rt_text.push(format!("metric {}", med));
        }
        rt_text.push(format!("localpref {}", rt.get_local_pref().unwrap_or(DEFAULT_LOCAL_PREF)));

        // More standard Cisco gubbins
        rt_text.push(String::from("weight 32768"));
        rt_text.push(String::from("valid"));

        println!("      {}", rt_text.join(", "));
        if let Some(community) = rt.get_community() {
            println!("      Community: {}", &community);
        }
    }
}


pub fn juniper_show_route(peers: &Option<MrtPeerIndexTable>, route: &MrtNlri) {
    let mut count: u64 = 0;
    for rt in &route.rib_entries {
        let age = rt.origin_time.elapsed().unwrap_or_default();
        let mut rt_text:Vec<String> = vec![format!("[BGP/170] {}", util::friendly_duration(age))];
        if let Some(med) = rt.get_med() {
            rt_text.push(format!("MED {}", med));
        }
        rt_text.push(format!("localpref {}", rt.get_local_pref().unwrap_or(DEFAULT_LOCAL_PREF)));
        if let Some(peers) = peers {
            rt_text.push(format!("from {}", peers[rt.peer_id as usize].peer_address.to_string()));
        }

        if count==0 {
            println!("{}/{}\t{}",
                     route.prefix,
                     route.plen, rt_text.join(", ")
            );
        } else {
            println!("\t\t{}",
                     rt_text.join(", ")
            );
        }
        println!("\t\t AS path: {} {}", rt.get_aspath(), rt.get_origin_char());
        if let Some(communities) = rt.get_community() {
            println!("\t\t Communities: {}", &communities);
        }

        println!("\t\t> to {}", rt.get_nexthop());
        count += 1;
    }
}

pub fn csv_show_route(peers: &Option<MrtPeerIndexTable>, route: &MrtNlri) {
    let peers = peers.as_ref().unwrap();

    println!("route/plen|neighbor|next_hop|med|localpref|aspath|communities");

    for rt in &route.rib_entries {
        println!("{}/{}|{}|{}|{}|{}|{} {}|{}",
            route.prefix, route.plen,
            &peers[rt.peer_id as usize].peer_address,
            rt.get_nexthop(),
            rt.get_med().map_or(String::from(""), |x| x.to_string()),
            rt.get_local_pref().unwrap_or(DEFAULT_LOCAL_PREF),
            rt.get_aspath(), rt.get_origin_char(),
            rt.get_community().unwrap_or(String::from(""))
        );
    }
}
