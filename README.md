# p2p_channel
NAT traversal

一个点对点通信工具库

对于对称网络，会启动一组UDP尝试打洞
## 示例

```rust

fn main() {
    let (mut channel, mut punch, idle) = p2p_channel::boot::Boot::new::<String>(100, 9000, 0).unwrap();
    {
        // 空闲处理，添加的路由空闲时触发
        std::thread::spawn(move || {
            loop {
                let (idle_status, id, route) = idle.next_idle().unwrap();
                // channel.send_to_route()
                // channel.remove_route(&id)
                println!("{:?}", idle_status);
            }
        });
    }
    {
        // 打洞处理
        std::thread::spawn(move || {
            let buf = b"hello";
            loop {
                let (id, nat_info) = punch.next_cone(None).unwrap();
                punch.punch(buf, id, nat_info).unwrap();
            }
        });
        std::thread::spawn(move || {
            let buf = b"hello";
            loop {
                let (id, nat_info) = punch.next_symmetric(None).unwrap();
                punch.punch(buf, id, nat_info).unwrap();
            }
        });
    }
    // 接收数据处理
    loop {
        let mut buf = [0; 10240];
        let (len, route_key) = channel.recv_from(&mut buf, None).unwrap();
        // Do something...
        // channel.punch("peer_id".to_string(), p2p_channel::punch::NatInfo) //触发打洞
        // channel.add_route() //超时触发空闲
    }
}

```
