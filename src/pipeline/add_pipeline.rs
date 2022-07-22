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
