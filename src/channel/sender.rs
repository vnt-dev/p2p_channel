use std::hash::Hash;
use std::io;
use std::io::{Error, ErrorKind};
use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

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
    pub(crate) direct_route_table_time: Arc<SkipMap<RouteKey, (Mutex<Vec<ID>>, AtomicI64, AtomicI64)>>,
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
            self.src_default_udp.send_all(buf, route_key.addr)
        } else {
            if let Some(udp) = self.share_info.read().get(route_key.index) {
                udp.send_all(buf, route_key.addr)
            } else {
                Err(Error::new(ErrorKind::Other, "not fount"))
            }
        }
    }
    /// 发送到指定地址，将使用默认udpSocket发送
    pub fn send_to_addr(&self, buf: &[u8], addr: SocketAddr) -> io::Result<usize> {
        self.src_default_udp.send_all(buf, addr)
    }
    pub fn send_to_addr_all(&self, buf: &[u8], addr: SocketAddr) -> io::Result<()> {
        self.src_default_udp.send_all(buf, addr)?;
        let mut count = 0;
        for udp in self.share_info.read().iter() {
            udp.send_all(buf, addr)?;
            count += 1;
            if count & 2 == 2 {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        Ok(())
    }
}

pub(crate) trait SendAll {
    fn send_all(&self, buf: &[u8], addr: SocketAddr) -> io::Result<usize>;
}

impl SendAll for UdpSocket {
    fn send_all(&self, buf: &[u8], addr: SocketAddr) -> io::Result<usize> {
        loop {
            return match self.send_to(buf, addr) {
                Ok(len) => {
                    Ok(len)
                }
                Err(e) => {
                    if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut {
                        std::thread::yield_now();
                        continue;
                    }
                    Err(e)
                }
            };
        }
    }
}

impl SendAll for mio::net::UdpSocket {
    fn send_all(&self, buf: &[u8], addr: SocketAddr) -> io::Result<usize> {
        loop {
            return match self.send_to(buf, addr) {
                Ok(len) => {
                    Ok(len)
                }
                Err(e) => {
                    if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut {
                        std::thread::yield_now();
                        continue;
                    }
                    Err(e)
                }
            };
        }
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
    pub(crate) fn add_route_(id: ID, route: Route, lock: &Mutex<()>,
                             direct_route_table_time: &SkipMap<RouteKey, (Mutex<Vec<ID>>, AtomicI64, AtomicI64)>,
                             direct_route_table: &DashMap<ID, Route>,
                             un_parker: &Unparker) {
        let route_key = route.route_key();
        let lock = lock.lock();
        if let Some(old) = direct_route_table.insert(id.clone(), route) {
            if old.route_key() != route_key {
                Sender::remove_id_route_(&id, &old.route_key(), direct_route_table_time);
            }
        }
        if let Some(entry) = direct_route_table_time.get(&route_key) {
            let mut list = entry.value().0.lock();
            if !list.contains(&id) {
                list.push(id.clone())
            }
        } else {
            let time = chrono::Local::now().timestamp_millis();
            direct_route_table_time.insert(route.route_key(), (Mutex::new(vec![id.clone()]), AtomicI64::new(time), AtomicI64::new(time)));
        }

        drop(lock);
        un_parker.unpark();
    }
    /// 添加路由
    pub fn add_route(&self, id: ID, route: Route) {
        Sender::<ID>::add_route_(id, route, &self.lock, &self.direct_route_table_time, &self.direct_route_table, &self.un_parker);
    }
    pub fn update_route(&self, id: &ID, metric: u8, rt: i64) {
        Sender::<ID>::update_route_(id, metric, rt, &self.direct_route_table)
    }
    pub(crate) fn update_route_(id: &ID, metric: u8, rt: i64, direct_route_table: &DashMap<ID, Route>) {
        if let Some(mut v) = direct_route_table.get_mut(id) {
            v.metric = metric;
            v.rt = rt;
        }
    }
    /// 查询路由
    pub fn route(&self, id: &ID) -> Option<Route> {
        Sender::<ID>::route_(id, &self.direct_route_table)
    }
    pub(crate) fn route_(id: &ID, direct_route_table: &DashMap<ID, Route>) -> Option<Route> {
        direct_route_table.get(id).map(|e| *e.value())
    }
    /// 删除路由
    pub fn remove_route(&self, id: &ID) {
        Sender::<ID>::remove_route_(id, &self.lock, &self.direct_route_table_time, &self.direct_route_table);
    }

    pub(crate) fn remove_route_(id: &ID, lock: &Mutex<()>,
                                direct_route_table_time: &SkipMap<RouteKey, (Mutex<Vec<ID>>, AtomicI64, AtomicI64)>,
                                direct_route_table: &DashMap<ID, Route>, ) {
        let lock = lock.lock();
        if let Some((_, route)) = direct_route_table.remove(id) {
            Sender::<ID>::remove_id_route_(&id, &route.route_key(), direct_route_table_time);
        }
        drop(lock);
    }

    fn remove_id_route_(id: &ID, route_key: &RouteKey,
                        direct_route_table_time: &SkipMap<RouteKey, (Mutex<Vec<ID>>, AtomicI64, AtomicI64)>) {
        if let Some(e) = direct_route_table_time.get(&route_key) {
            let mut ids = e.value().0.lock();
            ids.retain(|v| v != id);
            if ids.is_empty() {
                direct_route_table_time.remove(&route_key);
            }
        }
    }
    pub fn route_to_id(&self, route_key: &RouteKey) -> Option<ID> {
        Sender::<ID>::route_to_id_(route_key, &self.direct_route_table_time)
    }
    pub(crate) fn route_to_id_(route_key: &RouteKey,
                               direct_route_table_time: &SkipMap<RouteKey, (Mutex<Vec<ID>>, AtomicI64, AtomicI64)>) -> Option<ID> {
        if let Some(v) = direct_route_table_time.get(route_key) {
            if let Some(id) = v.value().0.lock().get(0) {
                return Some(id.clone());
            }
        }
        None
    }
    pub fn route_table(&self) -> Vec<(ID, Route)> {
        Sender::<ID>::route_table_(&self.direct_route_table)
    }
    pub(crate) fn route_table_(direct_route_table: &DashMap<ID, Route>) -> Vec<(ID, Route)> {
        direct_route_table.iter().map(|k| (k.key().clone(), k.value().clone())).collect()
    }
}
