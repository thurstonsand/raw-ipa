use std::marker::PhantomData;
use std::sync::Arc;

use super::{
    prss::{IndexedSharedRandomness, SequentialSharedRandomness},
    RecordId, Step, UniqueStepId,
};
use crate::{
    ff::Field,
    helpers::{
        messaging::{Gateway, Mesh},
        Role,
    },
    protocol::{malicious::SecurityValidatorAccumulator, prss::Endpoint as PrssEndpoint},
};

use crate::secret_sharing::{MaliciousReplicated, Replicated, SecretSharing};

/// Context used by each helper to perform computation. Currently they need access to shared
/// randomness generator (see `Participant`) and communication trait to send messages to each other.
#[derive(Clone, Debug)]
pub struct ProtocolContext<'a, S: SecretSharing<F>, F> {
    role: Role,
    step: UniqueStepId,
    prss: &'a PrssEndpoint,
    gateway: &'a Gateway,
    accumulator: Option<SecurityValidatorAccumulator<F>>,
    record_id: Option<RecordId>,
    _marker: PhantomData<S>,
}

impl<'a, F: Field, SS: SecretSharing<F>> ProtocolContext<'a, SS, F> {
    pub fn new(role: Role, participant: &'a PrssEndpoint, gateway: &'a Gateway) -> Self {
        Self {
            role,
            step: UniqueStepId::default(),
            prss: participant,
            gateway,
            accumulator: None,
            record_id: None,
            _marker: PhantomData::default(),
        }
    }

    /// The role of this context.
    #[must_use]
    pub fn role(&self) -> Role {
        self.role
    }

    /// A unique identifier for this stage of the protocol execution.
    #[must_use]
    pub fn step(&self) -> &UniqueStepId {
        &self.step
    }

    /// Make a sub-context.
    /// Note that each invocation of this should use a unique value of `step`.
    #[must_use]
    pub fn narrow<S: Step + ?Sized>(&self, step: &S) -> Self {
        ProtocolContext {
            role: self.role,
            step: self.step.narrow(step),
            prss: self.prss,
            gateway: self.gateway,
            accumulator: self.accumulator.clone(),
            record_id: self.record_id,
            _marker: PhantomData::default(),
        }
    }

    #[must_use]
    /// Make a sub-context which is bound to a record in case the same step is bound to a different `record_id`
    /// # Panics
    /// Panics in case the context is already bound to the same `record_id`
    pub fn bind(&self, record_id: RecordId) -> Self {
        if let Some(prev_record_id) = self.record_id {
            panic!(
                "Cannot bind to {record_id:?} because already bound to record: {prev_record_id:?}"
            )
        }

        ProtocolContext {
            role: self.role,
            // create a unique step that allows narrowing this context to the same step
            // if it is bound to a different record id
            step: UniqueStepId::from_step_id(&self.step),
            prss: self.prss,
            gateway: self.gateway,
            accumulator: self.accumulator.clone(),
            record_id: Some(record_id),
            _marker: PhantomData::default(),
        }
    }

    /// Get the indexed PRSS instance for this step.  It is safe to call this function
    /// multiple times.
    ///
    /// # Panics
    /// If `prss_rng()` is invoked for the same context, this will panic.  Use of
    /// these two functions are mutually exclusive.
    #[must_use]
    pub fn prss(&self) -> Arc<IndexedSharedRandomness> {
        self.prss.indexed(&self.step)
    }

    /// Get a pair of PRSS-based RNGs.  The first is shared with the helper to the "left",
    /// the second is shared with the helper to the "right".
    ///
    /// # Panics
    /// This method can only be called once.  This is also mutually exclusive with `prss()`.
    /// This will panic if you have previously invoked `prss()`.
    #[must_use]
    pub fn prss_rng(&self) -> (SequentialSharedRandomness, SequentialSharedRandomness) {
        self.prss.sequential(&self.step)
    }

    /// Get a set of communications channels to different peers.
    #[must_use]
    pub fn mesh(&self) -> Mesh<'_, '_> {
        self.gateway.mesh(&self.step)
    }
}

/// Implementation to upgrade semi-honest context to malicious. Only works for replicated secret
/// sharing because it is not known yet how to do it for any other type of secret sharing.
impl<'a, F: Field> ProtocolContext<'a, Replicated<F>, F> {
    #[must_use]
    pub fn upgrade_to_malicious(
        self,
        accumulator: SecurityValidatorAccumulator<F>,
    ) -> ProtocolContext<'a, MaliciousReplicated<F>, F> {
        ProtocolContext {
            role: self.role,
            step: self.step,
            prss: self.prss,
            gateway: self.gateway,
            accumulator: Some(accumulator),
            record_id: self.record_id,
            _marker: PhantomData::default(),
        }
    }
}

/// Implementation that is specific to malicious contexts operating over replicated secret sharings.
impl<'a, F: Field> ProtocolContext<'a, MaliciousReplicated<F>, F> {
    /// Get the accumulator that collects messages MACs.
    ///
    /// ## Panics
    /// Does not panic in normal circumstances, panic here will indicate a bug in protocol context
    /// setup that left the accumulator field empty inside the malicious context.
    #[must_use]
    pub fn accumulator(&self) -> SecurityValidatorAccumulator<F> {
        self.accumulator
            .as_ref()
            .expect("Accumulator must be set in the context in order to perform maliciously secure multiplication")
            .clone()
    }

    /// In some occasions it is required to reinterpret malicious context as semi-honest. Ideally
    /// protocols should be generic over `SecretShare` trait and not requiring this cast and taking
    /// `ProtocolContext<'a, S: SecretShare<F>, F: Field>` as the context. If that is not possible,
    /// this implementation makes it easier to reinterpret the context as semi-honest.
    ///
    /// The context received will be an exact copy of malicious, so it will be tied up to the same step
    /// and prss.
    #[must_use]
    pub fn to_semi_honest(self) -> ProtocolContext<'a, Replicated<F>, F> {
        ProtocolContext {
            role: self.role,
            step: self.step,
            prss: self.prss,
            gateway: self.gateway,
            accumulator: None,
            record_id: self.record_id,
            _marker: PhantomData::default(),
        }
    }
}
