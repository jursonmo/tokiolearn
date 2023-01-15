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

/*
@sdwannfv
sdwannfv
Aug 16
use tokio::sync::oneshot;

#[tokio::main]
async fn main() {
    let (tx1, rx1) = oneshot::channel();
    let (tx2, rx2) = oneshot::channel();

    tokio::spawn(async {
        let _ = tx1.send("one");
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
    // 任何一个 select 分支结束后，都会继续执行接下来的代码
}

这段代码async 没有move 为什么能使用，tx1, tx2的生命周期不是比async代码段短吗？

---------------------------------
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    let (tx1, mut rx1) = mpsc::channel(10);
    let (tx2, mut rx2) = mpsc::channel(10);

    tokio::spawn(async move {
        let _ = tx1.send("one");
    });

    tokio::spawn(async move {
        let _ = tx2.send("two");
    });

    tokio::select! {
        val = rx1.recv() => {
            println!("rx1 completed first with {:?}", val);
        }
        val = rx2.recv() => {
            println!("rx2 completed first with {:?}", val);
        }
    }

    // 任何一个 select 分支结束后，都会继续执行接下来的代码
}

换成mspc就需要move?


mpsc::Sender::send(&self, value: T) 使用是Sender的引用，不使用move不会取得Sender的所有权，
而oneshot::Sender::send(mut self, t: T)使用是self不是引用，不使用move也会以取得Sender的所有权，
（莫：如果是引用，rust 推算出是就不会move的, 如果想移动所有权，就要明确写move,
 不是引用，默认推算就是move, 不需要明确写move）
*/
