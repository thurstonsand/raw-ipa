use crate::field::Fp31;
use crate::pipeline::PipelineExt;
use crate::shamir::{LagrangePolynomial, SecretSharing, Share};
use futures::stream::StreamExt;
use futures::Stream;
use std::num::NonZeroU8;

/// The leader in a shamir secret share addition computation. The leader both adds together its
/// shares, and receives the added shares from its 2 helpers in order to output a sum.
/// must know `k`, how many shares are required to reconstitute the original value via the lagrange
/// polynomial.
pub struct Leader {
    k: NonZeroU8,
}

impl Leader {
    /// accepts shares to add, as well as the added shares from its two helpers
    /// outputs the sum of the original numbers
    pub fn run(
        &self,
        source: impl Stream<Item = (Share<Fp31>, Share<Fp31>)>,
        h1_res: impl Stream<Item = Share<Fp31>>,
        h2_res: impl Stream<Item = Share<Fp31>>,
    ) -> impl Stream<Item = Fp31> {
        let k = self.k;
        source
            .map(|(x_share, y_share)| &x_share + &y_share)
            .zip3(h1_res, h2_res)
            .map(move |(source_share, h1_share, h2_share)| {
                SecretSharing::reconstruct(
                    &[source_share, h1_share, h2_share],
                    &LagrangePolynomial::new(k).unwrap(),
                )
                .unwrap()
            })
    }
}

/// The helper simply adds its shares together, and outputs the addition to be sent to the leaer
/// for a final value computation.
pub struct Helper {}

impl Helper {
    #[allow(clippy::unused_self)] // may need to use it eventually
    pub fn run(
        &self,
        source: impl Stream<Item = (Share<Fp31>, Share<Fp31>)>,
    ) -> impl Stream<Item = Share<Fp31>> {
        source.map(|(share_x, share_y)| &share_x + &share_y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::Field;
    use futures::stream::StreamExt;
    use rand::rngs::mock::StepRng;
    use rand::{thread_rng, Rng};

    fn gen_numbers(len: usize) -> impl Stream<Item = (NonZeroU8, NonZeroU8)> {
        let mut rng = StepRng::new(1, 1);

        futures::stream::iter(std::iter::from_fn(move || {
            let x = NonZeroU8::new(rng.gen_range(1..Fp31::PRIME)).unwrap();
            let y = NonZeroU8::new(rng.gen_range(1..Fp31::PRIME)).unwrap();
            Some((x, y))
        }))
        .take(len)
    }

    #[tokio::test]
    async fn add_pipeline() {
        // generate the stream of numbers to add
        let nums_iter = gen_numbers(10);
        let k = NonZeroU8::new(3).unwrap();
        let n = NonZeroU8::new(4).unwrap();

        let mut unzipped = nums_iter
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
        let (shares_1, shares_2, shares_3) = unzipped.output();

        // must include this in order to drive the values from the `unzipped` stream into the
        // `shares_*` streams
        tokio::spawn(async move {
            unzipped.await.expect("unzip should complete");
        });

        let leader = Leader { k };
        let helper1 = Helper {};
        let helper2 = Helper {};

        let helper1_res = helper1.run(shares_1);
        let helper2_res = helper2.run(shares_2);
        let mut leader_res = leader.run(shares_3, helper1_res, helper2_res);
        while let Some(res) = leader_res.next().await {
            println!("{:?}", res);
        }
    }
}
