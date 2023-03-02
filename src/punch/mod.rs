use std::collections::HashMap;
use std::hash::Hash;
use std::io;
use std::io::{Error, ErrorKind};
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use crossbeam::channel::{Receiver, RecvError, RecvTimeoutError};
use rand::prelude::SliceRandom;

use crate::channel::sender::Sender;

pub struct Punch<ID> {
    cone_receiver: Receiver<(ID, NatInfo)>,
    symmetric_receiver: Receiver<(ID, NatInfo)>,
    sender: Sender<ID>,
    port_vec: Vec<u16>,
    port_index: HashMap<ID, usize>,
}

impl<ID> Punch<ID> {
    pub fn try_clone(&self) -> io::Result<Punch<ID>> {
        let mut rng = rand::thread_rng();
        let mut port_vec = self.port_vec.clone();
        port_vec.shuffle(&mut rng);
        Ok(Punch {
            cone_receiver: self.cone_receiver.clone(),
            symmetric_receiver: self.symmetric_receiver.clone(),
            sender: self.sender.try_clone()?,
            port_vec,
            port_index: HashMap::new(),
        })
    }
    pub fn new(cone_receiver: Receiver<(ID, NatInfo)>,
               symmetric_receiver: Receiver<(ID, NatInfo)>,
               sender: Sender<ID>, ) -> Punch<ID> {
        let mut port_vec: Vec<u16> = (1..65535).collect();
        port_vec.push(65535);
        let mut rng = rand::thread_rng();
        port_vec.shuffle(&mut rng);
        Punch {
            cone_receiver,
            symmetric_receiver,
            sender,
            port_vec,
            port_index: HashMap::new(),
        }
    }
    /// 接收向对称网络打洞的信息
    pub fn next_symmetric(&self, timeout: Option<Duration>) -> io::Result<(ID, NatInfo)> {
        Punch::next_(&self.symmetric_receiver, timeout)
    }
    /// 接收向锥形网络打洞的信息
    pub fn next_cone(&self, timeout: Option<Duration>) -> io::Result<(ID, NatInfo)> {
        Punch::next_(&self.cone_receiver, timeout)
    }
    fn next_(receiver: &Receiver<(ID, NatInfo)>, timeout: Option<Duration>) -> io::Result<(ID, NatInfo)> {
        let (id, nat_info) = if let Some(timeout) = timeout {
            match receiver.recv_timeout(timeout) {
                Ok(rs) => {
                    rs
                }
                Err(RecvTimeoutError::Timeout) => {
                    return Err(Error::from(ErrorKind::TimedOut));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(Error::new(ErrorKind::Other, "close"));
                }
            }
        } else {
            match receiver.recv() {
                Ok(rs) => {
                    rs
                }
                Err(_) => {
                    return Err(Error::new(ErrorKind::Other, "close"));
                }
            }
        };
        Ok((id, nat_info))
    }
}

impl<ID: Clone + Eq + Hash> Punch<ID> {
    pub fn punch(&mut self, buf: &[u8], id: ID, nat_info: NatInfo) -> io::Result<()> {
        let is_cone = self.sender.is_clone();
        match nat_info.nat_type {
            NatType::Symmetric => {
                // 假设对方绑定n个端口，通过NAT对外映射出n个 公网ip:公网端口，自己随机尝试k次的情况下
                // 猜中的概率 p = 1-((65535-n)/65535)*((65535-n-1)/(65535-1))*...*((65535-n-k+1)/(65535-k+1))
                // n取76，k取600，猜中的概率就超过50%了
                // 前提 自己是锥形网络，否则猜中了也通信不了

                //避免发送时NAT切换成对称网络，造成大量io
                let (max_k1, max_k2) = if is_cone {
                    (60, 800)
                } else {
                    (5, 10)
                };
                if nat_info.public_port_range < max_k1 * 2 {
                    //端口变化不大时，在预测的范围内随机发送
                    let min_port = if nat_info.public_port > nat_info.public_port_range {
                        nat_info.public_port - nat_info.public_port_range
                    } else {
                        1
                    };
                    let (max_port, overflow) = nat_info.public_port.overflowing_add(nat_info.public_port_range);
                    let max_port = if overflow {
                        65535
                    } else {
                        max_port
                    };
                    let k = if max_port - min_port + 1 > max_k1 {
                        max_k1 as usize
                    } else {
                        (max_port - min_port + 1) as usize
                    };
                    let mut nums: Vec<u16> = (min_port..max_port).collect();
                    nums.push(max_port);
                    let mut rng = rand::thread_rng();
                    nums.shuffle(&mut rng);
                    self.punch_(&nums[..k], buf, &nat_info.public_ips, max_k1 as usize, is_cone)?;
                }
                let start = *self.port_index.entry(id.clone()).or_insert(0);
                let mut end = start + max_k2;
                let mut index = end;
                if end >= self.port_vec.len() {
                    end = self.port_vec.len();
                    index = 0
                }
                self.punch_(&self.port_vec[start..end], buf, &nat_info.public_ips, max_k2, is_cone)?;
                self.port_index.insert(id, index);
            }
            NatType::Cone => {
                for ip in nat_info.public_ips {
                    let addr = SocketAddr::new(ip, nat_info.public_port);
                    self.sender.send_to_addr(buf, addr)?;
                }
            }
        }
        Ok(())
    }
    fn punch_(&self, ports: &[u16], buf: &[u8], ips: &Vec<IpAddr>, max: usize, is_cone: bool) -> io::Result<()> {
        let mut count = 0;
        for port in ports {
            for pub_ip in ips {
                count += 1;
                if count == max {
                    return Ok(());
                }
                let addr = SocketAddr::new(*pub_ip, *port);
                if is_cone {
                    self.sender.send_to_addr(buf, addr)?;
                } else {
                    self.sender.send_to_addr_all(buf, addr)?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum NatType {
    Symmetric,
    Cone,
}

#[derive(Clone, Debug)]
pub struct NatInfo {
    public_ips: Vec<IpAddr>,
    public_port: u16,
    public_port_range: u16,
    pub(crate) nat_type: NatType,
}

impl NatInfo {
    pub fn new(public_ips: Vec<IpAddr>,
               public_port: u16,
               public_port_range: u16,
               nat_type: NatType, ) -> Self {
        Self {
            public_ips,
            public_port,
            public_port_range,
            nat_type,
        }
    }
}