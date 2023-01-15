use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc::Sender;

use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::sync::Arc;
use tokio::sync::Mutex;

#[allow(dead_code)]
type Db = Arc<Mutex<HashMap<i32, Sender<Vec<u8>>>>>;
type Fdb = Arc<Mutex<HashMap<MacAddr, Arc<Mutex<Sender<Vec<u8>>>>>>>;
/*
1. 打开tun 接口，创建tun_channel
  两个任务：
  a: 从tun_channel 读取socket 转发的数据， 然后写到tun 接口里
  b: 从tun接口 读取数据，转发到相应的socket的channel 里，这样socket 就可以从channel 里读数据，然后发到对端

2. tcp server, 接受到new socket 后，创建相应channel, 并且把channel 放进hashmap db
    每个new socket 都要开启两个任务：
    任务1：socket 读到数据，就放到tun_channel 里，等待tun 任务写入tun 接口。
    任务2: 循环读取自己channel 的数据，发给对端
*/
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Allow passing an address to listen on as the first argument of this
    // program, but otherwise we'll just set up our TCP listener on
    // 127.0.0.1:8080 for connections.
    let addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "0.0.0.0:8080".to_string());

    // Next up we create a TCP listener which will listen for incoming
    // connections. This TCP listener is bound to the address we determined
    // above and must be associated with an event loop.
    let listener = TcpListener::bind(&addr).await?;
    println!("Listening on: {}", addr);

    let tun = build_tun(true).unwrap();
    let (mut tun_reader, mut tun_writer) = tokio::io::split(tun);
    let (tun_chan_tx, mut tun_chan_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(128);

    // let map: HashMap<i32, tokio::sync::mpsc::Sender<Vec<u8>>> = HashMap::with_capacity(10);
    // let db = Arc::new(Mutex::new(map));
    // let db_clone = db.clone();
    // let mut id = 0;

    let mac_map: HashMap<MacAddr, Arc<Mutex<tokio::sync::mpsc::Sender<Vec<u8>>>>> =
        HashMap::with_capacity(10);
    let fdb = Arc::new(Mutex::new(mac_map));
    let fdb_clone = fdb.clone();

    tokio::spawn(async move {
        loop {
            if let Some(data) = tun_chan_rx.recv().await {
                //data.bytes();
                let ret = tun_writer.write(data.as_slice()).await;
                match ret {
                    Err(e) => {
                        println!("tun writer err:{}", e);
                        return;
                    }
                    Ok(n) => println!("tun writer n:{}", n),
                }
            }
        }
    });

    tokio::spawn(async move {
        let mut buf: [u8; 2048] = [0; 2048];
        loop {
            let ret = tun_reader.read(&mut buf).await;
            match ret {
                Err(e) => {
                    println!("tun read err:{}", e);
                    return;
                }
                Ok(n) => {
                    println!("tun read n:{}", n);
                    //let db_clone = db_clone.lock().unwrap();
                    // let db_clone = db_clone.lock().await;
                    // let id = 1i32;
                    // if let Some(tx) = db_clone.get(&id) {
                    //     let mut data = Vec::with_capacity(n);
                    //     data.extend_from_slice(&buf[..n]);
                    //     if let Err(_) = (*tx).send(data).await {
                    //         println!("receiver droped");
                    //         return;
                    //     }
                    //     println!("forward tun data to socket channel");
                    // }
                    //forward(&db_clone, &buf[..n]).await;
                    forward_by_fdb(&fdb_clone, &buf[..n]).await;
                }
            }
        }
    });

    loop {
        // Asynchronously wait for an inbound socket.
        let (socket, addr) = listener.accept().await?;
        println!("new conn from {}", addr);

        let tun_chan_tx_clone = tun_chan_tx.clone();
        // let db_clone = db.clone();
        // //id = id + 1;
        // id += 1;
        // let current_id = id;

        let fdb_clone = fdb.clone();

        tokio::spawn(async move {
            let socket_info = format!("{:?}", socket);
            //let socket_info = socket_info.as_str();
            let socket_info_clone = socket_info.clone();

            //let (tx, mut rx) = mpsc::channel(128);
            let (tx, mut rx) = tokio::sync::mpsc::channel(128);
            let (mut r, mut w) = tokio::io::split(socket);
            let atx = Arc::new(Mutex::new(tx));
            // let mut db_clone = db_clone.lock().await;
            // db_clone.insert(current_id, tx);

            let socket_read_task = tokio::spawn(async move {
                // In a loop, read data from the socket and write the data back.
                let mut buf: [u8; 2048] = [0; 2048];

                loop {
                    let n = r
                        .read(&mut buf)
                        .await
                        .expect("failed to read data from socket");

                    println!("socket read n:{}", n);
                    if n == 0 {
                        println!("socket closed");
                        return;
                    }
                    //learn
                    //learn(db_clone, &buf[..n], &tx);
                    match get_smac(&buf[..n]) {
                        Ok(smac) => {
                            println!("get smac:{}, try to insert fdb", smac);
                            //fdb_learn(&fdb_clone, smac, &tx).await;

                            let mut fdb = fdb_clone.lock().await;
                            // if fdb.contains_key(smac) {
                            //     println!("mac:{} have in fdb", smac);
                            //     ()
                            // }
                            // if let Some(v) = fdb.get(smac) {
                            //     println!("mac:{} have in fdb, value:{:?}", smac, v);
                            //     ()
                            // }
                            // if let None = fdb.insert(smac, Arc::clone(&atx)) {
                            //     println!("insert new mac:{} to fdb", smac);
                            // }
                            //entry.or_insert 只有不存在时才插入，存在就返回当前值
                            //fdb.entry(smac).or_insert(Arc::clone(&atx));
                            match fdb.insert(smac, Arc::clone(&atx)) {
                                Some(v) => println!("update ok , mac:{:?} old value:{:?}", smac, v),
                                None => println!("insert new mac:{} to fdb", smac),
                            }
                            // let mut fdbx = fdb_clone.lock().await;
                            // fdb_learn2(&mut fdbx, smac, &tx);
                        }
                        Err(e) => println!("{}", e),
                    }

                    let mut v: Vec<u8> = Vec::with_capacity(n);
                    v.extend_from_slice(&buf[..n]);

                    //let ret = tx.send(v).await;
                    let ret = tun_chan_tx_clone.send(v).await;
                    match ret {
                        Ok(_) => println!("forward socket data to tun channel ok, n:{}", n),
                        Err(e) => {
                            println!(
                                "receiver dropped? read from socket and send channel err:{}",
                                e
                            );
                            return;
                        }
                    }
                }
            });

            let socket_write_task = tokio::spawn(async move {
                while let Some(data) = rx.recv().await {
                    let ret = w.write(data.as_slice()).await;
                    match ret {
                        Ok(n) => println!("read from tun and write to socket n:{}", n),
                        Err(e) => {
                            println!("write to socket err:{}, return", e);
                            return;
                        }
                    }
                }
                println!(
                    "socket:{}, quit loop of rx read, tx have been dropped?",
                    socket_info_clone
                )
            });
            let (socket_read_task_result, socket_write_task_result) =
                tokio::join!(socket_read_task, socket_write_task);
            match socket_read_task_result {
                Ok(_) => println!("socket_read_task completed ok"),
                Err(e) => println!("socket_read_task completed err:{}", e),
            }
            match socket_write_task_result {
                Ok(_) => println!("socket_write_task completed ok"),
                Err(e) => println!("socket_write_task completed err:{}", e),
            }
            println!("socket {} all task completed", socket_info);
            //socket PollEvented { io: Some(TcpStream { addr: 192.168.10.1:8080, peer: 192.168.10.2:54448, fd: 12 }) } all task completed
        });
    }
}

use tokio_tun::TunBuilder;
fn build_tun(tap: bool) -> tokio_tun::result::Result<tokio_tun::Tun> {
    use std::net::Ipv4Addr;

    let tun = TunBuilder::new()
        .name("mytun0") // if name is empty, then it is set by kernel.
        .tap(tap) // false (default): TUN, true: TAP.
        .packet_info(false) // false: IFF_NO_PI, default is true.
        .mtu(1500)
        .up() // or set it up manually using `sudo ip link set <tun-name> up`.
        .address(Ipv4Addr::new(10, 0, 0, 1))
        .netmask(Ipv4Addr::new(255, 255, 255, 0))
        .try_build()?; // or `.try_build_mq(queues)` for multi-queue support.

    println!(
        "tun created, name: {}, mtu:{}, tap:{}, address:{}, netmask:{}",
        tun.name(),
        tun.mtu().unwrap(),
        tun.flags().unwrap(),
        tun.address().unwrap(),
        tun.netmask().unwrap()
    );
    Ok(tun)
}

use pnet::packet::ethernet::{EtherTypes, EthernetPacket};
use pnet_base::MacAddr;
// use tokio::sync::MutexGuard;
// fn learn(
//     db: MutexGuard<HashMap<i32, Sender<Vec<u8>>>>,
//     buf: &[u8],
//     tx: *mut tokio::sync::mpsc::Sender<Vec<u8>>,
// ) {
//     let packet = EthernetPacket::new(buf).unwrap();
// }

fn get_smac(buf: &[u8]) -> std::result::Result<MacAddr, &str> {
    let packet = EthernetPacket::new(buf).unwrap();
    //println!("{:?}", packet);
    match packet.get_ethertype() {
        EtherTypes::Ipv4 | EtherTypes::Arp => {
            println!("ipv4 or arp");
            let smac = packet.get_source();
            println!("get smac:{:?}, ethertype:{}", smac, packet.get_ethertype());
            return Ok(smac);
        }
        _ => return Err("get smac fail"),
    }
    //Err("get smac fail")
}
fn get_dmac(buf: &[u8]) -> std::result::Result<MacAddr, &str> {
    let packet = EthernetPacket::new(buf).unwrap();
    match packet.get_ethertype() {
        EtherTypes::Ipv4 | EtherTypes::Arp => {
            println!("ipv4 or arp");
            let smac = packet.get_destination();
            println!("get dmac:{:?}", smac);
            return Ok(smac);
        }
        _ => return Err("get dmac fail"),
    }
    //Err("get mac fail")
}

#[allow(dead_code)]
async fn forward(db_clone: &Db, buf: &[u8]) {
    //let db_clone = db_clone.lock().unwrap();
    let db_clone = db_clone.lock().await;
    let id = 1i32;
    if let Some(tx) = db_clone.get(&id) {
        let mut data = Vec::with_capacity(buf.len());
        data.extend_from_slice(buf);
        if let Err(_) = (*tx).send(data).await {
            println!("receiver droped");
            return;
        }
        println!("forward tun data to socket channel");
    }
}

//tun data forward to one of socket
async fn forward_by_fdb(fdb_clone: &Fdb, buf: &[u8]) {
    let dmac = match get_dmac(buf) {
        Ok(dmac) => dmac,
        Err(e) => {
            println!("err:{}", e);
            return;
        }
    };

    let mut data = Vec::with_capacity(buf.len());
    data.extend_from_slice(buf);

    //这里有个性能的隐患, 是不是要等到ttx.send完成后，db_clone 才能解锁？
    //使用 Tokio 提供的异步锁:
    //https://course.rs/async-rust/tokio/shared-state.html#%E4%BD%BF%E7%94%A8-tokio-%E6%8F%90%E4%BE%9B%E7%9A%84%E5%BC%82%E6%AD%A5%E9%94%81
    //如果其他任务只有在insert new mac 时使用db_clone， 那就应该没有太大问题，因为大部分情况都是tun read 的这一个任务在调用forward_by_fdb->db_clone.lock()
    //所以socket 不能一直经常调用db_clone.lock()，经常用会产生锁竞争。这个要调整，TODO
    //let db_clone = db_clone.lock().unwrap();
    let db_clone = fdb_clone.lock().await;
    if let Some(tx) = db_clone.get(&dmac) {
        let ttx = tx.lock().await;
        if let Err(_) = ttx.send(data).await {
            println!("receiver of socket droped");
        } else {
            println!("forward tun data to socket channel");
        }
        return;
    }

    println!("can't find dmac:{} in fdb, need to broadcast", dmac);
    //broadcast, 但是有多个mac 对应同一个socket tx的情况，所以这里不严谨，有待改善
    // for (mac, tx) in db_clone.iter() {
    //     let ttx = tx.lock().await;
    //     let data_copy = data.clone();
    //     if let Err(_) = ttx.send(data_copy).await {
    //         println!("receiver of socket droped");
    //     } else {
    //         println!("forward tun data to mac:({})->socket channel", mac);
    //     }
    // }

    for tx in db_clone.values() {
        let ttx = tx.lock().await;
        // broadcast时，不想clone分配很多份data 内存，是不是可以用Arc 来减少内存的分配，还是用RefCell？
        let data_copy = data.clone();
        if let Err(_) = ttx.send(data_copy).await {
            println!("receiver of socket droped");
        } else {
            println!("forward tun data to socket channel");
        }
    }
}

// async fn fdb_learn(fdb: &Fdb, mac: MacAddr, tx: &Sender<Vec<u8>>) {
//     let mut fdb = fdb.lock().await;
//     // if let None = fdb.insert(mac, *tx) {
//     //     println!("insert new mac:{} to fdb", mac);
//     // }
//     match fdb.insert(mac, *tx) {
//         Some(v) => println!("update ok , mac:{:?} old value:{:?}", mac, v),
//         None => println!("insert new mac:{} to fdb", mac),
//     }
//     return;
// }

//use tokio::sync::MutexGuard;
// fn fdb_learn2(
//     fdb: &mut MutexGuard<HashMap<MacAddr, Sender<Vec<u8>>>>,
//     mac: MacAddr,
//     tx: &Sender<Vec<u8>>,
// ) {
//     if let None = fdb.insert(mac, *tx) {
//         println!("insert new mac:{} to fdb", mac);
//     }
//     return;
// }
