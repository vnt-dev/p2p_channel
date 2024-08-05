fn main() {
    let r = p2p_channel::stun::stun_test_nat(vec!["193.43.148.37:3478".to_string()], None).unwrap();
    println!("{:?}", r);
}
