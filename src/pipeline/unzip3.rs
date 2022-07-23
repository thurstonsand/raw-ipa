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
    sender_a: Sender<A>,
    sender_b: Sender<B>,
    sender_c: Sender<C>,
    receiver_a: Option<Receiver<A>>,
    receiver_b: Option<Receiver<B>>,
    receiver_c: Option<Receiver<C>>,
    buffered_a: Option<A>,
    buffered_b: Option<B>,
    buffered_c: Option<C>,
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
            buffered_a: None,
            buffered_b: None,
            buffered_c: None,
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

macro_rules! sink_ready {
    ($sink:expr, $cx:expr $(,)?) => {
        match futures::ready!($sink.poll_ready($cx)) {
            std::result::Result::Ok(_) => &mut *$sink, // TODO: why?
            err => return std::task::Poll::Ready(err),
        }
    };
}

impl<A, B, C, InpSt: Stream<Item = (A, B, C)>> Future for Unzip3<A, B, C, InpSt> {
    type Output = Result<(), SendError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        loop {
            // "sink_ready!" will immediately return if sink is not ready.
            // thus, we only "take()" the buffered value when we can send it.
            // in other words, "sink_ready!" must come before "buffered_*.take()"
            if this.buffered_a.is_some() {
                sink_ready!(this.sender_a, cx)
                    .start_send(this.buffered_a.take().unwrap())
                    .unwrap();
            }
            if this.buffered_b.is_some() {
                sink_ready!(this.sender_b, cx)
                    .start_send(this.buffered_b.take().unwrap())
                    .unwrap();
            }
            if this.buffered_c.is_some() {
                sink_ready!(this.sender_c, cx)
                    .start_send(this.buffered_c.take().unwrap())
                    .unwrap();
            }

            match ready!(this.inp.as_mut().poll_next(cx)) {
                Some((a, b, c)) => {
                    *this.buffered_a = Some(a);
                    *this.buffered_b = Some(b);
                    *this.buffered_c = Some(c);
                }
                None => return Poll::Ready(Ok(())),
            }
        }
    }
}
