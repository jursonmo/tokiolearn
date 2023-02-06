use tokio::sync::oneshot;
use tokio::time::{sleep, Duration};
#[tokio::main]
async fn main() {
    let (tx1, rx1) = oneshot::channel();
    let (tx2, rx2) = oneshot::channel();

    tokio::spawn(async {
        let ret = tx1.send("one");
        match ret {
            Ok(()) => return,
            Err(str) => println!("tx1 send return err str:{}", str),
        }
    });

    tokio::spawn(async {
        sleep(Duration::from_secs(2));
        let ret = tx2.send("two");
        match ret {
            Ok(()) => return,
            Err(str) => println!("tx2 send return err str:{}", str),
        }
    });
    use tokio::time::Instant;

    tokio::select! {
        val = rx1 => {
            println!("rx1 completed first with {:?}", val);
            let now = Instant::now();
            println!("now:{:?}", now);
            // tokio::join!(rx2);
            // let now = Instant::now();
            // println!("rx2 over :{:?}", now);
            return;
        }
        val = rx2 => {
            println!("rx2 completed first with {:?}", val);
        }
        else => {
            println!("both channel closed");
            return;
        }
    }
}
