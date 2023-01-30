use std::env;
use std::error::Error;
use std::net::SocketAddr;
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};

use tokio::sync::mpsc;
fn show_first_arg() {
    if let Some(v) = env::args().nth(1) {
        println!("first parament is {}", v)
    }
    let myaddr = gen_myaddr("xxxxx").unwrap_or_default();
    println!("MyAddr:{:?}, default addr:{}", myaddr, myaddr.addr);
}

#[derive(Debug)]
struct MyAddr {
    addr: String,
}
//测试下Default trait, unwrap_or_default 接口
impl std::default::Default for MyAddr {
    fn default() -> Self {
        MyAddr {
            addr: String::from("192.168.10.1:8080"),
        }
    }
}

fn gen_myaddr(addr: &str) -> Option<MyAddr> {
    if addr.parse::<SocketAddr>().is_err() {
        println!("socket addr:{} invalid", addr);
        return None;
    }
    return Some(MyAddr {
        addr: addr.to_string(),
    });
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    show_first_arg();
    //let args: Vec<String> = env::args().collect();
    let args = env::args().skip(1).collect::<Vec<_>>();
    // let default_addr = "192.168.10.1:8080";
    // let addr = match args.first().ok_or(default_addr) {
    //     Ok(addr) => addr,
    //     Err(defaut) => defaut,
    // };

    let mut addr = "192.168.10.1:8080";
    if let Some(v) = args.first() {
        addr = v
    }

    //1. check addr trans to SocketAddr,
    let x: std::result::Result<SocketAddr, std::net::AddrParseError> = addr.parse();
    if let Ok(s) = x {
        println!("socket addr:{:?}", s);
    } else if let Err(e) = x {
        println!("socket addr:{} invalid, err:{}", addr, e);
        //return Ok(());
    }
    //2. check addr trans to SocketAddr
    if addr.parse::<SocketAddr>().is_err() {
        println!("socket addr:{} invalid", addr);
        //return Ok(());
    }

    use std::process;
    let _: SocketAddr = addr.parse().unwrap_or_else(|err| {
        println!("socket addr:{}, parsing arguments: {}", addr, err);
        process::exit(1);
    });

    //3. check addr trans to SocketAddr, fail will panic,  unwrap 或者 expect 都可能会panic, 只是expect在panic时打印自定义的错误内容
    let _: SocketAddr = addr.parse().unwrap();
    let _ = addr.parse::<SocketAddr>().expect("socket addr invalid");

    //4. 利用 map_or 在Err的情况下返回默认值 或者Ok的情况下返回其他值。
    let default_addr = "192.168.10.1:8080";
    let f = |_| addr;
    let addr = addr.parse::<SocketAddr>().map_or(default_addr, f);

    println!("connecting {}....", addr);
    //let stream: TcpStream = TcpStream::connect(addr).await?; //应该要循环不停conneting
    /*
    //如果connet没有超时机制，tcp syn 被丢弃的话(没有收到服务器的reset)，
    //这里就会很久没有任何输出，给人一种不知道底层有没有在connecting。（connet底层也有默认的超时，但是很久)
    use tokio::net::TcpStream;
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
    */

    //如果connet 有了超时机制，tcp syn 被丢弃的话, 没有收到服务器的任何回应，超时就会打印超时出错。
    let timeout_second = 6u64;
    loop {
        let (tx, mut rx) = mpsc::channel(128);

        let stream = loop {
            let ret = tokio::time::timeout(
                tokio::time::Duration::from_secs(timeout_second),
                tokio::net::TcpStream::connect(addr),
            )
            .await;
            match ret {
                Ok(socket) => match socket {
                    Err(e) => {
                        println!("connetc addr:{} err:{}", addr, e);
                        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                    }
                    Ok(s) => {
                        //stream = s;
                        break s;
                    }
                },
                Err(e) => {
                    println!(
                        "timeout while connecting to server : {}, timeout:{}",
                        e, timeout_second
                    );
                }
            };
        };

        let socket_info = format!("{:?}", stream);
        //let socket_info_str = socket_info.as_str();
        let socket_info_str = "test static str";
        let (mut r, mut w) = io::split(stream); //stream.split();

        //1. socket read --> tx -->rx --> tun write
        let socket_read_task = tokio::spawn(async move {
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
                // 如果对方发送了Reset, r.read 没有返回Err(e), 而是返回n==0 ??
                // 抓包看，服务器没有发reset, 而是发fin, client 收到fin, read 不会返回错误。
                // 为了模拟服务器发送Reset, golang 写个tcp 服务器，并且服务器的应用层不read, 保证服务器的Recv-Q有数据，
                // 这时，如果杀掉服务器进程，服务器就会发送Reset 给client
                // 这时 client 就会收到RST Reset, read 操作马上返回错误：Connection reset by peer
                // 这里会打印：socket read err:Connection reset by peer (os error 104), quit loop
                // socket write 就马上发生错误。 如果是fin, socket 需要再write 一次，应用才返回错误:pipe broken.
                println!("socket read n:{}", n);
                if n == 0 {
                    println!(
                        "socket:{} read loop quit, tx channel will be dropped",
                        socket_info
                    );
                    println!("socket_info_str:{}", socket_info_str);
                    return;
                }

                let mut v = Vec::with_capacity(n);
                v.extend_from_slice(&buf[..n]);
                if let Err(_) = tx.send(v).await {
                    println!("receiver droped, quit socket read loop");
                    return;
                }
            }
        });

        let tun = build_tun(true).unwrap();
        let (mut tun_reader, mut tun_writer) = tokio::io::split(tun);

        let tun_write_task = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let n = match tun_writer.write(msg.as_slice()).await {
                    Ok(n) => n,
                    Err(e) => {
                        println!("tun write err:{}, quit loop", e);
                        return;
                    }
                };
                println!("recv data from socket rx and forwrd to tun ok, n:{}", n);
            }
            println!("quit socket rx recv loop, because tx dropped?, tun write task also quit");
        });

        //2. tun read --> socket write
        let mut buf = [0u8; 2048];
        loop {
            let n = tun_reader.read(&mut buf).await?;
            println!("tun read, n:{}", n);
            let ret = w.write(&buf[..n]).await;
            //发现ctrl+c 服务器的程序后，调用w.write()时，是无法感知socket 已经关闭的，需要下次再write,才发现错误Broken pipe
            //原因是ctrl+c 服务器的程序后，服务器发送fin,并没有发送reset(一般是发送reset的), 所以client 需要发送一次数据给服务器，
            //服务器发送reset 后，client协议栈才把socket 设置 broken. client 再次发送数据，这时应用层才感知
            /*
            if let Err(e) = ret {
                println!(
                    "forward tun data to socket fail, err:{}, socket writer:{}",
                    e, w
                );
                //Ok(())
                //return Err(Box::new(e));
                //break;
                return Ok(());
            } else {
                println!("forward tun data to socket ok, n:{}", n);
            }
            */
            match ret {
                Ok(n) => {
                    if n == 0 {
                        println!("something wrong? forward tun data to socket n:{}", n);
                        //return Ok(());
                        break;
                    }
                    println!("forward tun data to socket ok, n:{}", n);
                }
                Err(e) => {
                    println!(
                        "forward tun data to socket fail, err:{}, socket writer:{:?}",
                        e, w
                    );
                    //return Ok(());
                    break;
                }
            }
        }

        //join task, 等到异步task 都结束后, tun网口也自动关闭，重新发起connect
        let (socket_read_task_result, tun_write_task_result) =
            tokio::join!(socket_read_task, tun_write_task);
        match socket_read_task_result {
            Ok(_) => println!("socket_read_task completed ok"),
            Err(e) => println!("socket_read_task completed err:{}", e),
        }
        match tun_write_task_result {
            Ok(_) => println!("tun_write_task completed ok"),
            Err(e) => println!("tun_write_task completed err:{}", e),
        }
        println!("socket_info_str:{}", socket_info_str);
        // loop {
        //     //select 是侦听future,而不是JoinHandel<>.
        //     tokio::select! {
        //         _ = socket_read_task => { // socket_read_task应该是future, 而不是joinHandle
        //             println!("socket_read_task completed ")
        //         }
        //         _ = tun_write_task => {
        //             println!("tun_write_task completed ")
        //         }
        //         else =>{println!("all task completed");break},
        //     }
        // }
    }
    //Ok(())
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
