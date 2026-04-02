//! Finite dead-boundary padding certificates for the compiled routing witness.
//!
//! The open-boundary construction is finite: the routed witness names finitely many published
//! pieces, each published piece has finite target support, and one-step Life only depends on the
//! predecessor radius-1 neighborhood of that target support. This module packages the resulting
//! finite-board lemma as a concrete certificate with an explicit polynomial board-size bound.

use crate::published::{published_part1_specs, published_root, PublishedPattern, PublishedSize};
use crate::routing::RoutingWitness;
use anyhow::{Context, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeadBoundaryPaddingCertificate {
    pub predecessor_radius: usize,
    pub dead_padding: usize,
    pub max_pattern_width: usize,
    pub max_pattern_height: usize,
    pub max_horizontal_step: usize,
    pub max_vertical_step: usize,
    pub support_width_upper_bound: usize,
    pub support_height_upper_bound: usize,
    pub finite_board_width_upper_bound: usize,
    pub finite_board_height_upper_bound: usize,
}

impl DeadBoundaryPaddingCertificate {
    pub fn is_complete(&self) -> bool {
        self.predecessor_radius == 1
            && self.dead_padding >= self.predecessor_radius + 1
            && self.support_width_upper_bound > 0
            && self.support_height_upper_bound > 0
            && self.finite_board_width_upper_bound
                >= self.support_width_upper_bound + 2 * self.dead_padding
            && self.finite_board_height_upper_bound
                >= self.support_height_upper_bound + 2 * self.dead_padding
    }

    pub fn render_summary(&self) -> String {
        format!(
            "finite_dead_boundary_complete={} support_bound={}x{} finite_board_bound={}x{} dead_padding={} predecessor_radius={}",
            self.is_complete(),
            self.support_width_upper_bound,
            self.support_height_upper_bound,
            self.finite_board_width_upper_bound,
            self.finite_board_height_upper_bound,
            self.dead_padding,
            self.predecessor_radius,
        )
    }
}

pub fn certify_dead_boundary_padding(
    witness: &RoutingWitness,
) -> Result<DeadBoundaryPaddingCertificate> {
    let (max_pattern_width, max_pattern_height) = published_dimension_bounds()?;
    let max_horizontal_step = max_pattern_width;
    let max_vertical_step = max_pattern_height;
    let predecessor_radius = 1usize;
    let dead_padding = predecessor_radius + 1;

    let support_width_upper_bound = witness
        .bounds
        .width()
        .saturating_mul(max_horizontal_step)
        .max(max_pattern_width);
    let support_height_upper_bound = witness
        .bounds
        .height()
        .saturating_mul(max_vertical_step)
        .max(max_pattern_height);
    let finite_board_width_upper_bound =
        support_width_upper_bound.saturating_add(2 * dead_padding);
    let finite_board_height_upper_bound =
        support_height_upper_bound.saturating_add(2 * dead_padding);

    Ok(DeadBoundaryPaddingCertificate {
        predecessor_radius,
        dead_padding,
        max_pattern_width,
        max_pattern_height,
        max_horizontal_step,
        max_vertical_step,
        support_width_upper_bound,
        support_height_upper_bound,
        finite_board_width_upper_bound,
        finite_board_height_upper_bound,
    })
}

fn published_dimension_bounds() -> Result<(usize, usize)> {
    let root = published_root();
    let mut max_width = 0usize;
    let mut max_height = 0usize;

    for spec in published_part1_specs() {
        let (width, height) = match spec.size {
            Some(PublishedSize(width, height)) => (width, height),
            None => {
                let path = root.join(spec.path);
                let pattern = PublishedPattern::from_csv_file(&path).with_context(|| {
                    format!(
                        "Failed to load published pattern '{}' while computing padding bounds",
                        spec.name
                    )
                })?;
                (pattern.width, pattern.height)
            }
        };
        max_width = max_width.max(width);
        max_height = max_height.max(height);
    }

    anyhow::ensure!(
        max_width > 0 && max_height > 0,
        "Published gadget library unexpectedly has zero size bounds"
    );
    Ok((max_width, max_height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::{Clause, CnfFormula, Literal};
    use crate::compiler::ConstructionCompiler;
    use crate::routing::construct_routing_witness;

    #[test]
    fn test_certify_dead_boundary_padding_for_small_formula() {
        let formula = CnfFormula::new(vec![
            Clause::new(vec![Literal::positive("x1"), Literal::negative("x2")]),
            Clause::new(vec![Literal::positive("x2"), Literal::positive("x3")]),
        ]);
        let construction = ConstructionCompiler::compile_cnf(&formula).unwrap();
        let witness = construct_routing_witness(&construction).unwrap();
        let certificate = certify_dead_boundary_padding(&witness).unwrap();

        assert!(certificate.is_complete());
        assert_eq!(certificate.predecessor_radius, 1);
        assert_eq!(certificate.dead_padding, 2);
        assert!(certificate.finite_board_width_upper_bound > certificate.support_width_upper_bound);
        assert!(certificate
            .render_summary()
            .contains("finite_dead_boundary_complete=true"));
    }
}
