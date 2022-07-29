use crate::field::Fp31;
use crate::pipeline::PipelineExt;
use crate::shamir::{LagrangePolynomial, SecretSharing, Share};
use futures::stream::StreamExt;
use futures::Stream;
use std::num::NonZeroU8;

/// The helper simply adds its shares together, and outputs the addition to be sent to the leader
/// for a final value computation.
pub struct Adder {}

impl Adder {
    #[allow(clippy::unused_self)] // may need to use it eventually
    pub fn run(
        &self,
        source: impl Stream<Item = (Share<Fp31>, Share<Fp31>)>,
    ) -> impl Stream<Item = Share<Fp31>> {
        source.map(|(share_x, share_y)| &share_x + &share_y)
    }
}

/// The client collects the 3 shares from the helpers and reconstructs the cleartext output of the
/// addition.
pub struct Client {
    k: NonZeroU8,
}

impl Client {
    /// if the client simply wants to print the results, use this
    #[allow(dead_code)]
    pub async fn print_results(
        &self,
        h1: impl Stream<Item = Share<Fp31>> + Unpin,
        h2: impl Stream<Item = Share<Fp31>> + Unpin,
        h3: impl Stream<Item = Share<Fp31>> + Unpin,
    ) {
        let mut results = self.reconstruct(h1, h2, h3);
        println!("Results:");
        while let Some(res) = results.next().await {
            println!("{:?}", res);
        }
    }

    /// takes in the outputs of the 3 helper streams, and produces a new stream of results
    pub fn reconstruct(
        &self,
        h1: impl Stream<Item = Share<Fp31>> + Unpin,
        h2: impl Stream<Item = Share<Fp31>> + Unpin,
        h3: impl Stream<Item = Share<Fp31>> + Unpin,
    ) -> impl Stream<Item = Fp31> {
        let k = self.k;
        h1.zip3(h2, h3).map(move |(h1_share, h2_share, h3_share)| {
            SecretSharing::reconstruct(
                &[h1_share, h2_share, h3_share],
                &LagrangePolynomial::new(k).unwrap(),
            )
            .unwrap()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::Field;
    use futures::stream::StreamExt;
    use futures_util::SinkExt;
    use rand::{thread_rng, Rng};
    use std::future;

    fn gen_numbers(
        len: usize,
    ) -> (
        impl Stream<Item = (NonZeroU8, NonZeroU8)>,
        impl Stream<Item = Fp31>,
    ) {
        let (mut compute_tx, compute_rx) = futures::channel::mpsc::channel(1);
        let (mut expected_tx, expected_rx) = futures::channel::mpsc::channel(1);
        tokio::spawn(async move {
            for _ in 0..len {
                let x = NonZeroU8::new(thread_rng().gen_range(1..Fp31::PRIME)).unwrap();
                let y = NonZeroU8::new(thread_rng().gen_range(1..Fp31::PRIME)).unwrap();
                compute_tx.send((x, y)).await.unwrap();
                // compute the result of the addition
                expected_tx
                    .send({
                        let x = Fp31::from(x.get());
                        let y = Fp31::from(y.get());
                        x + y
                    })
                    .await
                    .unwrap();
            }
        });
        (compute_rx, expected_rx)
    }

    fn gen_shares<St: Stream<Item = (NonZeroU8, NonZeroU8)> + Send + 'static>(
        stream: St,
        k: NonZeroU8,
        n: NonZeroU8,
    ) -> (
        impl Stream<Item = (Share<Fp31>, Share<Fp31>)>,
        impl Stream<Item = (Share<Fp31>, Share<Fp31>)>,
        impl Stream<Item = (Share<Fp31>, Share<Fp31>)>,
    ) {
        let mut unzipped = stream
            .map(move |(x, y)| {
                let shamir = SecretSharing::new(k, n).unwrap();
                let x_shares = shamir.split(Fp31::from(x.get()), thread_rng());
                let y_shares = shamir.split(Fp31::from(y.get()), thread_rng());
                let share0 = (x_shares[0].clone(), y_shares[0].clone());
                let share1 = (x_shares[1].clone(), y_shares[1].clone());
                let share2 = (x_shares[2].clone(), y_shares[2].clone());
                (share0, share1, share2)
            })
            .unzip3(1);
        let (shares_0, shares_1, shares_2) = unzipped.output();
        // must include this in order to drive the values from the `unzipped` stream into the
        // `shares_*` streams
        tokio::spawn(async move { unzipped.await.expect("unzip should complete without error") });

        (shares_0, shares_1, shares_2)
    }

    #[tokio::test]
    async fn add_pipeline() {
        // generate the stream of numbers to add
        let (nums_iter, expected) = gen_numbers(10);
        let k = NonZeroU8::new(3).unwrap();
        let n = NonZeroU8::new(4).unwrap();

        let (shares_1, shares_2, shares_3) = gen_shares(nums_iter, k, n);

        let helper1 = Adder {};
        let helper2 = Adder {};
        let helper3 = Adder {};

        let helper1_res = helper1.run(shares_1);
        let helper2_res = helper2.run(shares_2);
        let helper3_res = helper3.run(shares_3);

        let reconstructed = Client { k }.reconstruct(helper1_res, helper2_res, helper3_res);
        expected
            .zip(reconstructed)
            .for_each(|(expected, reconstructed)| {
                assert_eq!(expected, reconstructed);
                future::ready(())
            })
            .await;
    }
}
