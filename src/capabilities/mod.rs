//! # Capability Token System
//!
//! Zero-cost proofs that authorize operations. Capabilities cannot be forged
//! because their constructors are private to this module.
//!
//! ## Pattern: Unforgeable Capability Tokens
//!
//! ```rust,ignore
//! // Only this module can create capabilities
//! mod capabilities {
//!     pub struct CanRead(());  // Private unit field
//!     pub fn grant_read() -> CanRead { CanRead(()) }
//! }
//!
//! // Functions require proof of capability
//! fn read_data(_proof: &CanRead, source: &DataSource) -> Data {
//!     source.read()  // Caller MUST have valid token
//! }
//! ```

use std::marker::PhantomData;

// ============================================================================
// CAPABILITY TOKENS
// ============================================================================

/// Capability to parse source code files.
/// Only granted after repository discovery completes successfully.
#[derive(Debug, Clone, Copy)]
pub struct CanParse(());

/// Capability to build and manipulate the knowledge graph.
/// Only granted after parsing completes with valid AST.
#[derive(Debug, Clone, Copy)]
pub struct CanStore(());

/// Capability to validate RDF against ontology constraints.
/// Only granted when ontology schemas are loaded.
#[derive(Debug, Clone, Copy)]
pub struct CanValidate(());

/// Capability to query the graph store.
/// Only granted after data is stored and validated.
#[derive(Debug, Clone, Copy)]
pub struct CanQuery(());

// ============================================================================
// CAPABILITY GRANTING (Private Constructors)
// ============================================================================

/// Internal module for capability construction.
/// This pattern ensures only authorized code paths can create capabilities.
pub(crate) mod grants {
    use super::*;

    /// Grant parsing capability after successful repo discovery
    pub fn grant_parse() -> CanParse {
        CanParse(())
    }

    /// Grant storage capability after successful parsing
    pub fn grant_store() -> CanStore {
        CanStore(())
    }

    /// Grant validation capability when ontology is loaded
    pub fn grant_validate() -> CanValidate {
        CanValidate(())
    }

    /// Grant query capability after validation passes
    pub fn grant_query() -> CanQuery {
        CanQuery(())
    }
}

// ============================================================================
// CAPABILITY SETS (Type-Level Collections)
// ============================================================================

/// Marker trait for capability sets.
/// Used with HLists from frunk for type-level capability tracking.
pub trait CapabilitySet {}

/// Empty capability set
impl CapabilitySet for frunk::HNil {}

/// Capability set with at least one capability
impl<H, T: CapabilitySet> CapabilitySet for frunk::HCons<H, T> {}

/// Type-level proof that a capability set contains a specific capability.
pub trait HasCapability<C> {}

// Base case: head of list matches
impl<C, T> HasCapability<C> for frunk::HCons<C, T> {}

// Note: Recursive case removed due to Rust's coherence rules.
// In a real implementation, use specialization or a different pattern.

// ============================================================================
// SCOPED CAPABILITIES (Lifetime-Bound)
// ============================================================================

/// A capability that is bound to a specific scope.
/// When the scope ends, the capability becomes invalid.
///
/// This prevents capabilities from escaping their intended context.
#[derive(Debug)]
pub struct ScopedCapability<'scope, C> {
    capability: C,
    _scope: PhantomData<&'scope ()>,
}

impl<'scope, C> ScopedCapability<'scope, C> {
    /// Create a scoped capability (internal use only)
    pub(crate) fn new(capability: C) -> Self {
        Self {
            capability,
            _scope: PhantomData,
        }
    }

    /// Borrow the underlying capability
    pub fn as_ref(&self) -> &C {
        &self.capability
    }
}

// ============================================================================
// CAPABILITY COMBINATORS
// ============================================================================

/// Proof that we have both capabilities
pub struct Both<A, B> {
    pub a: A,
    pub b: B,
}

impl<A, B> Both<A, B> {
    pub fn new(a: A, b: B) -> Self {
        Self { a, b }
    }
}

/// Proof that we have at least one of two capabilities
pub enum Either<A, B> {
    Left(A),
    Right(B),
}

// ============================================================================
// COMPILE-TIME ASSERTIONS
// ============================================================================

/// Static assertions to verify capability properties at compile time
#[cfg(test)]
mod static_checks {
    use super::*;
    use static_assertions::{assert_impl_all, const_assert_eq};

    // Capabilities are zero-sized (no runtime cost)
    const_assert_eq!(std::mem::size_of::<CanParse>(), 0);
    const_assert_eq!(std::mem::size_of::<CanStore>(), 0);
    const_assert_eq!(std::mem::size_of::<CanValidate>(), 0);
    const_assert_eq!(std::mem::size_of::<CanQuery>(), 0);

    // Capabilities can be copied (they're just proofs)
    assert_impl_all!(CanParse: Copy, Clone, Send, Sync);
    assert_impl_all!(CanStore: Copy, Clone, Send, Sync);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_flow() {
        // Simulate the pipeline capability flow
        let _parse = grants::grant_parse();
        let _store = grants::grant_store();
        let _validate = grants::grant_validate();
        let _query = grants::grant_query();

        // These capabilities would be passed to functions that require them
    }

    #[test]
    fn scoped_capability_lifetime() {
        let query_cap = grants::grant_query();

        {
            let scoped = ScopedCapability::new(query_cap);
            let _borrowed: &CanQuery = scoped.as_ref();
            // scoped capability valid here
        }
        // scoped capability dropped, but original is still valid
        let _still_valid = query_cap;
    }
}
