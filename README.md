# p2p_channel
NAT traversal

一个点对点通信工具库

对于对称网络，会启动一组UDP尝试打洞
## 示例

```rust

fn main() {
    // 参数一：对称网络类型时，指定用于打洞的端口数量，越多打洞速度越快
    // 参数二：路由表记录的路由读超时时间
    // 参数三：路由表记录的路由写超时时间
    let (mut channel, mut punch, idle) = p2p_channel::boot::Boot::new::<String>(100, 9000, 0).unwrap();

    // 设置本地网络类型，设置正确类型增加打洞成功概率
    //channel.set_nat_type(NatType::Cone).unwrap();

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
        // 对方是锥形网络的打洞
        std::thread::spawn(move || {
            let buf = b"hello";
            loop {
		// 取出打洞的请求
                let (id, nat_info) = punch.next_cone(None).unwrap();
		// 尝试对nat_info记录的远端打洞
                punch.punch(buf, id, nat_info).unwrap(); 
            }
        });
	// 对方是对称型网络打洞
        std::thread::spawn(move || {
            let buf = b"hello";
            loop {
                let (id, nat_info) = punch.next_symmetric(None).unwrap();
                punch.punch(buf, id, nat_info).unwrap();
            }
        });
    }
    // 与中继服务器沟通对端信息
    //channel.send_to_addr(b"hello", "remote_server_ip".parse();
    // 读取中继服务器的消息
    //channel.recv_from(&mut buf, None).unwrap();

   // 触发打洞
   /* channel
         .punch(
	      "peer_id".to_string(),  //为对端设定的ID
	       vec![public_ip],       //对端公网IP
	       public_port,           //对端公网端口
	       0,                     //公共端口区间
	       local_ip,              //对端本地IP
	       local_port,            //对端本地端口
	       NatType::Symmetric,    //对端的Nat网络类型
	 ); */
    // 接收数据处理
    loop {
        let mut buf = [0; 10240];
	// 接受远端发过来的消息
        let (len, route_key) = channel.recv_from(&mut buf, None).unwrap();
        // Do something...
	// 如果是对端发送过来的信息，那么加入路由，后续可以通过ID发送消息给对端
        // channel.add_route("peer_id".to_string(),Route::from(route_key, 10, 64)) //加入路由，超时触发空闲
    }
}

```
