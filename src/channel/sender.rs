use std::hash::Hash;
use std::io;
use std::io::{Error, ErrorKind};
use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use crossbeam::atomic::AtomicCell;
use crossbeam_skiplist::SkipMap;
use dashmap::DashMap;
use parking_lot::RwLock;
use crate::channel::{DEFAULT_TOKEN_INDEX, Route, Status};

pub struct Sender<ID> {
    pub(crate) src_default_udp: UdpSocket,
    pub(crate) share_info: Arc<RwLock<Vec<UdpSocket>>>,
    pub(crate) direct_route_table: Arc<DashMap<ID, Route>>,
    pub(crate) direct_route_table_time: Arc<SkipMap<Route, (ID, AtomicI64, AtomicI64)>>,
    pub(crate) status: Arc<AtomicCell<Status>>,
}

impl<ID> Sender<ID> {
    /// 当前是否是锥形网络
    pub fn is_clone(&self) -> bool {
        self.status.load() == Status::Cone
    }
    pub fn try_clone(&self) -> io::Result<Sender<ID>> {
        Ok(Sender {
            status: self.status.clone(),
            src_default_udp: self.src_default_udp.try_clone()?,
            share_info: self.share_info.clone(),
            direct_route_table: self.direct_route_table.clone(),
            direct_route_table_time: self.direct_route_table_time.clone(),
        })
    }
    /// 发送到指定路由
    pub fn send_to_route(&self, buf: &[u8], route: &Route) -> io::Result<usize> {
        if let Some(time) = self.direct_route_table_time.get(route) {
            time.value().2.store(chrono::Local::now().timestamp_millis(), Ordering::Relaxed);
        }
        if route.index == DEFAULT_TOKEN_INDEX {
            self.src_default_udp.send_to(buf, route.addr)
        } else {
            if let Some(udp) = self.share_info.read().get(route.index) {
                udp.send_to(buf, route.addr)
            } else {
                Err(Error::new(ErrorKind::Other, "not fount"))
            }
        }
    }
    /// 发送到指定地址，将使用默认udpSocket发送
    pub fn send_to_addr(&self, buf: &[u8], addr: SocketAddr) -> io::Result<usize> {
        self.src_default_udp.send_to(buf, addr)
    }
    pub fn send_to_addr_all(&self, buf: &[u8], addr: SocketAddr) -> io::Result<()> {
        self.src_default_udp.send_to(buf, addr)?;
        for udp in self.share_info.read().iter() {
            udp.send_to(buf, addr)?;
        }
        Ok(())
    }
}

impl<ID: Eq + Hash + Clone> Sender<ID> {
    /// 发送到指定id
    pub fn send_to_id(&self, buf: &[u8], id: &ID) -> io::Result<usize> {
        match self.direct_route_table.get(id) {
            None => {
                Err(Error::new(ErrorKind::Other, "not fount route"))
            }
            Some(e) => {
                self.send_to_route(buf, e.value())
            }
        }
    }
}
