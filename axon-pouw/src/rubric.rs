//! Pluggable rubrics: turn a workflow output into a 0..=1 score.
//!
//! Three v0 rubrics:
//!   - [`ExactMatchRubric`] — output must exactly equal a reference. Score 1 or 0.
//!   - More complex rubrics (judge-LLM, structural similarity) plug in via the
//!     [`Rubric`] trait.

use serde::{Deserialize, Serialize};

/// A scored evaluation produced by a [`Rubric`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RubricEval {
    pub score: f32,
}

impl RubricEval {
    pub fn pass() -> Self {
        Self { score: 1.0 }
    }
    pub fn fail() -> Self {
        Self { score: 0.0 }
    }
}

pub trait Rubric {
    fn evaluate(&self, output: &[u8]) -> RubricEval;
}

/// Trivial rubric: 1.0 if `output == reference`, else 0.0.
pub struct ExactMatchRubric {
    pub reference: Vec<u8>,
}

impl ExactMatchRubric {
    pub fn new(reference: impl Into<Vec<u8>>) -> Self {
        Self {
            reference: reference.into(),
        }
    }
}

impl Rubric for ExactMatchRubric {
    fn evaluate(&self, output: &[u8]) -> RubricEval {
        if output == self.reference {
            RubricEval::pass()
        } else {
            RubricEval::fail()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_and_fail_constants() {
        assert_eq!(RubricEval::pass().score, 1.0);
        assert_eq!(RubricEval::fail().score, 0.0);
    }

    #[test]
    fn exact_match_passes_when_equal() {
        let r = ExactMatchRubric::new(b"hello".as_slice());
        assert_eq!(r.evaluate(b"hello"), RubricEval::pass());
    }

    #[test]
    fn exact_match_fails_when_different() {
        let r = ExactMatchRubric::new(b"hello".as_slice());
        assert_eq!(r.evaluate(b"goodbye"), RubricEval::fail());
    }

    #[test]
    fn exact_match_fails_on_length_mismatch() {
        let r = ExactMatchRubric::new(b"hello".as_slice());
        assert_eq!(r.evaluate(b"hell"), RubricEval::fail());
    }

    #[test]
    fn rubric_eval_serde_round_trip() {
        let e = RubricEval { score: 0.42 };
        let json = serde_json::to_string(&e).unwrap();
        let back: RubricEval = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }
}
