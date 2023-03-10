use std::hash::Hash;
use std::io;
use std::io::{Error, ErrorKind};
use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use crossbeam::atomic::AtomicCell;
use crossbeam::sync::Unparker;
use crossbeam_skiplist::SkipMap;
use dashmap::DashMap;
use parking_lot::{Mutex, RwLock};

use crate::channel::{DEFAULT_TOKEN_INDEX, Route, RouteKey, Status};

pub struct Sender<ID> {
    pub(crate) src_default_udp: UdpSocket,
    pub(crate) share_info: Arc<RwLock<Vec<UdpSocket>>>,
    pub(crate) direct_route_table: Arc<DashMap<ID, Route>>,
    pub(crate) direct_route_table_time: Arc<SkipMap<RouteKey, (ID, AtomicI64, AtomicI64)>>,
    pub(crate) status: Arc<AtomicCell<Status>>,
    pub(crate) un_parker: Unparker,
    pub(crate) lock: Arc<Mutex<()>>,
}

impl<ID> Sender<ID> {
    pub fn is_close(&self) -> bool {
        self.status.load() == Status::Close
    }
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
            un_parker: self.un_parker.clone(),
            lock: self.lock.clone(),
        })
    }
    /// 发送到指定路由
    pub fn send_to_route(&self, buf: &[u8], route_key: &RouteKey) -> io::Result<usize> {
        if let Some(time) = self.direct_route_table_time.get(route_key) {
            time.value().2.store(chrono::Local::now().timestamp_millis(), Ordering::Relaxed);
        }
        if route_key.index == DEFAULT_TOKEN_INDEX {
            self.src_default_udp.send_to(buf, route_key.addr)
        } else {
            if let Some(udp) = self.share_info.read().get(route_key.index) {
                udp.send_to(buf, route_key.addr)
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
                self.send_to_route(buf, &e.value().route_key())
            }
        }
    }
}

impl<ID: Hash + Eq + Clone + Send + 'static> Sender<ID> {
    /// 添加路由
    pub fn add_route(&self, id: ID, route: Route) {
        let lock = self.lock.lock();
        let time = chrono::Local::now().timestamp_millis();
        self.direct_route_table_time.insert(route.route_key(), (id.clone(), AtomicI64::new(time), AtomicI64::new(time)));
        let route_key = route.route_key();
        if let Some(old) = self.direct_route_table.insert(id, route) {
            if old.route_key() != route_key {
                self.direct_route_table_time.remove(&old.route_key());
            }
        }
        drop(lock);
        self.un_parker.unpark();
    }
    pub fn update_route(&self, id: &ID, metric: u8, rt: i64) {
        if let Some(mut v) = self.direct_route_table.get_mut(id) {
            v.metric = metric;
            v.rt = rt;
        }
    }
    /// 查询路由
    pub fn route(&self, id: &ID) -> Option<Route> {
        self.direct_route_table.get(id).map(|e| *e.value())
    }
    /// 删除路由
    pub fn remove_route(&self, id: &ID) {
        let lock = self.lock.lock();
        if let Some((_, route)) = self.direct_route_table.remove(id) {
            self.direct_route_table_time.remove(&route.route_key());
        }
        drop(lock);
    }
    pub fn route_to_id(&self, route_key: &RouteKey) -> Option<ID> {
        if let Some(v) = self.direct_route_table_time.get(route_key) {
            Some(v.value().0.clone())
        } else {
            None
        }
    }
    pub fn route_list(&self) -> Vec<(ID, Route)> {
        self.direct_route_table.iter().map(|k| (k.key().clone(), k.value().clone())).collect()
    }
}
