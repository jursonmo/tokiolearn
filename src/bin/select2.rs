use tokio_stream::{self as stream, StreamExt};

#[tokio::main]
async fn main() {
    let mut stream1 = stream::iter(vec![1, 2, 3]);
    let mut stream2 = stream::iter(vec![4, 5, 6]);

    let mut values = vec![];

    loop {
        tokio::select! {
            Some(v) = stream1.next() => {values.push(v);println!("add stream1 value:{}", v)},
            Some(v) = stream2.next() => {values.push(v);println!("add stream2 value:{}", v)},
            else => {println!("all over");break},
        }
    }
    println!("values:{:?}", values);
    // values.sort();
    // assert_eq!(&[1, 2, 3, 4, 5, 6], &values[..]);
}
/*
add stream1 value:1
add stream2 value:4
add stream2 value:5
add stream2 value:6
add stream1 value:2
add stream1 value:3
all over
values:[1, 4, 5, 6, 2, 3]
*/
