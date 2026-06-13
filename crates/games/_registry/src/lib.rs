//! Aggregator crate. It contains no logic — its only job is to force every game
//! crate to be linked into the final binary so their `inventory` registrations
//! are present at runtime.
//!
//! To add a game, append ONE line below (and the matching dependency in
//! Cargo.toml). The `as _` import links the crate without using any of its
//! symbols. Keep entries alphabetical and append-only for conflict-free merges.

use game_snake as _;
