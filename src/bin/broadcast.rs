use tokio::sync::broadcast;
#[tokio::main]
async fn main() {
    let (tx, mut rx) = broadcast::channel(2);

    tx.send(10).unwrap();
    tx.send(20).unwrap();
    tx.send(30).unwrap();

    // The receiver lagged behind
    assert!(rx.recv().await.is_err()); //RecvError::Lagged

    // At this point, we can abort or continue with lost messages

    assert_eq!(20, rx.recv().await.unwrap());
    assert_eq!(30, rx.recv().await.unwrap());
    //RecvError::Closed
}
