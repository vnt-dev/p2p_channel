use std::{collections::HashMap, net::UdpSocket};

use env_logger::Env;
use log::info;

fn bytes_to_u32(b: &[u8]) -> Option<u32> {
    if b.len() != 4 {
        return None;
    }
    let mut bytes = [0u8; 4];
    unsafe {
        std::ptr::copy_nonoverlapping(b.as_ptr(), bytes.as_mut_ptr(), 4);
    }
    Some(u32::from_be_bytes(bytes))
}
fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    let udp = UdpSocket::bind("0.0.0.0:3000").unwrap();
    let mut map = HashMap::new();
    let mut buf = [0u8; 1500];
    while let Ok((size, from)) = udp.recv_from(&mut buf) {
        if size > 0 && buf[0] == 253 {
            let Some(from_id) = bytes_to_u32(&buf[1..5]) else {
                continue;
            };
            let Some(peer_id) = bytes_to_u32(&buf[5..9]) else {
                continue;
            };
            udp.send_to(&[252u8], from).unwrap();
            map.insert(from_id, from);
            if let Some(v) = map.get(&peer_id) {
                info!("pair {v} with {from}");
                let mut for_from = vec![254u8];
                for_from.extend_from_slice(peer_id.to_be_bytes().as_slice());
                for_from.extend_from_slice(v.to_string().as_bytes());
                udp.send_to(&for_from, from).unwrap();

                let mut for_peer = vec![254u8];
                for_peer.extend_from_slice(from_id.to_be_bytes().as_slice());
                for_peer.extend_from_slice(from.to_string().as_bytes());
                udp.send_to(&for_peer, v).unwrap();
                map.remove(&peer_id);
                map.remove(&from_id);
            }
        } else if size > 0 && buf[0] == 255 {
            if let Err(_) = udp.send_to(&[255u8], from) {
                if let Some(v) = map.iter().find(|i| i.1 == &from) {
                    let key = *v.0;
                    map.remove(&key);
                }
            }
        }
    }
}
