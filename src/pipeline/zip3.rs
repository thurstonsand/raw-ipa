use futures::Stream;
use futures_util::stream::{Fuse, FusedStream};
use futures_util::StreamExt;
use pin_project::pin_project;
use std::cmp;
use std::pin::Pin;
use std::task::{Context, Poll};

/// largely copied from implementation of [Zip], but for 3 streams
#[pin_project]
pub struct Zip3<St1: Stream, St2: Stream, St3: Stream> {
    #[pin]
    stream1: Fuse<St1>,
    #[pin]
    stream2: Fuse<St2>,
    #[pin]
    stream3: Fuse<St3>,
    queued1: Option<St1::Item>,
    queued2: Option<St2::Item>,
    queued3: Option<St3::Item>,
}

impl<St1: Stream, St2: Stream, St3: Stream> Zip3<St1, St2, St3> {
    #[allow(dead_code)]
    pub fn new(stream1: St1, stream2: St2, stream3: St3) -> Self {
        Self {
            stream1: stream1.fuse(),
            stream2: stream2.fuse(),
            stream3: stream3.fuse(),
            queued1: None,
            queued2: None,
            queued3: None,
        }
    }
}

impl<St1: Stream, St2: Stream, St3: Stream> FusedStream for Zip3<St1, St2, St3> {
    fn is_terminated(&self) -> bool {
        self.stream1.is_terminated() && self.stream2.is_terminated() && self.stream3.is_terminated()
    }
}

impl<St1: Stream, St2: Stream, St3: Stream> Stream for Zip3<St1, St2, St3> {
    type Item = (St1::Item, St2::Item, St3::Item);

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        if this.queued1.is_none() {
            match this.stream1.as_mut().poll_next(cx) {
                Poll::Ready(Some(item1)) => *this.queued1 = Some(item1),
                Poll::Ready(None) | Poll::Pending => {}
            }
        }
        if this.queued2.is_none() {
            match this.stream2.as_mut().poll_next(cx) {
                Poll::Ready(Some(item2)) => *this.queued2 = Some(item2),
                Poll::Ready(None) | Poll::Pending => {}
            }
        }
        if this.queued3.is_none() {
            match this.stream3.as_mut().poll_next(cx) {
                Poll::Ready(Some(item3)) => *this.queued3 = Some(item3),
                Poll::Ready(None) | Poll::Pending => {}
            }
        }

        if this.queued1.is_some() && this.queued2.is_some() && this.queued3.is_some() {
            let triplet = (
                this.queued1.take().unwrap(),
                this.queued2.take().unwrap(),
                this.queued3.take().unwrap(),
            );
            Poll::Ready(Some(triplet))
        } else if this.stream1.is_done() || this.stream2.is_done() || this.stream3.is_done() {
            Poll::Ready(None)
        } else {
            Poll::Pending
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let queued1_len = if self.queued1.is_some() { 1 } else { 0 };
        let queued2_len = if self.queued2.is_some() { 1 } else { 0 };
        let queued3_len = if self.queued3.is_some() { 1 } else { 0 };
        let (stream1_lower, stream1_upper) = self.stream1.size_hint();
        let (stream2_lower, stream2_upper) = self.stream2.size_hint();
        let (stream3_lower, stream3_upper) = self.stream3.size_hint();

        let stream1_lower = stream1_lower.saturating_add(queued1_len);
        let stream2_lower = stream2_lower.saturating_add(queued2_len);
        let stream3_lower = stream3_lower.saturating_add(queued3_len);

        let lower = cmp::min(stream1_lower, stream2_lower);
        let lower = cmp::min(lower, stream3_lower);

        let stream1_upper = stream1_upper.map(|upper| upper.saturating_add(queued1_len));
        let stream2_upper = stream2_upper.map(|upper| upper.saturating_add(queued2_len));
        let stream3_upper = stream3_upper.map(|upper| upper.saturating_add(queued3_len));
        let upper = vec![stream1_upper, stream2_upper, stream3_upper]
            .iter()
            .fold(Option::<usize>::None, |min, n| match (min, *n) {
                (Some(x), Some(y)) => {
                    let x = x.saturating_add(queued1_len);
                    let y = y.saturating_add(queued2_len);
                    Some(cmp::min(x, y))
                }
                (Some(x), None) => x.checked_add(queued1_len),
                (None, Some(y)) => y.checked_add(queued2_len),
                (None, None) => None,
            });

        (lower, upper)
    }
}
