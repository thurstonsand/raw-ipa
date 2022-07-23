use futures::channel::mpsc::{self, Receiver, SendError, Sender};
use futures::{ready, Stream};
use pin_project::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

#[pin_project]
pub struct Unzip3<A, B, C, InpSt: Stream<Item = (A, B, C)>> {
    #[pin]
    inp: InpSt,
    #[pin]
    sender_a: Sender<A>,
    #[pin]
    sender_b: Sender<B>,
    #[pin]
    sender_c: Sender<C>,
    receiver_a: Option<Receiver<A>>,
    receiver_b: Option<Receiver<B>>,
    receiver_c: Option<Receiver<C>>,
}

impl<A, B, C, InpSt: Stream<Item = (A, B, C)>> Unzip3<A, B, C, InpSt> {
    pub fn new(inp: InpSt) -> Self {
        let (txa, rxa) = mpsc::channel(1);
        let (txb, rxb) = mpsc::channel(1);
        let (txc, rxc) = mpsc::channel(1);
        Unzip3 {
            inp,
            sender_a: txa,
            sender_b: txb,
            sender_c: txc,
            receiver_a: Some(rxa),
            receiver_b: Some(rxb),
            receiver_c: Some(rxc),
        }
    }

    pub fn output(
        &mut self,
    ) -> (
        impl Stream<Item = A>,
        impl Stream<Item = B>,
        impl Stream<Item = C>,
    ) {
        let stream1 = self.receiver_a.take();
        let stream2 = self.receiver_b.take();
        let stream3 = self.receiver_c.take();
        let err_msg = "output should only be called once";
        (
            stream1.expect(err_msg),
            stream2.expect(err_msg),
            stream3.expect(err_msg),
        )
    }
}

macro_rules! send_data {
    ($cx:expr, $sender:expr, $msg:expr $(,)?) => {{
        let poll_ready = futures::ready!($sender.poll_ready($cx));
        if poll_ready.is_err() {
            return std::task::Poll::Ready(poll_ready);
        }
        $sender.start_send($msg).unwrap();
    }};
}
impl<A, B, C, InpSt: Stream<Item = (A, B, C)>> Future for Unzip3<A, B, C, InpSt> {
    type Output = Result<(), SendError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        loop {
            match ready!(this.inp.as_mut().poll_next(cx)) {
                Some((a, b, c)) => {
                    send_data!(cx, this.sender_a, a);
                    send_data!(cx, this.sender_b, b);
                    send_data!(cx, this.sender_c, c);
                }
                None => return Poll::Ready(Ok(())),
            }
        }
    }
}
