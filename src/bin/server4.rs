/*
优化编译：cargo build --release --bin server4
运行：RUST_LOG=info ../../target/release/server4

server4.rs 已解决:
1. 如果client 把socket 关闭了，服务器这边socket_read_task 退出了，怎么通知socket_write_task 退出。 tokio_util cancel()
2. 如果fdb mac 对应的tx 的接受rx 已经drop, 那么需要删除这个mac 条目
3. fdb学习时：如果是arp, 必须更新fdb, 不管条目是否已经存在。如果是其他数据, 只有fdb mac 条目不存在时才插入新条目
4. log:env_logger, todo: 能否提供接口实时修改日志level
5. todo: 跟client3 一样有命令行提示
6.
*/

use byteorder::{BigEndian, ByteOrder};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::Sender;

use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::sync::Arc;
use tokio::sync::Mutex;

use log::{debug, error, info, log_enabled, warn, Level};

#[allow(dead_code)]
type Db = Arc<Mutex<HashMap<i32, Sender<Vec<u8>>>>>;
type Fdb = Arc<Mutex<HashMap<MacAddr, Arc<Mutex<Sender<Vec<u8>>>>>>>;
type Ports = Arc<Mutex<HashMap<String, Arc<Mutex<Sender<Vec<u8>>>>>>>;

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
    let addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "0.0.0.0:8080".to_string());

    env_logger::init();
    let listener = TcpListener::bind(&addr).await?;
    info!("Listening on: {}", addr);

    let tun = build_tun(true).unwrap();
    let (mut tun_reader, mut tun_writer) = tokio::io::split(tun);
    let (tun_chan_tx, mut tun_chan_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(128);

    let mac_map: HashMap<MacAddr, Arc<Mutex<tokio::sync::mpsc::Sender<Vec<u8>>>>> =
        HashMap::with_capacity(10);
    let fdb = Arc::new(Mutex::new(mac_map));
    let fdb_clone = fdb.clone();

    //for broadcast
    let ports_map: HashMap<String, Arc<Mutex<tokio::sync::mpsc::Sender<Vec<u8>>>>> =
        HashMap::with_capacity(10);
    let ports = Arc::new(Mutex::new(ports_map));
    let ports_clone = ports.clone();

    // let ports_map: HashMap<String, Arc<Mutex<tokio::sync::broadcast::Receiver<Vec<u8>>>>> = HashMap::with_capacity(10);
    // let (btx, _) = tokio::sync::broadcast::channel(8);
    // btx.send(vec![0, 1, 2]);

    tokio::spawn(async move {
        loop {
            if let Some(data) = tun_chan_rx.recv().await {
                //data.bytes();
                let ret = tun_writer.write(data.as_slice()).await;
                match ret {
                    Err(e) => {
                        error!("tun writer err:{}", e);
                        return;
                    }
                    Ok(n) => {
                        if log_enabled!(Level::Debug) {
                            debug!("tun writer n:{}", n);
                        }
                    }
                }
            }
        }
    });

    tokio::spawn(async move {
        let mut buf: [u8; 2048] = [0; 2048];
        loop {
            let ret = tun_reader.read(&mut buf[2..]).await;
            match ret {
                Err(e) => {
                    error!("tun read err:{}", e);
                    return;
                }
                Ok(n) => {
                    //println!("tun read n:{}", n);
                    if log_enabled!(Level::Debug) {
                        debug!("tun read n:{}", n);
                    }
                    BigEndian::write_u16(&mut buf, n as u16);
                    forward_by_fdb(&fdb_clone, &ports_clone, &buf[..n + 2]).await;
                }
            }
        }
    });

    loop {
        // Asynchronously wait for an inbound socket.
        let (socket, addr) = listener.accept().await?;
        info!("new conn from {}", addr);

        let tun_chan_tx_clone = tun_chan_tx.clone();
        let fdb_clone = fdb.clone();

        //let brx_clone = btx.subscribe();
        //let arc_brx_clone = Arc::new(Mutex::new(brx_clone));

        let ports_clone = ports.clone();

        tokio::spawn(async move {
            handle_client(socket, ports_clone, fdb_clone, tun_chan_tx_clone).await;
            /*
                let socket_info = format!("{:?}", socket);
                let socket_info_clone = socket_info.clone();

                let (tx, mut rx) = tokio::sync::mpsc::channel(128);
                let (r, mut w) = tokio::io::split(socket);
                let atx = Arc::new(Mutex::new(tx));

                let mut ports = ports_clone.lock().await;
                ports.insert(socket_info.clone(), atx.clone());
                drop(ports);

                let socket_read_task = tokio::spawn(async move {
                    // In a loop, read data from the socket and write the data back.
                    let mut buf: [u8; 2048] = [0; 2048];
                    let mut r = BufReader::new(r);
                    loop {
                        let ret = r.read_exact(&mut buf[..2]).await; //.expect("failed to read data from socket");
                        let n = match ret {
                            Ok(n) => n,
                            Err(e) => {
                                println!("---read socket data err:{}", e);
                                //对方关闭socket,这里读任务会退出, 但是写任务是感知不到的, 怎么通知写任务退出呢？
                                //socket 写任务发送数据时，对方才回应reset, 本端必须再发送一次数据，应用层才得到错误broken pipe
                                return;
                            }
                        };
                        println!("socket read header n:{}", n);
                        if n == 0 {
                            println!("socket closed");
                            return;
                        }
                        assert_eq!(n, 2);

                        let data_len = BigEndian::read_u16(&buf[..2]) as usize;

                        let n = r
                            .read_exact(&mut buf[..data_len])
                            .await
                            .expect("failed to read data from socket");

                        println!("socket read payload data n:{}", n);
                        if n == 0 {
                            println!("socket closed");
                            return;
                        }
                        assert_eq!(n, data_len);

                        match get_smac(&buf[..n]) {
                            Ok(smac) => {
                                println!("get smac:{}, try to insert fdb", smac);
                                let mut fdb = fdb_clone.lock().await;
                                //entry.or_insert 只有不存在时才插入，存在就返回当前值
                                //fdb.entry(smac).or_insert(Arc::clone(&atx));
                                match fdb.insert(smac, Arc::clone(&atx)) {
                                    Some(v) => println!("update ok , mac:{:?} old value:{:?}", smac, v),
                                    None => println!("insert new mac:{} to fdb", smac),
                                }
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
                    );
                });

                //这里有点问题：必须两个任务结束时, tokio::join 才返回，如果其中一个早就结束了，需要等另一个也结束。解决：tokio::select
                let (socket_read_task_result, socket_write_task_result) =
                    tokio::join!(socket_read_task, socket_write_task);
                match socket_read_task_result {
                    Ok(_) => println!("---socket_read_task completed ok"),
                    Err(e) => println!("---socket_read_task completed err:{}", e),
                }
                match socket_write_task_result {
                    Ok(_) => println!("---socket_write_task completed ok"),
                    Err(e) => println!("---socket_write_task completed err:{}", e),
                }
                println!("--- socket {} all task completed", socket_info);
                //socket PollEvented { io: Some(TcpStream { addr: 192.168.10.1:8080, peer: 192.168.10.2:54448, fd: 12 }) } all task completed
                let mut ports = ports_clone.lock().await;
                ports.remove(&socket_info).unwrap();
            */
        });
    }
}

async fn handle_client(
    socket: TcpStream,
    ports_clone: Ports,
    fdb_clone: Fdb,
    tun_chan_tx_clone: Sender<Vec<u8>>,
) {
    let socket_info = format!("{:?}", socket);
    let socket_info_clone = socket_info.clone();

    let (tx, mut rx) = tokio::sync::mpsc::channel(128);
    let (r, mut w) = tokio::io::split(socket);
    let atx = Arc::new(Mutex::new(tx));

    let mut ports = ports_clone.lock().await;
    ports.insert(socket_info.clone(), atx.clone());
    drop(ports);

    use tokio_util::sync::CancellationToken;
    let token = CancellationToken::new();
    let cloned_token = token.clone();

    let socket_read_task = tokio::spawn(async move {
        // In a loop, read data from the socket and write the data back.
        let mut buf: [u8; 2048] = [0; 2048];
        let mut r = BufReader::new(r);
        loop {
            let ret = r.read_exact(&mut buf[..2]).await; //.expect("failed to read data from socket");
            let n = match ret {
                Ok(n) => n,
                Err(e) => {
                    error!("---read socket data err:{}", e);
                    //对方关闭socket,这里读任务会退出, 但是写任务是感知不到的, 怎么通知写任务退出呢？
                    //1. socket 连续发生两次数据后退出：socket 写任务发送数据时，对方才回应reset, 本端必须再发送一次数据，应用层才得到错误broken pipe
                    //2. tokio cacnel future:
                    token.cancel();
                    return;
                }
            };
            debug!("socket read header n:{}", n);
            if n == 0 {
                error!("socket closed");
                return;
            }
            assert_eq!(n, 2);

            let data_len = BigEndian::read_u16(&buf[..2]) as usize;

            let n = r
                .read_exact(&mut buf[..data_len])
                .await
                .expect("failed to read data from socket");

            if n == 0 {
                error!("read 0, socket closed");
                return;
            }
            assert_eq!(n, data_len);

            debug!("socket read payload data n:{}", n);

            let (mac, is_arp) = get_smac(&buf[..n]);
            match mac {
                Ok(smac) => {
                    debug!("get smac:{}, is_arp:{}, try to insert fdb", smac, is_arp);
                    let mut fdb = fdb_clone.lock().await;
                    //entry.or_insert 只有不存在时才插入，存在就返回当前值
                    //fdb.entry(smac).or_insert(Arc::clone(&atx));
                    if is_arp {
                        //如果是arp, 必须更新fdb, 不管条目是否已经存在。
                        match fdb.insert(smac, Arc::clone(&atx)) {
                            //每个数据包都要调用Arc::clone,不合适，最好还是用entry(smac).or_insert
                            Some(v) => info!("update ok , mac:{:?} old value:{:?}", smac, v),
                            None => info!("insert new mac:{} to fdb", smac),
                        }
                    } else {
                        //如果是其他数据, 只有fdb mac 条目不存在时才插入新条目
                        fdb.entry(smac).or_insert(Arc::clone(&atx));
                    }
                }
                Err(e) => debug!("{}", e),
            }

            let mut v: Vec<u8> = Vec::with_capacity(n);
            v.extend_from_slice(&buf[..n]);

            //let ret = tx.send(v).await;
            let ret = tun_chan_tx_clone.send(v).await;
            match ret {
                Ok(_) => debug!("forward socket data to tun channel ok, n:{}", n),
                Err(e) => {
                    error!(
                        "receiver dropped? read from socket and send channel err:{}",
                        e
                    );
                    return;
                }
            }
        }
    });

    let socket_write_task = tokio::spawn(async move {
        // while let Some(data) = rx.recv().await {
        //     let ret = w.write(data.as_slice()).await;
        //     match ret {
        //         Ok(n) => println!("read from tun and write to socket n:{}", n),
        //         Err(e) => {
        //             println!("write to socket err:{}, return", e);
        //             return;
        //         }
        //     }
        // }
        // println!(
        //     "socket:{}, quit loop of rx read, tx have been dropped?",
        //     socket_info_clone
        // );
        loop {
            tokio::select! {
                _= cloned_token.cancelled() => {
                    error!("socket_write_task have been cancelled");
                    return;
                }
                recv = rx.recv() => {
                    if let Some(data) = recv{
                        let ret = w.write(data.as_slice()).await;
                        match ret {
                            Ok(n) => debug!("read from tun and write to socket n:{}", n),
                            Err(e) => {
                                error!("write to socket err:{}, return", e);
                                return;
                            }
                        }
                    }else {
                        error!("socket:{}, quit loop of rx read, tx have been dropped?",socket_info_clone);
                        return;
                    }
                }
            }
        }
    });

    //这里有点问题：必须两个任务结束时, tokio::join 才返回，如果其中一个早就结束了，需要等另一个也结束。解决：tokio::select
    let (socket_read_task_result, socket_write_task_result) =
        tokio::join!(socket_read_task, socket_write_task);
    match socket_read_task_result {
        Ok(_) => warn!("---socket_read_task completed ok"),
        Err(e) => error!("---socket_read_task completed err:{}", e),
    }
    match socket_write_task_result {
        Ok(_) => warn!("---socket_write_task completed ok"),
        Err(e) => error!("---socket_write_task completed err:{}", e),
    }
    warn!("--- socket {} all task completed", socket_info);
    //socket PollEvented { io: Some(TcpStream { addr: 192.168.10.1:8080, peer: 192.168.10.2:54448, fd: 12 }) } all task completed
    let mut ports = ports_clone.lock().await;
    ports.remove(&socket_info).unwrap();
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

    info!(
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
fn get_smac(buf: &[u8]) -> (std::result::Result<MacAddr, &str>, bool) {
    let packet = EthernetPacket::new(buf).unwrap();
    //println!("{:?}", packet);
    let mut is_arp = false;
    match packet.get_ethertype() {
        EtherTypes::Arp => {
            is_arp = true;
            (Ok(packet.get_source()), is_arp)
        }
        EtherTypes::Ipv4 => {
            // println!("ipv4 or arp");
            // let smac = packet.get_source();
            // println!("get smac:{:?}, ethertype:{}", smac, packet.get_ethertype());
            debug!("get smac:{:?}, ethertype:{}", smac, packet.get_ethertype());
            // return Ok(smac);//by clippy
            (Ok(packet.get_source()), is_arp)
        }
        _ => (Err("get smac fail"), false),
    }
    //Err("get smac fail")
}
fn get_dmac(buf: &[u8]) -> std::result::Result<MacAddr, &str> {
    let packet = EthernetPacket::new(buf).unwrap();
    match packet.get_ethertype() {
        EtherTypes::Ipv4 | EtherTypes::Arp => {
            //println!("ipv4 or arp");
            let smac = packet.get_destination();
            //println!("get dmac:{:?}", smac);
            //return Ok(smac);
            Ok(smac)
        }
        _ => Err("get dmac fail"),
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
        //if let Err(_) = (*tx).send(data).await { //recommend by clippy
        if (*tx).send(data).await.is_err() {
            println!("receiver droped");
            return;
        }
        println!("forward tun data to socket channel");
    }
}

//tun data forward to one of socket
async fn forward_by_fdb(fdb_clone: &Fdb, ports_clone: &Ports, buf: &[u8]) {
    let dmac = match get_dmac(&buf[2..]) {
        Ok(dmac) => dmac,
        Err(e) => {
            error!("err:{}", e);
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
    let mut db_clone = fdb_clone.lock().await;
    let mut should_remove = false;
    if let Some(tx) = db_clone.get(&dmac) {
        let ttx = tx.lock().await;
        //if let Err(_) = ttx.send(data).await { //recommend by clippy
        if ttx.send(data).await.is_err() {
            error!("receiver of socket droped");

            //如果没有接受者,可以把这个fdb条目删除,
            //db_clone.remove(&dmac);但是编译出错，原因是ttx 没有drop,db_clone还是不可变引用的作用域里，而这里db_clone必须是可变引用。所以编译出错。
            should_remove = true;
        }
        if should_remove {
            drop(ttx); //结束ttx的作用域;  --- db_clone immutable borrow later used here; 这样就可以对db_clone进行可变引用了。
            db_clone.remove(&dmac); //如果没有接受者,可以把这个fdb条目删除
        }
        return;
    }

    info!("can't find dmac:{} in fdb, need to broadcast", dmac);
    //db_clone 是 mac-->socket tx 的对应关系，即有多个mac 对应同一个socket tx的情况
    //broadcast, 这里是把数据往相同tx 发多次的情况，所以这里不严谨，有待改善
    /*
    for tx in db_clone.values() {
        let ttx = tx.lock().await;
        // broadcast时，不想clone分配很多份data 内存，是不是可以用Arc 来减少内存的分配，还是用RefCell？broadcast channel?
        let data_copy = data.clone();
        if let Err(_) = ttx.send(data_copy).await {
            println!("receiver of socket droped");
        } else {
            println!("forward tun data to socket channel");
        }
    }
    */
    //drop 参考:https://doc.rust-lang.org/std/sync/struct.Mutex.html
    drop(db_clone); //necessary to manually drop the mutex guard to unlock, 或者用中括号把db_clone固定在小的作用域里

    //解决:把broadcast数据往相同tx 发多次的情况.
    let ports = ports_clone.lock().await;
    for (socket, tx) in ports.iter() {
        let ttx = tx.lock().await;
        // broadcast时，不想clone分配很多份data 内存，是不是可以用Arc 来减少内存的分配，还是用RefCell？broadcast channel?
        let data_copy = data.clone();
        //if let Err(_) = ttx.send(data_copy).await {
        if ttx.send(data_copy).await.is_err() {
            error!("receiver of socket droped");
        } else {
            debug!("forward tun data to socket:{} channel", socket);
        }
    }
}
