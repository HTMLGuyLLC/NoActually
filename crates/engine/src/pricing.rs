//! List prices, to turn the tokens a report card counts into dollars. Dated, because prices
//! change: the app shows the date under every cost, and a newer table re-prices old tokens too.

use crate::{normalize_model, Model};
use serde::{Deserialize, Serialize};

/// When [`price`]'s table was last checked against the providers' price pages.
pub const PRICES_AS_OF: &str = "2026-10";

/// US dollars per million tokens, by kind of token.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Price {
    /// Input not read from cache.
    pub input: f64,
    /// Input written to the cache (the 5-minute rate).
    pub cache_write: f64,
    /// Input read from the cache.
    pub cache_read: f64,
    /// Output, thinking included.
    pub output: f64,
}

/// Tokens by kind, as the tools record them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// Input not read from cache.
    pub input: u64,
    /// Input written to the cache.
    pub cache_write: u64,
    /// Input read from the cache.
    pub cache_read: u64,
    /// Output, thinking included.
    pub output: u64,
}

impl Usage {
    /// Adds `other` to this.
    pub fn add(&mut self, other: &Usage) {
        self.input += other.input;
        self.cache_write += other.cache_write;
        self.cache_read += other.cache_read;
        self.output += other.output;
    }

    /// The tokens a person is shown: new input, cache writes and output. Cache reads are left
    /// out because they're huge and cheap; [`Usage::cost`] still counts them.
    pub fn shown(&self) -> u64 {
        self.input + self.cache_write + self.output
    }

    /// The tokens used since `earlier`, of a running total. A total that went down (a reset, or a
    /// forked thread) starts over, so then this is all of it.
    #[must_use]
    pub fn minus(&self, earlier: &Usage) -> Usage {
        let all = |u: &Usage| u.input + u.cache_write + u.cache_read + u.output;
        if all(self) < all(earlier) {
            return *self;
        }
        Usage {
            input: self.input.saturating_sub(earlier.input),
            cache_write: self.cache_write.saturating_sub(earlier.cache_write),
            cache_read: self.cache_read.saturating_sub(earlier.cache_read),
            output: self.output.saturating_sub(earlier.output),
        }
    }

    /// `percent` of each kind, rounded down: the share of the work a follow-up made someone redo.
    #[must_use]
    pub fn share(&self, percent: u8) -> Usage {
        let p = u64::from(percent.min(100));
        Usage {
            input: self.input * p / 100,
            cache_write: self.cache_write * p / 100,
            cache_read: self.cache_read * p / 100,
            output: self.output * p / 100,
        }
    }

    /// What these tokens cost at `price`, in US dollars.
    pub fn cost(&self, price: &Price) -> f64 {
        (self.input as f64 * price.input
            + self.cache_write as f64 * price.cache_write
            + self.cache_read as f64 * price.cache_read
            + self.output as f64 * price.output)
            / 1_000_000.0
    }
}

impl Usage {
    /// What these tokens cost on the model the tool recorded as `raw_model`, or `None` when it has no
    /// price (an unknown model, a blank id, Claude Code's "<synthetic>" placeholder).
    pub fn cost_on(&self, raw_model: &str) -> Option<f64> {
        normalize_model(raw_model).and_then(|m| price(&m)).map(|p| self.cost(&p))
    }
}

/// The list price for `model`, or `None` when it isn't in the table (then only tokens are shown).
pub fn price(model: &Model) -> Option<Price> {
    // Input, output, cache read. A 5-minute cache write is 1.25x input on every model here.
    let (input, output, cache_read) = match (model.family.as_str(), model.version.as_str()) {
        ("Claude Fable", "5.1") => (10.0, 50.0, 0.25),
        ("Claude Fable", "5") => (10.0, 50.0, 1.0),
        ("Claude Opus", "5.5") => (4.0, 20.0, 0.20),
        ("Claude Opus", "5" | "4.8" | "4.7" | "4.6" | "4.5") => (5.0, 25.0, 0.50),
        ("Claude Sonnet", "5.5" | "5") => (2.0, 10.0, 0.20),
        ("Claude Sonnet", "4.6" | "4.5" | "4") => (3.0, 15.0, 0.30),
        // The rate for prompts up to 100K tokens; longer prompts cost 5x, so this can undercount.
        ("Claude Haiku", "5.5") => (0.10, 0.50, 0.01),
        ("Claude Haiku", "4.5") => (1.0, 5.0, 0.10),
        _ => return None,
    };
    Some(Price { input, cache_write: input * 1.25, cache_read, output })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalize_model;

    fn usage() -> Usage {
        Usage { input: 1_000, cache_write: 2_000, cache_read: 100_000, output: 3_000 }
    }

    #[test]
    fn shown_tokens_leave_out_cache_reads() {
        assert_eq!(usage().shown(), 6_000);
    }

    #[test]
    fn share_takes_a_percent_of_each_kind() {
        let half = usage().share(50);
        assert_eq!(half, Usage { input: 500, cache_write: 1_000, cache_read: 50_000, output: 1_500 });
        assert_eq!(usage().share(0), Usage::default());
        assert_eq!(usage().share(200), usage(), "capped at all of it");
    }

    #[test]
    fn costs_every_kind_of_token() {
        let p = price(&normalize_model("claude-opus-5-5").unwrap()).unwrap();
        assert_eq!(p, Price { input: 4.0, cache_write: 5.0, cache_read: 0.20, output: 20.0 });
        // 1k × $4 + 2k × $5 + 100k × $0.20 + 3k × $20, per million.
        assert!((usage().cost(&p) - 0.094).abs() < 1e-9);
    }

    #[test]
    fn minus_is_the_step_from_an_earlier_total_and_starts_over_when_it_drops() {
        let a = Usage { input: 10, cache_write: 20, cache_read: 300, output: 40 };
        let b = Usage { input: 4, cache_write: 5, cache_read: 100, output: 15 };
        assert_eq!(a.minus(&b), Usage { input: 6, cache_write: 15, cache_read: 200, output: 25 });
        assert_eq!(b.minus(&a), b, "a total that went down is a fresh start");
        assert_eq!(a.minus(&Usage::default()), a);
    }

    #[test]
    fn cost_on_a_raw_model_id() {
        let u = usage();
        assert!((u.cost_on("claude-opus-5-5").unwrap() - 0.094).abs() < 1e-9);
        assert_eq!(u.cost_on("gpt-5.1"), None, "no price");
        assert_eq!(u.cost_on(""), None);
        assert_eq!(u.cost_on("<synthetic>"), None);
    }

    #[test]
    fn unknown_models_have_no_price() {
        assert!(price(&normalize_model("gpt-5.1").unwrap()).is_none());
        assert!(price(&normalize_model("claude-3-opus-20240229").unwrap()).is_none());
        assert!(price(&normalize_model("claude-haiku-4-5-20251001").unwrap()).is_some());
    }
}
