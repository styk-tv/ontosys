//! # Type-State Pattern Demonstration
//!
//! This example demonstrates the various type-level safety patterns
//! used in the graph-transformer library.
//!
//! Run with: `cargo run --example type_state_demo`

use std::marker::PhantomData;

// ============================================================================
// PATTERN 1: TYPE-STATE MACHINES
// ============================================================================

/// States are zero-sized types (no runtime cost)
mod connection {
    use super::*;

    pub struct Disconnected;
    pub struct Connected;
    pub struct Authenticated;

    pub struct Connection<State> {
        host: String,
        _state: PhantomData<State>,
    }

    impl Connection<Disconnected> {
        pub fn new(host: &str) -> Self {
            println!("📡 Created connection to {}", host);
            Self {
                host: host.to_string(),
                _state: PhantomData,
            }
        }

        /// Connect: Disconnected → Connected
        pub fn connect(self) -> Connection<Connected> {
            println!("🔗 Connected to {}", self.host);
            Connection {
                host: self.host,
                _state: PhantomData,
            }
        }
    }

    impl Connection<Connected> {
        /// Authenticate: Connected → Authenticated
        pub fn authenticate(self, _token: &str) -> Connection<Authenticated> {
            println!("🔐 Authenticated");
            Connection {
                host: self.host,
                _state: PhantomData,
            }
        }

        /// Disconnect: Connected → Disconnected
        pub fn disconnect(self) -> Connection<Disconnected> {
            println!("👋 Disconnected");
            Connection {
                host: self.host,
                _state: PhantomData,
            }
        }
    }

    impl Connection<Authenticated> {
        /// Only available when authenticated
        pub fn fetch_data(&self) -> String {
            println!("📦 Fetching data from {}", self.host);
            "data".to_string()
        }

        /// Logout: Authenticated → Connected
        pub fn logout(self) -> Connection<Connected> {
            println!("🚪 Logged out");
            Connection {
                host: self.host,
                _state: PhantomData,
            }
        }
    }
}

// ============================================================================
// PATTERN 2: CAPABILITY TOKENS
// ============================================================================

mod capabilities {
    /// Private unit field makes this unforgeable
    pub struct CanRead(pub(super) ());
    pub struct CanWrite(pub(super) ());
    pub struct CanDelete(pub(super) ());

    /// Only this module can create capabilities
    pub fn grant_read() -> CanRead {
        CanRead(())
    }

    pub fn grant_write() -> CanWrite {
        CanWrite(())
    }

    pub fn grant_delete() -> CanDelete {
        CanDelete(())
    }
}

/// Functions require capability tokens as proof
fn read_file(_proof: &capabilities::CanRead, path: &str) -> String {
    println!("📖 Reading {} (with capability proof)", path);
    "file contents".to_string()
}

fn write_file(_proof: &capabilities::CanWrite, path: &str, data: &str) {
    println!("✏️  Writing to {} (with capability proof): {}", path, data);
}

fn delete_file(_proof: &capabilities::CanDelete, path: &str) {
    println!("🗑️  Deleting {} (with capability proof)", path);
}

// ============================================================================
// PATTERN 3: SEALED TRAITS
// ============================================================================

mod sealed {
    mod private {
        pub trait Sealed {}
    }

    /// Only types in this module can implement Entity
    pub trait Entity: private::Sealed {
        fn entity_type() -> &'static str;
    }

    pub struct User {
        pub name: String,
    }

    pub struct Document {
        pub title: String,
    }

    // Explicitly seal allowed types
    impl private::Sealed for User {}
    impl private::Sealed for Document {}

    impl Entity for User {
        fn entity_type() -> &'static str {
            "User"
        }
    }

    impl Entity for Document {
        fn entity_type() -> &'static str {
            "Document"
        }
    }

    // External code cannot implement Entity:
    // struct External;
    // impl private::Sealed for External {} // ❌ private::Sealed is private
    // impl Entity for External {} // ❌ Won't work
}

// ============================================================================
// PATTERN 4: TYPE-LEVEL FEATURE FLAGS
// ============================================================================

mod features {
    use super::*;

    /// Marker trait for features
    pub trait Feature {}

    pub struct Logging;
    pub struct Metrics;
    pub struct Caching;

    impl Feature for Logging {}
    impl Feature for Metrics {}
    impl Feature for Caching {}

    /// Type-level list (like frunk::HList)
    pub struct HNil;
    pub struct HCons<H, T>(PhantomData<(H, T)>);

    /// Position of a feature in the list (frunk-style index), which keeps the
    /// two impls below from overlapping: `Here` = head, `There<I>` = in the tail.
    pub struct Here;
    pub struct There<I>(PhantomData<I>);

    /// Check if feature list contains a feature (the index is inferred)
    pub trait HasFeature<F: Feature, I> {}

    impl<F: Feature, T> HasFeature<F, Here> for HCons<F, T> {}
    impl<F: Feature, H, T: HasFeature<F, I>, I> HasFeature<F, There<I>> for HCons<H, T> {}

    /// A service with compile-time feature tracking
    pub struct Service<Features> {
        name: String,
        _features: PhantomData<Features>,
    }

    impl Service<HNil> {
        pub fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                _features: PhantomData,
            }
        }
    }

    impl<F> Service<F> {
        pub fn with_feature<New: Feature>(self) -> Service<HCons<New, F>> {
            Service {
                name: self.name,
                _features: PhantomData,
            }
        }
    }

    /// Only available if Logging feature is enabled
    impl<F> Service<F> {
        pub fn log<I>(&self, msg: &str)
        where
            F: HasFeature<Logging, I>,
        {
            println!("📝 [{}] {}", self.name, msg);
        }
    }

    /// Only available if Metrics feature is enabled
    impl<F> Service<F> {
        pub fn record_metric<I>(&self, name: &str, value: f64)
        where
            F: HasFeature<Metrics, I>,
        {
            println!("📊 [{}] {}={}", self.name, name, value);
        }
    }

    /// Only available if Caching feature is enabled
    impl<F> Service<F> {
        pub fn cache_get<I>(&self, key: &str) -> Option<String>
        where
            F: HasFeature<Caching, I>,
        {
            println!("🗄️  [{}] Cache get: {}", self.name, key);
            None
        }
    }
}

// ============================================================================
// PATTERN 5: SESSION TYPES (Protocol Guarantees)
// ============================================================================

mod session {
    use super::*;

    /// Protocol states
    pub struct Start;
    pub struct HeadersSent;
    pub struct BodySent;
    pub struct Done;

    /// HTTP request builder with protocol enforcement
    pub struct RequestBuilder<State> {
        method: String,
        url: String,
        headers: Vec<(String, String)>,
        body: Option<String>,
        _state: PhantomData<State>,
    }

    impl RequestBuilder<Start> {
        pub fn new(method: &str, url: &str) -> Self {
            Self {
                method: method.to_string(),
                url: url.to_string(),
                headers: vec![],
                body: None,
                _state: PhantomData,
            }
        }

        /// Must send headers before body
        pub fn header(mut self, name: &str, value: &str) -> RequestBuilder<HeadersSent> {
            self.headers.push((name.to_string(), value.to_string()));
            RequestBuilder {
                method: self.method,
                url: self.url,
                headers: self.headers,
                body: self.body,
                _state: PhantomData,
            }
        }
    }

    impl RequestBuilder<HeadersSent> {
        /// Can add more headers
        pub fn header(mut self, name: &str, value: &str) -> Self {
            self.headers.push((name.to_string(), value.to_string()));
            self
        }

        /// Or send body (transitions to BodySent)
        pub fn body(mut self, data: &str) -> RequestBuilder<BodySent> {
            self.body = Some(data.to_string());
            RequestBuilder {
                method: self.method,
                url: self.url,
                headers: self.headers,
                body: self.body,
                _state: PhantomData,
            }
        }

        /// Or finish without body
        pub fn finish(self) -> RequestBuilder<Done> {
            RequestBuilder {
                method: self.method,
                url: self.url,
                headers: self.headers,
                body: self.body,
                _state: PhantomData,
            }
        }
    }

    impl RequestBuilder<BodySent> {
        /// Can only finish after body is sent
        pub fn finish(self) -> RequestBuilder<Done> {
            RequestBuilder {
                method: self.method,
                url: self.url,
                headers: self.headers,
                body: self.body,
                _state: PhantomData,
            }
        }
    }

    impl RequestBuilder<Done> {
        /// Execute the request
        pub fn send(self) -> String {
            println!("🚀 {} {}", self.method, self.url);
            for (k, v) in &self.headers {
                println!("   {}: {}", k, v);
            }
            if let Some(body) = &self.body {
                println!("   Body: {}", body);
            }
            "Response".to_string()
        }
    }
}

// ============================================================================
// PATTERN 6: INVARIANTS (CONTRACTS)
// ============================================================================

mod invariants {
    use super::*;

    /// Marker for non-empty collections
    pub struct NonEmpty;

    /// A collection with an attached invariant
    pub struct Checked<T, I> {
        value: T,
        _invariant: PhantomData<I>,
    }

    impl<T> Checked<Vec<T>, NonEmpty> {
        /// Create only succeeds if vec is non-empty
        pub fn new(vec: Vec<T>) -> Option<Self> {
            if vec.is_empty() {
                None
            } else {
                Some(Self {
                    value: vec,
                    _invariant: PhantomData,
                })
            }
        }

        /// Safe because NonEmpty invariant guarantees this
        pub fn first(&self) -> &T {
            &self.value[0]
        }

        /// Safe because NonEmpty invariant guarantees this
        pub fn last(&self) -> &T {
            &self.value[self.value.len() - 1]
        }

        pub fn len(&self) -> usize {
            self.value.len()
        }
    }
}

// ============================================================================
// MAIN DEMONSTRATION
// ============================================================================

fn main() {
    println!("═══════════════════════════════════════════════════════════════");
    println!("         TYPE-STATE PATTERNS DEMONSTRATION");
    println!("═══════════════════════════════════════════════════════════════\n");

    // Pattern 1: Type-State Machines
    println!("━━━ Pattern 1: Type-State Machines ━━━");
    {
        use connection::*;

        let conn = Connection::new("api.example.com");
        let conn = conn.connect();
        let conn = conn.authenticate("secret-token");
        let _data = conn.fetch_data();
        let conn = conn.logout();
        let _conn = conn.disconnect();

        // These would NOT compile:
        // Connection::new("x").fetch_data();  // ❌ Can't fetch when disconnected
        // Connection::new("x").connect().fetch_data();  // ❌ Not authenticated
    }
    println!();

    // Pattern 2: Capability Tokens
    println!("━━━ Pattern 2: Capability Tokens ━━━");
    {
        // Get capabilities (in real code, these would come from
        // authenticated/authorized code paths)
        let read_cap = capabilities::grant_read();
        let write_cap = capabilities::grant_write();
        let delete_cap = capabilities::grant_delete();

        // Use capabilities as proof
        let _content = read_file(&read_cap, "config.json");
        write_file(&write_cap, "output.txt", "Hello, World!");
        delete_file(&delete_cap, "temp.txt");

        // This would NOT compile without the capability:
        // read_file(&????, "secret.txt");  // ❌ No way to forge a capability
    }
    println!();

    // Pattern 3: Sealed Traits
    println!("━━━ Pattern 3: Sealed Traits ━━━");
    {
        use sealed::*;

        fn process_entity<E: Entity>(entity: &E) {
            println!("Processing entity of type: {}", E::entity_type());
        }

        let user = User {
            name: "Alice".to_string(),
        };
        let doc = Document {
            title: "Report".to_string(),
        };

        process_entity(&user);
        process_entity(&doc);

        // External code cannot implement Entity!
    }
    println!();

    // Pattern 4: Type-Level Feature Flags
    println!("━━━ Pattern 4: Type-Level Feature Flags ━━━");
    {
        use features::*;

        // Create service with specific features
        let service = Service::new("my-service")
            .with_feature::<Logging>()
            .with_feature::<Metrics>();

        // These work because the features are enabled:
        service.log("Starting up");
        service.record_metric("requests", 42.0);

        // This would NOT compile:
        // service.cache_get("key");  // ❌ Caching feature not enabled

        // Now create one with caching:
        let cached_service = Service::new("cached-service")
            .with_feature::<Logging>()
            .with_feature::<Caching>();

        cached_service.log("Checking cache");
        cached_service.cache_get("user:123");

        // This would NOT compile:
        // cached_service.record_metric("x", 1.0);  // ❌ Metrics not enabled
    }
    println!();

    // Pattern 5: Session Types
    println!("━━━ Pattern 5: Session Types (Protocol Guarantees) ━━━");
    {
        use session::*;

        // Valid protocol sequence: Start → Headers → Body → Done
        let _response = RequestBuilder::new("POST", "https://api.example.com/data")
            .header("Content-Type", "application/json")
            .header("Authorization", "Bearer token")
            .body(r#"{"key": "value"}"#)
            .finish()
            .send();

        // Valid: Start → Headers → Done (no body)
        let _response = RequestBuilder::new("GET", "https://api.example.com/users")
            .header("Accept", "application/json")
            .finish()
            .send();

        // These would NOT compile:
        // RequestBuilder::new("POST", "/").body("x");  // ❌ Must send headers first
        // RequestBuilder::new("POST", "/").header("a", "b").send();  // ❌ Must finish first
    }
    println!();

    // Pattern 6: Invariants
    println!("━━━ Pattern 6: Invariants (Contracts) ━━━");
    {
        use invariants::*;

        let numbers = vec![1, 2, 3, 4, 5];
        if let Some(non_empty) = Checked::new(numbers) {
            // These are guaranteed to work - NonEmpty invariant proves it
            println!("First: {}", non_empty.first());
            println!("Last: {}", non_empty.last());
            println!("Length: {}", non_empty.len());
        }

        let empty: Vec<i32> = vec![];
        match Checked::new(empty) {
            Some(_) => unreachable!(),
            None => println!("Empty vector rejected at construction time"),
        }
    }
    println!();

    println!("═══════════════════════════════════════════════════════════════");
    println!("All patterns demonstrated! Invalid states are compile-time errors.");
    println!("═══════════════════════════════════════════════════════════════");
}
