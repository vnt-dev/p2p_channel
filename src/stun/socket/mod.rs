use anyhow::{anyhow, Context};
use network_interface::{NetworkInterface, NetworkInterfaceConfig};
use socket2::Protocol;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
#[cfg(unix)]
pub use unix::*;
#[cfg(windows)]
pub use windows::*;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

pub trait VntSocketTrait {
    fn set_ip_unicast_if(&self, _interface: &LocalInterface) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct LocalInterface {
    index: u32,
    #[cfg(unix)]
    #[allow(dead_code)]
    name: Option<String>,
}

pub fn connect_tcp(
    addr: SocketAddr,
    default_interface: Option<&LocalInterface>,
) -> anyhow::Result<socket2::Socket> {
    let socket = create_tcp(addr.is_ipv4(), default_interface)?;
    socket.connect(&addr.into())?;
    Ok(socket)
}

pub fn create_tcp(
    v4: bool,
    default_interface: Option<&LocalInterface>,
) -> anyhow::Result<socket2::Socket> {
    let socket = if v4 {
        socket2::Socket::new(
            socket2::Domain::IPV4,
            socket2::Type::STREAM,
            Some(Protocol::TCP),
        )?
    } else {
        socket2::Socket::new(
            socket2::Domain::IPV6,
            socket2::Type::STREAM,
            Some(Protocol::TCP),
        )?
    };
    if v4 && default_interface.is_some() {
        socket.set_ip_unicast_if(default_interface.unwrap())?;
    }
    socket.set_nodelay(true)?;
    Ok(socket)
}
pub fn bind_udp_ops(
    addr: SocketAddr,
    only_v6: bool,
    default_interface: Option<&LocalInterface>,
) -> anyhow::Result<socket2::Socket> {
    let socket = if addr.is_ipv4() {
        let socket = socket2::Socket::new(
            socket2::Domain::IPV4,
            socket2::Type::DGRAM,
            Some(Protocol::UDP),
        )?;
        if let Some(default_interface) = default_interface {
            socket.set_ip_unicast_if(default_interface)?;
        }
        socket
    } else {
        let socket = socket2::Socket::new(
            socket2::Domain::IPV6,
            socket2::Type::DGRAM,
            Some(Protocol::UDP),
        )?;
        socket
            .set_only_v6(only_v6)
            .with_context(|| format!("set_only_v6 failed: {}", &addr))?;
        socket
    };
    socket.set_nonblocking(true)?;
    socket.bind(&addr.into())?;
    Ok(socket)
}
pub fn bind_udp(
    addr: SocketAddr,
    default_interface: Option<&LocalInterface>,
) -> anyhow::Result<socket2::Socket> {
    bind_udp_ops(addr, true, default_interface).with_context(|| format!("{}", addr))
}

pub fn get_interface(dest_ip: Ipv4Addr) -> anyhow::Result<LocalInterface> {
    let network_interfaces = NetworkInterface::show()?;
    for iface in network_interfaces {
        for addr in iface.addr {
            if let IpAddr::V4(ip) = addr.ip() {
                if ip == dest_ip {
                    return Ok(LocalInterface {
                        index: iface.index,
                        #[cfg(unix)]
                        name: Some(iface.name),
                    });
                }
            }
        }
    }
    Err(anyhow!("No network card with IP {} found", dest_ip))
}
