use crate::field::Fp31;
use crate::pipeline::PipelineExt;
use crate::shamir::{LagrangePolynomial, SecretSharing, Share};
use futures::stream::StreamExt;
use futures::Stream;
use std::num::NonZeroU8;

#[allow(dead_code)]
struct AddPipelineLeader {
    k: NonZeroU8,
}

impl AddPipelineLeader {
    #[allow(dead_code)]
    fn run(
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

#[allow(dead_code)]
struct AddPipelineHelper {}

impl AddPipelineHelper {
    #[allow(dead_code)]
    fn run(
        source: impl Stream<Item = (Share<Fp31>, Share<Fp31>)>,
    ) -> impl Stream<Item = Share<Fp31>> {
        source.map(|(share_x, share_y)| &share_x + &share_y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::{ Field};
    use rand::rngs::mock::StepRng;
    use rand::{Rng, thread_rng};
    use futures::stream::StreamExt;

    #[test]
    fn add_pipeline() {
        let num_computations = 10_000;

        let mut rng = StepRng::new(1, 1);
        let mut nums_iter = futures::stream::iter(std::iter::from_fn(move || {
            let x = NonZeroU8::new(rng.gen_range(1..Fp31::PRIME)).unwrap();
            let y = NonZeroU8::new(rng.gen_range(1..Fp31::PRIME)).unwrap();
            Some((x, y))
        })).take(num_computations);

        let k = NonZeroU8::new(3).unwrap();
        let n = NonZeroU8::new(4).unwrap();
        let shamir = SecretSharing::new(k, n).unwrap();
        let lc = LagrangePolynomial::<Fp31>::new(n).unwrap();

        let nums_iter = nums_iter.map(|(x, y)| {
            let x_shares = shamir.split(Fp31::from(x.get() as u128), thread_rng());
            let y_shares = shamir.split(Fp31::from(y.get() as u128), thread_rng());
            let share0 = (x_shares[0].clone(), y_shares[0].clone());
            let share1 = (x_shares[1].clone(), y_shares[1].clone());
            let share2 = (x_shares[2].clone(), y_shares[2].clone());
            (share0, share1, share2)
        });
    }
}
