use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc::Sender;

use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::sync::Arc;
use tokio::sync::Mutex;

#[allow(dead_code)]
type Fdb = Arc<Mutex<HashMap<i32, Sender<Vec<u8>>>>>;
/*
1. 打开tun 接口，创建tun_channel
  两个任务：
  a: 从tun_channel 读取socket 转发的数据， 然后写到tun 接口里
  b: 从tun接口 读取数据，转发到相应的socket的channel 里，这样socket 就可以从channel 里读数据，然后发到对端

2. tcp server, 接受到new socket 后，创建相应channel,
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

    let tun = build_tun().unwrap();
    let (mut tun_reader, mut tun_writer) = tokio::io::split(tun);
    let (tun_chan_tx, mut tun_chan_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(128);
    let map: HashMap<i32, tokio::sync::mpsc::Sender<Vec<u8>>> = HashMap::with_capacity(10);
    let db = Arc::new(Mutex::new(map));
    tokio::spawn(async move {
        loop {
            if let Some(data) = tun_chan_rx.recv().await {
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

    let db_clone = db.clone();
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
                    let db_clone = db_clone.lock().await;
                    let id = 1i32;
                    if let Some(tx) = db_clone.get(&id) {
                        let mut data = Vec::with_capacity(n);
                        data.extend_from_slice(&buf[..n]);
                        if let Err(_) = (*tx).send(data).await {
                            println!("receiver droped");
                            return;
                        }
                        println!("forward tun data to socket channel");
                    }
                }
            }
        }
    });

    let mut id = 0;
    loop {
        // Asynchronously wait for an inbound socket.
        let (socket, _) = listener.accept().await?;

        // And this is where much of the magic of this server happens. We
        // crucially want all clients to make progress concurrently, rather than
        // blocking one on completion of another. To achieve this we use the
        // `tokio::spawn` function to execute the work in the background.
        //
        // Essentially here we're executing a new task to run concurrently,
        // which will allow all of our clients to be processed concurrently.
        let tun_chan_tx_clone = tun_chan_tx.clone();
        let db_clone = db.clone();
        id = id + 1;
        let current_id = id;

        tokio::spawn(async move {
            //let (tx, mut rx) = mpsc::channel(128);
            let (tx, mut rx) = tokio::sync::mpsc::channel(128);
            let (mut r, mut w) = tokio::io::split(socket);

            let mut db_clone = db_clone.lock().await;
            db_clone.insert(current_id, tx);

            tokio::spawn(async move {
                // In a loop, read data from the socket and write the data back.
                let mut buf: [u8; 2048] = [0; 2048];
                loop {
                    // let n = socket
                    //     .read(&mut buf)
                    //     .await
                    //     .expect("failed to read data from socket");
                    let n = r
                        .read(&mut buf)
                        .await
                        .expect("failed to read data from socket");

                    if n == 0 {
                        return;
                    }
                    println!("socket read n:{}", n);
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
                    // let msg = String::from_utf8_lossy(&mut buf[..n]);
                    // println!("{}", msg);
                    // socket
                    //     .write_all(&buf[0..n])
                    //     .await
                    //     .expect("failed to write data to socket");
                }
            });

            tokio::spawn(async move {
                // loop {
                //     //let data = rx.recv().await.unwrap();
                //     if let Some(data) = rx.recv().await {
                //         //write data to tun channel
                //         //tun_chan_rx_clone.send(data).await;
                //         let ret = w.write(data.as_slice()).await;
                //         match ret {
                //             Ok(n) => println!("read from tun and write to socket n:{}", n),
                //             Err(e) => {
                //                 println!("write to socket err:{}, return", e);
                //                 return;
                //             }
                //         }
                //     }
                // }
                while let Some(data) = rx.recv().await {
                    //write data to tun channel
                    //tun_chan_rx_clone.send(data).await;
                    let ret = w.write(data.as_slice()).await;
                    match ret {
                        Ok(n) => println!("read from tun and write to socket n:{}", n),
                        Err(e) => {
                            println!("write to socket err:{}, return", e);
                            return;
                        }
                    }
                }
            });

            /*
            // tun read --> write to socket
            let mut buf: [u8; 2048] = [0; 2048];
            loop {
                //let data = rx.recv().await.unwrap();
                let ret = tun_reader.read(&mut buf).await;
                match ret {
                    Err(e) => {
                        println!("tun read err:{}", e);
                        return;
                    }
                    Ok(n) => {
                        println!("tun read data n:{}", n);
                        //let ret = socket.write_all(&buf[0..n]).await;
                        let ret = w.write(&buf[0..n]).await;
                        match ret {
                            Ok(n) => println!("read from tun and write to socket n:{}", n),
                            Err(e) => {
                                println!("write to socket err:{}, return", e);
                                return;
                            }
                        }
                    }
                }
            }
            */
        });
    }
}

use tokio_tun::TunBuilder;
fn build_tun() -> tokio_tun::result::Result<tokio_tun::Tun> {
    use std::net::Ipv4Addr;

    let tun = TunBuilder::new()
        .name("mytun0") // if name is empty, then it is set by kernel.
        .tap(false) // false (default): TUN, true: TAP.
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
