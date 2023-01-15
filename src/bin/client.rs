use std::error::Error;
//use std::fs::copy;
//use std::net::Ipv4Addr;
use tokio::io::{self, AsyncReadExt, AsyncWriteExt}; //
use tokio::net::TcpStream;
use tokio::sync::mpsc;
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let (tx, mut rx) = mpsc::channel(128);
    let addr = "192.168.10.1:8080";
    //let stream: TcpStream = TcpStream::connect(addr).await?;
    let stream;
    loop {
        let ret = TcpStream::connect(addr).await;
        match ret {
            Err(e) => {
                println!("connetc addr:{} err:{}", addr, e);
                tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
            }
            Ok(s) => {
                stream = s;
                break;
            }
        }
    }

    let (mut r, mut w) = io::split(stream); //stream.split();

    // TcpStream::split会获取字节流的引用，然后将其分离成一个读取器和写入器。但由于使用了引用的方式，
    // 它们俩必须和 split 在同一个任务中。 优点就是，这种实现没有性能开销，因为无需 Arc 和 Mutex。
    // tokio::spawn(async move {
    //     let (mut rd, mut wr) = socket.split();

    //     if io::copy(&mut rd, &mut wr).await.is_err() {
    //         eprintln!("failed to copy");
    //     }
    // });

    /*
    tokio::spawn(async move {
        let n = w.write(b"hello world\n").await;
        if let Ok(n) = n {
            println!("ok write {} over", n);
        }
    });
    */
    tokio::spawn(async move {
        let mut buf: [u8; 2048] = [0; 2048];
        loop {
            let ret = r.read(&mut buf[..]).await;
            let n = match ret {
                Ok(n) => n,
                Err(e) => {
                    println!("socket read err:{}, quit loop", e);
                    return; //return, over task
                }
            };
            //1.
            // let mut v = Vec::with_capacity(n);
            // for i in 0..n {
            //     v.push(buf[i]);
            // }
            //2.
            //v.clone_from_slice(&mut buf[..n]);
            let mut v = Vec::with_capacity(n);
            v.extend_from_slice(&buf[..n]);

            //3.
            // let mut v = vec![0; 16];
            // v[..n].clone_from_slice(&mut buf[..n]);

            // let msg = String::from_utf8_lossy(&mut buf[..n]);
            // println!("socket read n:{}, msg:{}", n, msg);
            if let Err(_) = tx.send(v).await {
                println!("receiver droped, quit socket read loop");
                return;
            }
        }
    });

    let tun = build_tun().unwrap();
    let (mut tun_reader, mut tun_writer) = tokio::io::split(tun);

    //use tokio::io::{AsyncRead, AsyncWrite};
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            //println!("socket rx recv len:{}, msg:{:?}", msg.len(), msg);
            //println!("socket rx recv len:{}", msg.len());
            // let n = tun_writer.write(msg.as_slice()).await;
            // match n {
            //     Ok(n) => println!("tun write, n:{}", n),
            //     Err(e) => {
            //         println!("tun write, err:{}", e);
            //         return;
            //     }
            // }
            let n = match tun_writer.write(msg.as_slice()).await {
                Ok(n) => n,
                Err(e) => {
                    println!("tun write err:{}, quit loop", e);
                    return;
                }
            };
            println!("recv data from socket rx and forwrd to tun ok, n:{}", n);
        }
        println!("quit socket rx recv loop, because tx dropped?");
    });

    let mut buf = [0u8; 2048];
    loop {
        let n = tun_reader.read(&mut buf).await?;
        //println!("tun reading {} bytes: {:?}", n, &buf[..n]);
        println!("tun read, n:{}", n);
        let ret = w.write(&buf[..n]).await;
        // match ret {
        //     Ok(n) => {
        //         println!("forward tun data to socket ok, n:{}", n);
        //     }
        //     Err(e) => {
        //         println!("forward tun data to socket fail, err:{}", e);
        //     }
        // }

        if let Err(e) = ret {
            println!("forward tun data to socket fail, err:{}", e);
            //Ok(())
            //return Err(Box::new(e));
            ()
        }
        println!("forward tun data to socket ok, n:{}", n);
    }

    // use tokio::time::{sleep, Duration};
    // sleep(Duration::from_secs(100)).await;

    // Ok(()) // unreachable expression
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
        .address(Ipv4Addr::new(10, 0, 0, 2))
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
