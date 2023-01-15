use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
//把文件放到src/bin 下，vscode 才进行语法分析？
#[tokio::main]
async fn main() {
    let (tx1, mut rx1) = mpsc::channel(128);
    let (tx2, mut rx2) = mpsc::channel(128);

    tokio::spawn(async move {
        // 用 tx1 和 tx2 干一些不为人知的事
        sleep(Duration::from_secs(1)).await;
        // let _ = tx1.send("tx1").await;
        // let _ = tx2.send("tx2").await;
        let _ = tx1.send("tx1");
        let _ = tx2.send("tx2");
    });

    tokio::select! {
        Some(v) = rx1.recv() => {
            println!("Got {:?} from rx1", v);
        }
        Some(v) = rx2.recv() => {
            println!("Got {:?} from rx2", v);
        }
        else => {
            println!("Both channels closed");
        }
    }
}
