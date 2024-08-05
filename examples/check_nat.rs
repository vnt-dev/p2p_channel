fn main() {
    let r = p2p_channel::stun::stun_test_nat(
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
    println!("{:?}", r);
}
