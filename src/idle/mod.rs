use crate::channel::{RouteKey, Status};
use crossbeam::atomic::AtomicCell;
use crossbeam::sync::Parker;
use crossbeam_skiplist::SkipMap;
use parking_lot::Mutex;
use std::io;
use std::io::{Error, ErrorKind};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum IdleStatus {
    Write,
    Read,
    Both,
}

pub struct Idle<ID> {
    read_idle: i64,
    write_idle: i64,
    parker: Parker,
    direct_route_table_time: Arc<SkipMap<RouteKey, (Mutex<Vec<ID>>, AtomicI64, AtomicI64)>>,
    status: Arc<AtomicCell<Status>>,
}

impl<ID> Idle<ID> {
    pub(crate) fn new(
        read_idle: i64,
        write_idle: i64,
        parker: Parker,
        direct_route_table_time: Arc<SkipMap<RouteKey, (Mutex<Vec<ID>>, AtomicI64, AtomicI64)>>,
        status: Arc<AtomicCell<Status>>,
    ) -> Idle<ID> {
        Idle {
            write_idle,
            read_idle,
            parker,
            direct_route_table_time,
            status,
        }
    }
}

impl<ID: Clone> Idle<ID> {
    /// 获取空闲路由
    pub fn next_idle(&self) -> io::Result<(IdleStatus, Vec<ID>, RouteKey)> {
        loop {
            let now = chrono::Local::now().timestamp_millis();
            let last_read_idle = now - self.read_idle;
            let last_write_idle = now - self.write_idle;
            let mut min = i64::MAX;
            for entry in self.direct_route_table_time.iter() {
                let mut is_read_idle = false;
                if self.read_idle > 0 {
                    let last_read = entry.value().1.load(Ordering::Relaxed);
                    if last_read < last_read_idle {
                        is_read_idle = true;
                    } else {
                        if min > last_read {
                            min = last_read;
                        }
                    }
                }
                if self.write_idle > 0 {
                    let last_write = entry.value().2.load(Ordering::Relaxed);
                    if last_write < last_write_idle {
                        if is_read_idle {
                            return Ok((
                                IdleStatus::Both,
                                entry.value().0.lock().clone(),
                                *entry.key(),
                            ));
                        }
                        return Ok((
                            IdleStatus::Write,
                            entry.value().0.lock().clone(),
                            *entry.key(),
                        ));
                    }
                    if min > last_write {
                        min = last_write;
                    }
                }
                if is_read_idle {
                    return Ok((
                        IdleStatus::Read,
                        entry.value().0.lock().clone(),
                        *entry.key(),
                    ));
                }
            }
            if self.direct_route_table_time.is_empty() {
                self.parker.park();
            } else {
                let sleep_time = chrono::Local::now().timestamp_millis() - min;
                if sleep_time > 0 {
                    self.parker
                        .park_timeout(Duration::from_millis(sleep_time as u64));
                }
            }
            if self.status.load() == Status::Close {
                return Err(Error::new(ErrorKind::Other, "closed"));
            }
        }
    }
}
