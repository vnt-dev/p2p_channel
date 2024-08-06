use clap::Parser;
use ctrlc2;
use env_logger::Env;
use log::{debug, info};
use std::net::{IpAddr, SocketAddrV4};
use std::sync::mpsc;

use p2p_channel::{channel::Route, punch::NatType};

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
/*
0: 对端打洞报文 |0u8|peer_id:u32|
1: 对端数据报文 |1u8|peer_id:u32| data|

252: 服务器确认上报的信息 |252u8|
253: 上报服务器信息 |253u8|my_peer_id:u32|to_peer_id:u32|
254: 服务器协调消息 |254u8|peer_id:u32|peer_addr:String|
255: 心跳 |255u8|
 */

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// 服务器地址
    #[arg(short, long)]
    server: String,

    /// 身份号
    #[arg(short, long)]
    id: u32,

    /// 对方身份号
    #[arg(short, long)]
    peer_id: u32,
}
fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    let Args {
        server,
        id: my_peer_id,
        peer_id,
    } = Args::parse();
    let (mut channel, mut punch, idle) =
        p2p_channel::boot::Boot::new::<String>(20, 9000, 0).unwrap();
    //channel.set_nat_type(NatType::Cone).unwrap();  //一定要根据本地环境的Nat网络类型设置
    let nat = channel
        .set_nat_type_with_stun(
            vec![
                "stun.miwifi.com:3478".to_string(),
                "stun.chat.bilibili.com:3478".to_string(),
                "stun.hitv.com:3478".to_string(),
                "stun.cdnbye.com:3478".to_string(),
                "stun.tel.lu:3478".to_string(),
                "stun.smartvoip.com:3478".to_string(),
            ],
            None,
        )
        .unwrap();
    info!("nat info: {:?}", nat);
    let (tx, sig_rx) = mpsc::channel();
    let handle = ctrlc2::set_handler(move || {
        tx.send(()).expect("Could not send signal on channel.");
        true
    })
    .expect("Error setting Ctrl-C handler");

    let channel_route = channel.try_clone().unwrap();
    let idle_thr = {
        // 空闲处理，添加的路由空闲时触发
        std::thread::spawn(move || {
            loop {
                let (idle_status, id, _route) = match idle.next_idle() {
                    Ok(r) => r,
                    Err(_) => {
                        info!("idle task exit {}", line!());
                        break;
                    }
                };
                // channel.send_to_route()
                channel_route.remove_route(&id[0]);
                info!("idle {:?} {}", idle_status, &id[0]);
            }
        })
    };
    let (punch_cone_thr, puch_sym_thr) = {
        //let ident = args[1].clone();
        //let ident2 = ident.clone();
        // 打洞处理
        let mut punch2 = punch.try_clone().unwrap();
        (
            std::thread::spawn(move || {
                let mut buf = vec![0u8];
                buf.extend_from_slice(my_peer_id.to_be_bytes().as_slice());
                loop {
                    let (id, nat_info) = match punch.next_cone(None) {
                        Ok(r) => r,
                        Err(_) => {
                            break;
                        }
                    };
                    info!("{id} -> {nat_info:?}");
                    match punch.punch(&buf[..], id, nat_info) {
                        Ok(_) => {}
                        Err(_) => {
                            break;
                        }
                    }
                }
                info!("cone punch task exit {}", line!());
            }),
            std::thread::spawn(move || {
                let mut buf = vec![0u8];
                buf.extend_from_slice(my_peer_id.to_be_bytes().as_slice());
                loop {
                    let (id, nat_info) = match punch2.next_symmetric(None) {
                        Ok(r) => r,
                        Err(_) => {
                            break;
                        }
                    };
                    info!("{id} -> {nat_info:?}");
                    match punch2.punch(&buf[..], id, nat_info) {
                        Ok(_) => {}
                        Err(_) => {
                            break;
                        }
                    }
                }
                info!("symmetric punch task exit {}", line!());
            }),
        )
    };

    let mut buf = [0; 1500];

    let (tx, rx) = std::sync::mpsc::channel::<(IpAddr, u16, u32)>();
    let chanel2 = channel.try_clone().unwrap();
    let chanel3 = channel.try_clone().unwrap();
    let _report_and_heart_thr = {
        //上报服务器
        let mut bytes = vec![253u8];
        bytes.extend_from_slice(my_peer_id.to_be_bytes().as_slice());
        bytes.extend_from_slice(peer_id.to_be_bytes().as_slice());
        channel
            .send_to_addr(&bytes, server.parse().unwrap())
            .unwrap();
        match channel.recv_from(&mut buf, Some(std::time::Duration::from_secs(5))) {
            Ok((len, _)) => {
                if len != 1 || buf[0] != 252 {
                    panic!("report server unknow error!!!");
                }
                info!("上报中继服务器成功");
            }
            Err(e) => {
                panic!("report server error!!! {e:?}");
            }
        }
        //持续发送心跳
        let chanel1 = channel.try_clone().unwrap();
        std::thread::spawn(move || loop {
            if let Err(_) = chanel1.send_to_addr(&[255u8], "150.158.95.11:3000".parse().unwrap()) {
                info!("与服务心跳包任务结束");
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5000));
        })
    };
    // Do something...
    let recev_punch_thr = std::thread::spawn(move || {
        let mut threads_handler = vec![];
        while let Ok((public_ip, public_port, peer_id)) = rx.recv() {
            let channel = chanel2.try_clone().unwrap();
            threads_handler.push(std::thread::spawn(move || {
                let peer_id = peer_id.to_string();
                loop {
                    if let Err(_) = channel.punch(
                        peer_id.clone(),
                        p2p_channel::punch::NatInfo::new(
                            vec![public_ip],
                            public_port,
                            0,
                            public_ip,
                            public_port,
                            NatType::Cone, //对方的Nat网络类型
                        ),
                    ) {
                        break;
                    }; //触发打洞
                    if let Err(_) = channel.punch(
                        peer_id.clone(),
                        p2p_channel::punch::NatInfo::new(
                            vec![public_ip],
                            public_port,
                            0,
                            public_ip,
                            public_port,
                            NatType::Symmetric, //对方的Nat网络类型
                        ),
                    ) {
                        break;
                    }; //触发打洞
                    std::thread::sleep(std::time::Duration::from_millis(5000));
                }
            }));
        }
        for j in threads_handler {
            _ = j.join();
        }
        info!("trigger punch task exit {}", line!());
    });

    let receive_thr = std::thread::spawn(move || {
        let mut status = false;
        // 接收数据处理
        loop {
            let (len, route_key) = match channel.recv_from(&mut buf, None) {
                Ok(r) => r,
                Err(_) => {
                    break;
                }
            };
            if len > 0 && (buf[0] == 255 || buf[0] == 252) {
                //心跳或服务器确认
                continue;
            };
            if len > 0 && buf[0] == 254 {
                //服务器协商客户端
                let peer_id = bytes_to_u32(&buf[1..5]).unwrap();
                let peer_addr = String::from_utf8_lossy(&buf[5..len]).to_string();
                let s: SocketAddrV4 = peer_addr.parse().unwrap();
                let public_ip = std::net::IpAddr::V4(s.ip().to_owned());
                let public_port = s.port();
                tx.send((public_ip, public_port, peer_id)).unwrap();
                continue;
            }
            assert!(len >= 5, "len must be greater than 5");
            let remote_peer_id = bytes_to_u32(&buf[1..5]).unwrap().to_string();

            if len > 0 && buf[0] == 0 {
                debug!("say hello");
                if channel.route_to_id(&route_key).is_none() {
                    channel.add_route(remote_peer_id.clone(), Route::from(route_key, 10, 64));
                    //超时触发空闲
                }
            } else if len > 0 && buf[0] == 1 {
                if channel.route_to_id(&route_key).is_none() {
                    channel.add_route(remote_peer_id.clone(), Route::from(route_key, 10, 64));
                    //超时触发空闲
                }
                let text = String::from_utf8_lossy(&buf[4..len]).to_string();
                info!("receive {text} {route_key:?}");
            }
            if !status {
                let msg = format!("my identity is {}", my_peer_id);
                let mut msg_p = vec![1u8];
                msg_p.extend_from_slice(my_peer_id.to_be_bytes().as_slice());
                msg_p.extend_from_slice(msg.as_bytes());
                //channel.send_to_route(msg.as_bytes(), &route_key).unwrap();
                //println!("route table {:?}", channel.route_table());
                _ = channel
                    .send_to_id(msg_p.as_slice(), &remote_peer_id)
                    .unwrap();
                //channel.send_to_addr(msg.as_bytes(), addr.parse().unwrap()).unwrap();
                status = true;
            }
        }
        info!("receive data task exit {}", line!());
    });
    sig_rx.recv().expect("Could not receive from channel.");
    info!("exit");
    chanel3.close().unwrap();
    _ = idle_thr.join();
    _ = punch_cone_thr.join();
    _ = puch_sym_thr.join();
    _ = receive_thr.join();
    _ = recev_punch_thr.join();
    _ = _report_and_heart_thr.join();
    handle.join().unwrap();
}
