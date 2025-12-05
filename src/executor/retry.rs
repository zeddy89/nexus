// Retry execution with circuit breaker pattern
// Better than Ansible's simple retry/until - supports:
// - Multiple backoff strategies (fixed, exponential, linear)
// - Circuit breaker pattern for shared failure tracking
// - Jitter to prevent thundering herd

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use rand::Rng;

use crate::parser::ast::{CircuitBreakerConfig, DelayStrategy};

/// Global circuit breaker registry
/// Circuits are shared across tasks and hosts for coordinated failure handling
pub struct CircuitBreakerRegistry {
    circuits: RwLock<HashMap<String, Arc<RwLock<CircuitBreaker>>>>,
}

impl CircuitBreakerRegistry {
    pub fn new() -> Self {
        CircuitBreakerRegistry {
            circuits: RwLock::new(HashMap::new()),
        }
    }

    /// Get or create a circuit breaker by name
    pub fn get_or_create(&self, config: &CircuitBreakerConfig) -> Arc<RwLock<CircuitBreaker>> {
        let mut circuits = self.circuits.write();

        if let Some(circuit) = circuits.get(&config.name) {
            return circuit.clone();
        }

        let circuit = Arc::new(RwLock::new(CircuitBreaker::new(config.clone())));
        circuits.insert(config.name.clone(), circuit.clone());
        circuit
    }

    /// Get circuit status for reporting
    pub fn get_status(&self, name: &str) -> Option<CircuitState> {
        let circuits = self.circuits.read();
        circuits.get(name).map(|c| c.read().state())
    }
}

impl Default for CircuitBreakerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Circuit breaker states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Circuit is closed - normal operation
    Closed,
    /// Circuit is open - blocking requests
    Open,
    /// Circuit is half-open - testing if service recovered
    HalfOpen,
}

/// Circuit breaker implementation
pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    state: CircuitState,
    failure_count: u32,
    success_count: u32,
    last_failure_time: Option<Instant>,
}

impl CircuitBreaker {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        CircuitBreaker {
            config,
            state: CircuitState::Closed,
            failure_count: 0,
            success_count: 0,
            last_failure_time: None,
        }
    }

    /// Get current state
    pub fn state(&self) -> CircuitState {
        self.state
    }

    /// Check if request should be allowed
    pub fn should_allow(&mut self) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if reset timeout has passed
                if let Some(last_failure) = self.last_failure_time {
                    if last_failure.elapsed() >= self.config.reset_timeout {
                        // Transition to half-open
                        self.state = CircuitState::HalfOpen;
                        self.success_count = 0;
                        return true;
                    }
                }
                false
            }
            CircuitState::HalfOpen => true,
        }
    }

    /// Record a successful execution
    pub fn record_success(&mut self) {
        match self.state {
            CircuitState::Closed => {
                // Reset failure count on success
                self.failure_count = 0;
            }
            CircuitState::HalfOpen => {
                self.success_count += 1;
                if self.success_count >= self.config.success_threshold {
                    // Close the circuit
                    self.state = CircuitState::Closed;
                    self.failure_count = 0;
                    self.success_count = 0;
                }
            }
            CircuitState::Open => {
                // Should not happen
            }
        }
    }

    /// Record a failed execution
    pub fn record_failure(&mut self) {
        self.last_failure_time = Some(Instant::now());

        match self.state {
            CircuitState::Closed => {
                self.failure_count += 1;
                if self.failure_count >= self.config.failure_threshold {
                    // Open the circuit
                    self.state = CircuitState::Open;
                }
            }
            CircuitState::HalfOpen => {
                // Immediately open on failure in half-open
                self.state = CircuitState::Open;
                self.success_count = 0;
            }
            CircuitState::Open => {
                // Already open
            }
        }
    }

    /// Get remaining time until circuit might allow requests
    pub fn time_until_retry(&self) -> Option<Duration> {
        if self.state == CircuitState::Open {
            if let Some(last_failure) = self.last_failure_time {
                let elapsed = last_failure.elapsed();
                if elapsed < self.config.reset_timeout {
                    return Some(self.config.reset_timeout - elapsed);
                }
            }
        }
        None
    }
}

/// Calculate delay for a retry attempt
pub fn calculate_delay(strategy: &DelayStrategy, attempt: u32) -> Duration {
    match strategy {
        DelayStrategy::Fixed(duration) => *duration,
        DelayStrategy::Exponential { base, max, jitter } => {
            // delay = base * 2^attempt
            let multiplier = 2u64.saturating_pow(attempt);
            let delay = base.as_millis() as u64 * multiplier;
            let delay = Duration::from_millis(delay.min(max.as_millis() as u64));

            if *jitter {
                // Add 0-25% jitter
                let jitter_ms = rand::thread_rng().gen_range(0..=(delay.as_millis() as u64 / 4));
                delay + Duration::from_millis(jitter_ms)
            } else {
                delay
            }
        }
        DelayStrategy::Linear { base, increment, max } => {
            // delay = base + (increment * attempt)
            let delay_ms = base.as_millis() as u64 + (increment.as_millis() as u64 * attempt as u64);
            Duration::from_millis(delay_ms.min(max.as_millis() as u64))
        }
    }
}

/// Result of a retry operation
#[derive(Debug)]
pub enum RetryResult<T> {
    /// Operation succeeded
    Success(T),
    /// Operation failed after all retries
    Failed {
        last_error: String,
        attempts: u32,
        total_time: Duration,
    },
    /// Circuit breaker blocked the operation
    CircuitOpen {
        circuit_name: String,
        time_until_retry: Option<Duration>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circuit_breaker_opens_after_failures() {
        let config = CircuitBreakerConfig {
            name: "test".to_string(),
            failure_threshold: 3,
            reset_timeout: Duration::from_secs(60),
            success_threshold: 2,
        };

        let mut cb = CircuitBreaker::new(config);
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.should_allow());

        // Record failures
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.should_allow());
    }

    #[test]
    fn test_exponential_backoff() {
        let strategy = DelayStrategy::Exponential {
            base: Duration::from_secs(1),
            max: Duration::from_secs(60),
            jitter: false,
        };

        assert_eq!(calculate_delay(&strategy, 0), Duration::from_secs(1));
        assert_eq!(calculate_delay(&strategy, 1), Duration::from_secs(2));
        assert_eq!(calculate_delay(&strategy, 2), Duration::from_secs(4));
        assert_eq!(calculate_delay(&strategy, 3), Duration::from_secs(8));
        // Should cap at max
        assert_eq!(calculate_delay(&strategy, 10), Duration::from_secs(60));
    }

    #[test]
    fn test_linear_backoff() {
        let strategy = DelayStrategy::Linear {
            base: Duration::from_secs(5),
            increment: Duration::from_secs(10),
            max: Duration::from_secs(60),
        };

        assert_eq!(calculate_delay(&strategy, 0), Duration::from_secs(5));
        assert_eq!(calculate_delay(&strategy, 1), Duration::from_secs(15));
        assert_eq!(calculate_delay(&strategy, 2), Duration::from_secs(25));
        // Should cap at max
        assert_eq!(calculate_delay(&strategy, 10), Duration::from_secs(60));
    }

    #[test]
    fn test_circuit_breaker_registry() {
        let registry = CircuitBreakerRegistry::new();

        let config = CircuitBreakerConfig {
            name: "db".to_string(),
            failure_threshold: 5,
            reset_timeout: Duration::from_secs(30),
            success_threshold: 2,
        };

        let cb1 = registry.get_or_create(&config);
        let cb2 = registry.get_or_create(&config);

        // Should be the same circuit
        assert!(Arc::ptr_eq(&cb1, &cb2));
    }
}
