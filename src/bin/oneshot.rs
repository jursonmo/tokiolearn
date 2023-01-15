//use std::thread;
//use std::time;
use tokio::sync::oneshot;
use tokio::time::{sleep, Duration};

//cargo run --bin oneshot ,如果cargo.toml 没有相关[[bin]]指定配置，默认编译src/bin/oneshot.rs

async fn some_operation() -> String {
    // 在这里执行一些操作...
    //thread::sleep(time::Duration::from_secs(2)); //不能用标准库的std::thread;
    sleep(Duration::from_secs(1)).await;
    println!("some_operation() over...");
    String::from("xxxx")
}

#[tokio::main]
async fn main() {
    let (mut tx1, rx1) = oneshot::channel();
    let (tx2, rx2) = oneshot::channel();

    tokio::spawn(async {
        // 等待 `some_operation` 的完成
        // 或者处理 `oneshot` 的关闭通知
        tokio::select! {
            val = some_operation() => {
                let _ = tx1.send(val);
            }
            _ = tx1.closed() => {
                println!("tx1.closed()");
                // 收到了发送端发来的关闭信号
                // `select` 即将结束，此时，正在进行的 `some_operation()` 任务会被取消，任务自动完成，
                // tx1 被释放
            }
        }
    });

    tokio::spawn(async {
        let _ = tx2.send("two");
    });

    tokio::select! {
        val = rx1 => {
            println!("rx1 completed first with {:?}", val);
        }
        val = rx2 => {
            println!("rx2 completed first with {:?}", val);
        }
    }
    println!("async tokio::main over");
}

/*
1. 用标准库的std::thread; some_operation() 没有别取消，依然打印“some_operation() over...”
output:
rx2 completed first with Ok("two")
async tokio::main over
some_operation() over...

2. 用tokio::time::sleep 和Duration, 就能正常取消 some_operation()
rx2 completed first with Ok("two")
async tokio::main over
tx1.closed()
*/
