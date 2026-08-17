//! # Extended Pipeline States
//!
//! Additional state types and type-level features for the pipeline.

use std::marker::PhantomData;

// ============================================================================
// TYPE-LEVEL FEATURE FLAGS
// ============================================================================

/// Marker trait for pipeline features
pub trait Feature {}

/// Feature: Git history analysis
pub struct GitHistory;
impl Feature for GitHistory {}

/// Feature: Cross-file reference resolution
pub struct CrossFileRefs;
impl Feature for CrossFileRefs {}

/// Feature: Call graph extraction
pub struct CallGraph;
impl Feature for CallGraph {}

/// Feature: Type inference
pub struct TypeInference;
impl Feature for TypeInference {}

// ============================================================================
// TYPE-LEVEL FEATURE SETS
// ============================================================================

/// A set of features enabled for the pipeline
pub trait FeatureSet {}

impl FeatureSet for frunk::HNil {}
impl<H: Feature, T: FeatureSet> FeatureSet for frunk::HCons<H, T> {}

/// Check if a feature set contains a specific feature
pub trait HasFeature<F: Feature> {}

impl<F: Feature, T> HasFeature<F> for frunk::HCons<F, T> {}

// Note: Recursive case removed due to Rust's coherence rules.
// In a real implementation, use specialization or a different pattern.

// ============================================================================
// FEATURE-GATED PIPELINE
// ============================================================================

/// A pipeline with compile-time feature tracking
pub struct FeaturePipeline<State, Features: FeatureSet> {
    pub state: PhantomData<State>,
    pub features: PhantomData<Features>,
}

impl<S, F: FeatureSet> FeaturePipeline<S, F> {
    /// Add a feature to the pipeline
    pub fn with_feature<NewFeature: Feature>(
        self,
    ) -> FeaturePipeline<S, frunk::HCons<NewFeature, F>> {
        FeaturePipeline {
            state: PhantomData,
            features: PhantomData,
        }
    }
}

// ============================================================================
// SESSION TYPES (Protocol Guarantees)
// ============================================================================

/// Protocol marker trait
pub trait Protocol {}

/// Initial protocol state
pub struct ProtocolInit;
impl Protocol for ProtocolInit {}

/// Connected to store
pub struct ProtocolConnected;
impl Protocol for ProtocolConnected {}

/// Transaction in progress
pub struct ProtocolTransaction;
impl Protocol for ProtocolTransaction {}

/// Protocol completed
pub struct ProtocolDone;
impl Protocol for ProtocolDone {}

/// A session that enforces correct protocol ordering
pub struct Session<P: Protocol> {
    _protocol: PhantomData<P>,
}

impl Session<ProtocolInit> {
    pub fn new() -> Self {
        Self {
            _protocol: PhantomData,
        }
    }

    /// Connect to the store
    pub fn connect(self) -> Session<ProtocolConnected> {
        Session {
            _protocol: PhantomData,
        }
    }
}

impl Default for Session<ProtocolInit> {
    fn default() -> Self {
        Self::new()
    }
}

impl Session<ProtocolConnected> {
    /// Begin a transaction
    pub fn begin_transaction(self) -> Session<ProtocolTransaction> {
        Session {
            _protocol: PhantomData,
        }
    }
}

impl Session<ProtocolTransaction> {
    /// Commit the transaction
    pub fn commit(self) -> Session<ProtocolDone> {
        Session {
            _protocol: PhantomData,
        }
    }

    /// Rollback the transaction
    pub fn rollback(self) -> Session<ProtocolConnected> {
        Session {
            _protocol: PhantomData,
        }
    }
}

impl Session<ProtocolDone> {
    /// Get the result
    pub fn result(self) -> bool {
        true
    }
}

// ============================================================================
// INVARIANT CONTRACTS
// ============================================================================

/// A value with an attached invariant that must hold
#[derive(Debug)]
pub struct Invariant<T, I> {
    value: T,
    _invariant: PhantomData<I>,
}

/// Marker for "non-empty collection"
pub struct NonEmpty;

/// Marker for "sorted collection"
pub struct Sorted;

/// Marker for "validated data"
pub struct ValidatedData;

impl<T, I> Invariant<T, I> {
    /// Access the inner value (invariant is guaranteed by type)
    pub fn get(&self) -> &T {
        &self.value
    }

    /// Consume and return the inner value
    pub fn into_inner(self) -> T {
        self.value
    }
}

impl<T> Invariant<Vec<T>, NonEmpty> {
    /// Create a non-empty vector (fails at runtime if empty)
    pub fn non_empty(vec: Vec<T>) -> Option<Self> {
        if vec.is_empty() {
            None
        } else {
            Some(Invariant {
                value: vec,
                _invariant: PhantomData,
            })
        }
    }

    /// Get the first element (guaranteed to exist)
    pub fn first(&self) -> &T {
        // Safe because NonEmpty invariant guarantees non-empty
        &self.value[0]
    }

    /// Get the length (guaranteed > 0)
    pub fn len(&self) -> usize {
        self.value.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_protocol() {
        // This test verifies the session type transitions
        let session = Session::new();
        let connected = session.connect();
        let tx = connected.begin_transaction();
        let done = tx.commit();
        assert!(done.result());
    }

    #[test]
    fn non_empty_invariant() {
        let vec = vec![1, 2, 3];
        let non_empty = Invariant::<_, NonEmpty>::non_empty(vec).unwrap();
        assert_eq!(*non_empty.first(), 1);
        assert_eq!(non_empty.len(), 3);

        // Empty vector returns None
        let empty: Vec<i32> = vec![];
        assert!(Invariant::<_, NonEmpty>::non_empty(empty).is_none());
    }

    #[test]
    fn feature_pipeline() {
        type BaseFeatures = frunk::HNil;
        type WithCallGraph = frunk::HCons<CallGraph, BaseFeatures>;
        type WithGitHistory = frunk::HCons<GitHistory, BaseFeatures>;

        // Compile-time check that features are present (head of HList only)
        fn requires_call_graph<F: HasFeature<CallGraph>>() {}
        fn requires_git<F: HasFeature<GitHistory>>() {}

        requires_call_graph::<WithCallGraph>();
        requires_git::<WithGitHistory>();

        // Note: Recursive checking removed due to Rust coherence rules.
        // In a full implementation, use specialization or a different pattern.
    }
}
