use std::error::Error;
use std::net::SocketAddr;
use std::process;

use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};

use byteorder::{BigEndian, ByteOrder /*LittleEndian, WriteBytesExt*/};
use clap::Parser;

/// Simple client program of vpn
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// addr that connect to
    #[arg(short, long, default_value_t = String::from("192.168.10.1:8080"))]
    addr: String,

    /// the tun/tap iface name
    #[arg(short = 'n', long, default_value_t = String::from("mytun0"))]
    tuntap: String,

    ///is that tap iface
    #[arg(short, long, default_value_t = true)]
    tap: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    let addr = args.addr.as_str();

    //check addr valid
    let _: SocketAddr = addr.parse().unwrap_or_else(|err| {
        println!("socket addr:{}, parsing arguments: {}", addr, err);
        process::exit(1);
    });

    println!("connecting {}....", addr);
    //如果connet 有了超时机制，tcp syn 被丢弃的话, 没有收到服务器的任何回应，超时就会打印超时出错。
    let timeout_second = 6u64;
    let retry_intvl: u64 = 3;
    loop {
        let (tx, mut rx) = mpsc::channel(128);

        let stream = loop {
            let ret = connect_with_timeout(addr, timeout_second).await;
            match ret {
                Ok(s) => break s,
                Err(e) => {
                    println!("{}", e);
                    sleep(Duration::from_secs(retry_intvl)).await;
                }
            }
        };

        let socket_info = format!("{:?}", stream);
        let socket_info_clone = socket_info.clone();
        //let (r, mut w) = stream.split(); //steam 必须跟 r w 在同一个任务Future里
        let (r, mut w) = io::split(stream); //stream.split();

        //create tun/tap iface
        let tun = build_tun(&args.tuntap, args.tap).unwrap();
        let (mut tun_reader, mut tun_writer) = tokio::io::split(tun);

        //1. socket read --> tx -->rx --> tun write
        let socket_read_task = tokio::spawn(async move {
            let mut buf: [u8; 2048] = [0; 2048];
            let mut r = tokio::io::BufReader::new(r);
            loop {
                let ret = r.read_exact(&mut buf[..2]).await;
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
                println!("socket read header len:{}", n);
                if n == 0 {
                    println!(
                        "socket:{} read loop quit, tx channel will be dropped",
                        socket_info
                    );
                    return;
                }
                let data_len = BigEndian::read_u16(&buf[..2]) as usize;
                println!("payload len:{}", data_len);
                let ret = r.read_exact(&mut buf[..data_len]).await;
                let n = match ret {
                    Ok(n) => n,
                    Err(e) => {
                        println!("socket read err:{}, quit loop", e);
                        return; //return, over task
                    }
                };
                assert_eq!(n, data_len);

                let mut v = Vec::with_capacity(n);
                //let c = std::io::Cursor::new(v);
                //v.write_u16::<LittleEndian>(n as u16).unwrap();
                v.extend_from_slice(&buf[..n]);
                if let Err(_) = tx.send(v).await {
                    println!("receiver droped, quit socket read loop");
                    return;
                }
            }
        });

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
            let n = tun_reader.read(&mut buf[2..]).await?;
            println!("tun read, n:{}", n);
            BigEndian::write_u16(&mut buf, n as u16);
            let ret = w.write(&buf[..n + 2]).await;
            //发现ctrl+c 服务器的程序后，调用w.write()时，是无法感知socket 已经关闭的，需要下次再write,才发现错误Broken pipe
            //原因是ctrl+c 服务器的程序后，服务器发送fin,并没有发送reset(一般是发送reset的), 所以client 需要发送一次数据给服务器，
            //服务器发送reset 后，client协议栈才把socket 设置 broken. client 再次发送数据，这时应用层才感知
            //所以ctrl+c 服务器的程序后，如果服务器发送reset, 然后 client一发送数据，应用层马上得到错误Broken pipe
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
        eprintln!("socket {} all task over", socket_info_clone);
    }
}

use tokio_tun::TunBuilder;
fn build_tun(iface: &str, tap: bool) -> tokio_tun::result::Result<tokio_tun::Tun> {
    use std::net::Ipv4Addr;

    let tun = TunBuilder::new()
        .name(iface) // if name is empty, then it is set by kernel.
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

async fn connect_with_timeout(
    addr: &str,
    timeout_second: u64,
) -> Result<tokio::net::TcpStream, String> {
    let ret = tokio::time::timeout(
        tokio::time::Duration::from_secs(timeout_second),
        tokio::net::TcpStream::connect(addr),
    )
    .await;
    match ret {
        Ok(socket) => match socket {
            Err(e) => {
                //println!("connetc addr:{} err:{}", addr, e);
                let err = format!("connetc addr:{} err:{}", addr, e);
                println!("{}", err);
                return Err(err);
            }
            Ok(s) => {
                //stream = s;
                return Ok(s);
            }
        },
        Err(e) => {
            //use tokio::time::error::Elapsed;
            let err = format!(
                "timeout while connecting to server : {}, timeout:{}",
                e, timeout_second
            );
            return Err(err);
        }
    };
}
