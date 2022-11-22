use crate::ff::Field;
use crate::helpers::messaging::{Gateway, Mesh};
use crate::helpers::Role;
use crate::protocol::context::{semi_honest, Context, SemiHonestContext};
use crate::protocol::malicious::SecurityValidatorAccumulator;
use crate::protocol::prss::{
    Endpoint as PrssEndpoint, IndexedSharedRandomness, SequentialSharedRandomness,
};
use crate::protocol::{Step, Substep};
use crate::secret_sharing::{MaliciousReplicated, Replicated};
use std::borrow::Borrow;
use std::sync::Arc;

/// Represents protocol context in malicious setting, i.e. secure against one active adversary
/// in 3 party MPC ring.
#[derive(Clone, Debug)]
pub struct MaliciousContext<'a, F: Field> {
    /// TODO (alex): Arc is required here because of the `TestWorld` structure. Real world
    /// may operate with raw references and be more efficient
    inner: Arc<ContextInner<'a, F>>,
    step: Step,
}

impl<'a, F: Field> MaliciousContext<'a, F> {
    pub(super) fn new(
        source: &SemiHonestContext<'a, F>,
        acc: SecurityValidatorAccumulator<F>,
        r_share: Replicated<F>,
    ) -> Self {
        Self {
            inner: ContextInner::new(&source.inner, acc, r_share),
            step: source.step().clone(),
        }
    }

    #[must_use]
    pub fn accumulator(&self) -> SecurityValidatorAccumulator<F> {
        self.inner.accumulator.clone()
    }

    /// Sometimes it is required to reinterpret malicious context as semi-honest. Ideally
    /// protocols should be generic over `SecretShare` trait and not requiring this cast and taking
    /// `ProtocolContext<'a, S: SecretShare<F>, F: Field>` as the context. If that is not possible,
    /// this implementation makes it easier to reinterpret the context as semi-honest.
    ///
    /// The context received will be an exact copy of malicious, so it will be tied up to the same step
    /// and prss.
    #[must_use]
    pub fn to_semi_honest(self) -> SemiHonestContext<'a, F> {
        // TODO: it can be made more efficient by impersonating malicious context as semi-honest
        // it does not work as of today because of https://github.com/rust-lang/rust/issues/20400
        // while it is possible to define a struct that wraps a reference to malicious context
        // and implement `Context` trait for it, implementing SecureMul and Reveal for Context
        // is not
        // For the same reason, it is not possible to implement Context<F, Share = Replicated<F>>
        // for `MaliciousContext`. Deep clone is the only option
        let mut ctx = SemiHonestContext::new(self.inner.role, self.inner.prss, self.inner.gateway);
        ctx.step = self.step;

        ctx
    }
}

impl<'a, F: Field> Context<F> for MaliciousContext<'a, F> {
    type Share = MaliciousReplicated<F>;

    fn role(&self) -> Role {
        self.inner.role
    }

    fn step(&self) -> &Step {
        &self.step
    }

    fn narrow<S: Substep + ?Sized>(&self, step: &S) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            step: self.step.narrow(step),
        }
    }

    fn prss(&self) -> Arc<IndexedSharedRandomness> {
        self.inner.prss.indexed(self.step())
    }

    fn prss_rng(&self) -> (SequentialSharedRandomness, SequentialSharedRandomness) {
        self.inner.prss.sequential(self.step())
    }

    fn mesh(&self) -> Mesh<'_, '_> {
        self.inner.gateway.mesh(self.step())
    }

    fn share_of_one(&self) -> <Self as Context<F>>::Share {
        MaliciousReplicated::one(self.role(), self.inner.r_share.clone())
    }
}

#[derive(Debug)]
struct ContextInner<'a, F: Field> {
    role: Role,
    prss: &'a PrssEndpoint,
    gateway: &'a Gateway,
    accumulator: SecurityValidatorAccumulator<F>,
    r_share: Replicated<F>,
}

impl<'a, F: Field> ContextInner<'a, F> {
    fn new<B: Borrow<semi_honest::ContextInner<'a>>>(
        source: &B,
        accumulator: SecurityValidatorAccumulator<F>,
        r_share: Replicated<F>,
    ) -> Arc<Self> {
        let source = source.borrow();
        Arc::new(ContextInner {
            role: source.role,
            prss: source.prss,
            gateway: source.gateway,
            accumulator,
            r_share,
        })
    }
}
