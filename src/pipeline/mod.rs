#![allow(clippy::unnecessary_fold)] // contrived example; also, `.sum()` is not implemented for stream

mod add_pipeline;
mod incrementer_adder;
mod map2;
mod sum;
mod unzip3;
mod zip3;

use futures::{FutureExt, Stream, StreamExt};
use incrementer_adder::IncrementerAdder;
use map2::Map2;
use sum::Sum;
use unzip3::Unzip3;
use zip3::Zip3;

/// allow for new methods to be available on any [Stream] type
trait PipelineExt: Stream {
    /// recreate the `map` function on a stream to show that it's possible
    fn map2<T, F: FnMut(Self::Item) -> T>(self, f: F) -> Map2<Self, F>
    where
        Self: Sized,
    {
        Map2::new(self, f)
    }
    /// a custom, stateful stream operation. Trivial example, but if the stream is a repeating
    /// number before this, `incrementer_adder` will convert it to a range generator
    /// e.g.
    /// input stream: \[0, 0, 0, 0, 0\]
    /// output stream: \[1, 2, 3, 4, 5\]
    fn incrementer_adder(self) -> IncrementerAdder<Self>
    where
        Self: Sized,
        Self::Item: Default,
    {
        IncrementerAdder::new(self)
    }
    /// a custom, stateful future operation. Consumes a stream, and produces a future of the sum
    /// of all members of the stream
    fn sum(self) -> Sum<Self>
    where
        Self: Sized,
        Self::Item: Default,
    {
        Sum::new(self)
    }

    fn zip3<St1: Stream, St2: Stream>(self, other1: St1, other2: St2) -> Zip3<Self, St1, St2>
    where
        Self: Sized,
    {
        Zip3::new(self, other1, other2)
    }

    fn unzip3<A, B, C>(self, buffer: usize) -> Unzip3<A, B, C, Self>
    where
        Self: Sized + Stream<Item = (A, B, C)>,
    {
        Unzip3::new(self, buffer)
    }
}

/// magic so that any [`Stream`] implements [`PipelineExt`]
impl<T: Stream> PipelineExt for T {}

// this is the first of a pair of pipelines to process a stream of data
// in this stage, simply pair up values, then multiply them together
// the output of this pipeline is a stream, to be consumed by the next pipeline
pub struct PairAndMultiply {}
impl PairAndMultiply {
    #[allow(clippy::unused_self)]
    pub fn run(&self, inp: impl Stream<Item = i32>) -> impl Stream<Item = i32> {
        // incrementer_adder turns a stream of [0, 0, 0...] into a Range
        inp.incrementer_adder()
            // chunks provides a way to buffer results as needed.
            // for instance, if deserializing a certain number of bytes, you could chunk them
            .chunks(2)
            // map2 provides a way to act on the collected chunk.
            // in this case, multiply the 2 chunked values
            // same as built-in map, but implemented to show how to implement familiar functionality
            .map2(|v|
                // panics if invalid number of integers in stream; must be even number
                v[0] * v[1])
    }
}

// this is the second of a pair of pipelines to process a stream of data.
// in this stage, sum all values, and then to_string() the final sum.
// the output of this pipeline is a string of the sum
pub struct SumAndStringify {}
impl SumAndStringify {
    pub async fn run(&self, inp: impl Stream<Item = i32>) -> String {
        // sum consumes a Stream and produces a Future with the single value sum
        inp.sum()
            // inp.fold(0, |acc, i| async move { acc + i })
            // finally, it is possible to further work on the value, since it is within a future.
            // alternatively, just `await` the future and act on the value at that time.
            // in this case, stringify
            .map(|i| i.to_string())
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    #[tokio::test]
    async fn usage() {
        let pipe = MultiplySumAndStringify {};
        let inp = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let expected_res = inp
            .chunks(2)
            .map(|v| v[0] * v[1])
            .fold(0, |acc, i| acc + i)
            .to_string();
        let inp_stream = stream::iter(inp);
        let res = pipe.run(inp_stream).await;
        assert_eq!(expected_res, res)
    }
}
