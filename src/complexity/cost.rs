//! # Cost
//!
//! Growth as `base^n · n^degree · log^log n`, composed by `mul` (nesting) and
//! `max` (sequence), and rounded **up** into the spec's closed set of classes,
//! keeping the exact form when it was rounded.

/// A growth term. `exp_base` set means exponential.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cost {
    pub exp_base: Option<u32>,
    pub degree: u32,
    pub log: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GrowthClass {
    O1,
    OLogN,
    ON,
    ONLogN,
    ON2,
    ON3,
    /// Polynomial of this degree (4 or more).
    ONK(u32),
    /// Exponential with this base.
    OExp(u32),
}

impl Cost {
    pub const ONE: Cost = Cost {
        exp_base: None,
        degree: 0,
        log: 0,
    };

    pub fn n() -> Cost {
        Cost {
            degree: 1,
            ..Cost::ONE
        }
    }

    pub fn log_n() -> Cost {
        Cost {
            log: 1,
            ..Cost::ONE
        }
    }

    pub fn exp(base: u32) -> Cost {
        Cost {
            exp_base: Some(base),
            ..Cost::ONE
        }
    }

    /// Nested work: the product.
    pub fn mul(self, o: Cost) -> Cost {
        let exp_base = match (self.exp_base, o.exp_base) {
            (Some(a), Some(b)) => Some(a.saturating_mul(b)),
            (a, b) => a.or(b),
        };
        Cost {
            exp_base,
            degree: self.degree + o.degree,
            log: self.log + o.log,
        }
    }

    /// Sequential work: the larger (exponential, then degree, then log).
    pub fn max(self, o: Cost) -> Cost {
        let key = |c: &Cost| {
            (
                c.exp_base.is_some(),
                c.exp_base.unwrap_or(0),
                c.degree,
                c.log,
            )
        };
        if key(&o) > key(&self) {
            o
        } else {
            self
        }
    }

    fn poly_form(degree: u32, log: u32) -> String {
        let n = match degree {
            0 => String::new(),
            1 => "n".to_string(),
            d => format!("n^{d}"),
        };
        let l = match log {
            0 => String::new(),
            1 => "log n".to_string(),
            k => format!("log^{k} n"),
        };
        [n, l]
            .into_iter()
            .filter(|x| !x.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The class, rounded up, and the exact form when rounding changed it.
    pub fn class(&self) -> (GrowthClass, Option<String>) {
        use GrowthClass::*;
        if let Some(b) = self.exp_base {
            if self.degree == 0 && self.log == 0 {
                return (OExp(b), None);
            }
            // b^n · poly(n) <= (b+1)^n eventually
            return (
                OExp(b + 1),
                Some(format!("{b}^n {}", Cost::poly_form(self.degree, self.log))),
            );
        }
        let poly = |d: u32| match d {
            0 => O1,
            1 => ON,
            2 => ON2,
            3 => ON3,
            d => ONK(d),
        };
        match (self.degree, self.log) {
            (d, 0) => (poly(d), None),
            (0, 1) => (OLogN, None),
            (1, 1) => (ONLogN, None),
            // log^k n <= n, n log^k n <= n^2, n^d log^k n <= n^(d+1)
            (d, l) => (poly(d + 1), Some(Cost::poly_form(d, l))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::GrowthClass::*;
    use super::*;

    fn c(degree: u32, log: u32) -> Cost {
        Cost {
            exp_base: None,
            degree,
            log,
        }
    }

    #[test]
    fn rounding_table() {
        let rows: Vec<(Cost, GrowthClass, Option<&str>)> = vec![
            (c(0, 0), O1, None),
            (c(0, 1), OLogN, None),
            (c(1, 0), ON, None),
            (c(1, 1), ONLogN, None),
            (c(2, 0), ON2, None),
            (c(3, 0), ON3, None),
            (c(4, 0), ONK(4), None),
            (c(7, 0), ONK(7), None),
            (c(0, 2), ON, Some("log^2 n")),
            (c(1, 2), ON2, Some("n log^2 n")),
            (c(2, 1), ON3, Some("n^2 log n")),
            (c(3, 2), ONK(4), Some("n^3 log^2 n")),
            (Cost::exp(2), OExp(2), None),
            (Cost::exp(2).mul(c(1, 0)), OExp(3), Some("2^n n")),
        ];
        for (cost, class, exact) in rows {
            assert_eq!(cost.class(), (class, exact.map(str::to_string)), "{cost:?}");
        }
    }

    #[test]
    fn max_and_mul() {
        assert_eq!(Cost::n().mul(Cost::n()), c(2, 0));
        assert_eq!(Cost::n().mul(Cost::log_n()), c(1, 1));
        assert_eq!(Cost::ONE.mul(Cost::log_n()).mul(Cost::log_n()), c(0, 2));
        assert_eq!(c(2, 0).max(c(1, 5)), c(2, 0), "degree before log");
        assert_eq!(c(1, 1).max(c(1, 0)), c(1, 1));
        assert_eq!(
            c(9, 0).max(Cost::exp(2)),
            Cost::exp(2),
            "exponential beats any polynomial"
        );
        assert_eq!(Cost::exp(2).max(Cost::exp(3)), Cost::exp(3));
        assert_eq!(Cost::exp(2).mul(Cost::exp(3)), Cost::exp(6));
        assert_eq!(Cost::ONE.max(Cost::ONE), Cost::ONE);
    }
}
