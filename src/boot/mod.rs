use std::hash::Hash;
use std::io;
use std::sync::Arc;
use crossbeam::atomic::AtomicCell;
use crossbeam::channel::bounded;
use crossbeam::sync::Parker;
use crossbeam_skiplist::SkipMap;
use crate::channel::{Channel, Status};
use crate::idle::Idle;
use crate::punch::Punch;

pub struct Boot {}

impl Boot {
    pub fn new<ID: Eq + Hash>(size: usize, read_idle: i64, write_idle: i64) -> io::Result<(Channel<ID>, Punch<ID>, Idle<ID>)> {
        let (cone_sender, cone_receiver) = bounded(1);
        let (symmetric_sender, symmetric_receiver) = bounded(1);
        let direct_route_table_time = Arc::new(SkipMap::new());
        let parker = Parker::new();
        let un_parker = parker.unparker().clone();
        let status = Arc::new(AtomicCell::new(Status::Cone));
        let channel = Channel::<ID>::new(size, cone_sender,
                                         symmetric_sender,
                                         direct_route_table_time.clone(),
                                         un_parker, status.clone())?;
        let sender = channel.sender()?;
        let punch = Punch::new(cone_receiver, symmetric_receiver, sender);
        let idle = Idle::new(read_idle, write_idle, parker, direct_route_table_time,status);
        Ok((channel, punch, idle))
    }
}
