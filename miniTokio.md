#### 了解tokio 机制

https://course.rs/async-rust/tokio/async.html
最终代码: https://github.com/tokio-rs/website/blob/master/tutorial-code/mini-tokio/src/main.rs
```
mini_tokio:
--->new():两个队列 ---> 执行器run(): 侦听接受队列，等待task, 执行task.poll()-->pending 继续放回队列
    |                                          |
    |                                          |
    --->spawn(future)：如果资源没准备好           |
         将future 发送到执行器队列--------------->|
         （这样有问题，执行器不断循环调用future poll 去检测future 是否准备好）
            |
            |
            --->改进：当future 资源准备好后，才发送到执行器队列里
                |
 +--------------------------------------------------------+ 
| mini_tokio:spawn(future)创建一个Task 对象，它需要作为Waker,
| 所以Task 需要实现ArcWake trait
| 方法：fn wake_by_ref(arc_self: &Arc<Self>)；Task 作为中间层，
| 包含有真正要运行future的对象, 因为Task 作为一个Waker, 需要告诉执行器具体
| 哪个future任务已经准备好。                
| 比如：Delay{} 模拟成一个future, 即实现了Futrue Trait 方法
| fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>)
| 在具体实现poll时，创建一个线程去休眠来模拟等待资源就绪的情况，
| 就休眠后(即资源准备好了)再调用waker.Wake()-->Task.wake_by_ref()
| 把Task 放回到 执行器队列里。
|（Task as waker 是通过ctx参数传给future poll函数的）

创建 Waker 的最简单方法是实现 ArcWake trait，
然后使用 waker_by_ref 或者 .into_waker() 函数来把 Arc<impl ArcWake> 转变成 Waker。

改进后的过程：mini_tokio -->spawn 创建一个Task, 发送到执行器-->执行器调用Task.poll--> 实际future.poll,并传入Task as waker
-->Delay.poll-->如果资源没准备好，就休眠后再调用waker.Wake()-->Task.wake_by_ref()
-->Task.schedule-->将Task 发送到执行器队列里--->执行器就会调用Task.poll
-->Delay.poll,这时资源准备好了，不再调用waker.Wake()
```
##### 补充
[理解 Future 和异步任务是如何调度的](
https://huangjj27.github.io/async-book/02_execution/01_chapter.html)

#### Future trait
```rust
Future trait 是 Rust 异步编程中心内容。它是一种异步计算，可以产生值（尽管这个值可以为空， 如 ()）。
简化版 future trait看起来可能像这样：

trait SimpleFuture {
    type Output;
    fn poll(&mut self, wake: fn()) -> Poll<Self::Output>;
}

enum Poll<T> {
    Ready(T),
    Pending,
}
```
Future 能通过调用 poll 的方式推进，这会尽可能地推进 future 到完成状态。如果 future 完成了， 那就会返回 poll::Ready(result)。如果 future 尚未完成，则返回 poll::Pending，并且安排 wake() 函数在 Future 准备好进一步执行时调用（译者注：注册回调函数）。当 wake() 调用 时，驱动 Future 的执行器会再次 poll 使得 Future 有所进展。

没有 wake() 函数的话，执行器将无从获知一个 future 是否能有所进展，只能持续轮询（polling） 所有 future。但有了 wake() 函数，执行器就能知道哪些 future 已经准备好轮询了。

例如，考虑一下场景：我们准备读取一个套接字（socket），它可能还没有可以返回的数据。如果它有 数据了，我们可以读取数据并返回 poll::Ready(data)，但如果数据没有准备好，我们这个future 就会阻塞并且不能继续执行。当没有数据可用时，我们需要注册 wake 函数，以在有数据可用时告诉执行 器我们的 future 准备好进一步操作。一个简单的 SocketReadfuture 可能像这样:

```rust
pub struct SocketRead<'a> {
    socket: &'a Socket,
}

impl SimpleFuture for SocketRead<'_> {
    type Output = Vec<u8>;

    fn poll(&mut self, wake: fn()) -> Poll<Self::Output> {
        if self.socket.has_data_to_read() {
            // The socket has data -- read it into a buffer and return it.
            Poll::Ready(self.socket.read_buf())
        } else {
            // The socket does not yet have data.
            //
            // Arrange for `wake` to be called once data is available.
            // When data becomes available, `wake` will be called, and the
            // user of this `Future` will know to call `poll` again and
            // receive data.
            self.socket.set_readable_callback(wake);
            Poll::Pending
        }
    }
}
```
Waker提供wake()方法来告诉执行器哪个关联任务应该要唤醒。当wake()函数被调用时， 执行器知道Waker关联的任务已经准备好继续了，并且任务的future会被轮询一遍。

Waker类型还实现了clone()，因此可以到处拷贝储存。

创建 Waker 的最简单方法是实现 ArcWake trait，然后使用 waker_by_ref 或者 .into_waker() 函数来把 Arc<impl ArcWake> 转变成 Waker。
