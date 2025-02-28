use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;

use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use netlink_packet_core::NetlinkPayload;
use netlink_packet_route::RouteNetlinkMessage;
use netlink_packet_route::neighbour::NeighbourMessage;
use netlink_packet_route::neighbour::{NeighbourAddress, NeighbourAttribute, NeighbourState};
use netlink_packet_route::route::RouteType;
use rtnetlink::{Error, Handle, new_connection};

use clap::Parser;
use netlink_sys::{AsyncSocket, SocketAddr};
mod db;
mod op;
const RTNLGRP_NEIGH: u32 = 3;

const fn nl_mgrp(group: u32) -> u32 {
    if group > 31 {
        panic!("use netlink_sys::Socket::add_membership() for this group");
    }
    if group == 0 { 0 } else { 1 << (group - 1) }
}

#[derive(Debug, Parser)]
#[clap()]
struct Cli {
    #[clap(short, long)]
    iface: Option<String>,
    #[clap(short, long)]
    private_subnet: bool,
    #[clap(short, long)]
    sqlite: Option<PathBuf>,
}

#[derive(Debug)]
struct Neigh {
    ifindex: u32,
    state: NeighbourState,
    kind: RouteType,
    inet: NeighbourAddress,
    mac: String,
}

fn if_ipv6_in_private_subnet(ip: &Ipv6Addr) -> bool {
    // Check if address is ULA (fc00::/7)
    let is_ula = (ip.segments()[0] & 0xfe00) == 0xfc00;

    // Check if address is link-local (fe80::/10)
    let is_link_local = (ip.segments()[0] & 0xffc0) == 0xfe80;

    is_ula || is_link_local
}

fn if_ipv4_in_private_subnet(ip: &Ipv4Addr) -> bool {
    // Check for private network ranges
    let octets = ip.octets();

    // 10.0.0.0/8
    if octets[0] == 10 {
        return true;
    }

    // 172.16.0.0/12
    if octets[0] == 172 && (octets[1] >= 16 && octets[1] <= 31) {
        return true;
    }

    // 192.168.0.0/16
    if octets[0] == 192 && octets[1] == 168 {
        return true;
    }

    // 127.0.0.0/8 (loopback)
    if octets[0] == 127 {
        return true;
    }

    false
}

fn process_new_neigh(neigh: Neigh, db: &db::SqlitePersistence) {
    println!("New neighbour: {:?}", neigh);
}

fn process_del_neigh(neigh: Neigh, db: &db::SqlitePersistence) {
    println!("Del neighbour: {:?}", neigh);
}

fn is_multicast_or_broadcast_route_type(route_type: RouteType) -> bool {
    match route_type {
        RouteType::Multicast => true,
        RouteType::Broadcast => true,
        _ => false,
    }
}

fn is_multicast_or_broadcast(neigh: &Neigh) -> bool {
    return is_multicast_or_broadcast_route_type(neigh.kind);
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    let args = Cli::parse();
    let private_subnet = args.private_subnet;
    let (connection, handle, _) = new_connection().unwrap();
    tokio::spawn(connection);
    // let link = "eth0".to_string();
    dump_addresses(handle.clone(), args.iface).await.unwrap();
    println!("dumping neighbours");
    if let Ok(neighbours) = dump_neighbours(handle.clone(), private_subnet).await {
        neighbours.iter().for_each(|neigh| {
            println!("{:?}", neigh);
        });
    }
    println!();

    // conn - `Connection` that has a netlink socket which is a `Future` that
    // polls the socket and thus must have an event loop
    //
    // handle - `Handle` to the `Connection`. Used to send/recv netlink
    // messages.
    //
    // messages - A channel receiver.
    let (mut conn, mut _handle, mut messages) =
        new_connection().map_err(|e| format!("{e}")).unwrap();

    // These flags specify what kinds of broadcast messages we want to listen
    // for.
    let groups = nl_mgrp(RTNLGRP_NEIGH);

    let addr = SocketAddr::new(0, groups);
    conn.socket_mut()
        .socket_mut()
        .bind(&addr)
        .expect("Failed to bind");

    // Spawn `Connection` to start polling netlink socket.
    tokio::spawn(conn);
    let db = db::SqlitePersistence::new(args.sqlite.unwrap().as_path());
    // Start receiving events through `messages` channel.
    while let Some((message, _)) = messages.next().await {
        let payload = message.payload;
        if let NetlinkPayload::InnerMessage(msg) = payload {
            match msg {
                RouteNetlinkMessage::NewNeighbour(new_neigh) => {
                    let neigh = parse_neighbour_message(new_neigh, private_subnet);
                    if neigh.is_none() {
                        continue;
                    }
                    println!("New neighbour: {:?}", neigh);
                    process_new_neigh(neigh.unwrap(), &db);
                }
                RouteNetlinkMessage::DelNeighbour(del_neigh) => {
                    let neigh = parse_neighbour_message(del_neigh, private_subnet);
                    if neigh.is_none() {
                        continue;
                    }
                    println!("Del neighbour: {:?}", neigh);
                    process_del_neigh(neigh.unwrap(), &db);
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn format_mac(mac: Vec<u8>) -> String {
    let mut mac_str = String::new();
    for byte in mac {
        mac_str.push_str(&format!("{:02x}:", byte));
    }
    mac_str.pop();
    mac_str
}

async fn dump_addresses(handle: Handle, link: Option<String>) -> Result<(), Error> {
    let mut request = handle.link().get();
    if let Some(link) = link {
        request = request.match_name(link);
    }

    let mut links = request.execute();
    if let Some(link) = links.try_next().await? {
        let mut addresses = handle
            .address()
            .get()
            .set_link_index_filter(link.header.index)
            .execute();
        while let Some(msg) = addresses.try_next().await? {
            println!("{msg:?}");
        }
        Ok(())
    } else {
        eprintln!("link not found");
        Ok(())
    }
}

fn parse_neighbour_message(neigh: NeighbourMessage, private_subnet: bool) -> Option<Neigh> {
    let state = neigh.header.state;
    if state == NeighbourState::Permanent {
        return None;
    }
    let addr: NeighbourAddress = neigh.attributes.iter().find_map(|attr| match attr {
        NeighbourAttribute::Destination(inet) => Some(inet.to_owned()),
        _ => None,
    })?;
    if private_subnet {
        match addr {
            NeighbourAddress::Inet(addr) => {
                if !if_ipv4_in_private_subnet(&addr) {
                    return None;
                }
            }
            NeighbourAddress::Inet6(addr) => {
                if !if_ipv6_in_private_subnet(&addr) {
                    return None;
                }
            }
            _ => {}
        }
    };
    let kind = neigh.header.kind;
    let ifindex = neigh.header.ifindex;
    let mac = neigh.attributes.iter().find_map(|attr| match attr {
        NeighbourAttribute::LinkLocalAddress(mac) => Some(mac.to_owned()),
        _ => None,
    })?;
    Some(Neigh {
        ifindex,
        state,
        kind,
        inet: addr,
        mac: format_mac(mac),
    })
}

async fn dump_neighbours(handle: Handle, private_subnet: bool) -> Result<Vec<Neigh>, Error> {
    let mut neighbours = handle.neighbours().get().execute();
    let mut vec: Vec<Neigh> = Vec::new();
    while let Some(route) = neighbours.try_next().await? {
        if let Some(neigh) = parse_neighbour_message(route, private_subnet) {
            if !is_multicast_or_broadcast(&neigh) {
                vec.push(neigh);
            }
        }
    }
    Ok(vec)
}
