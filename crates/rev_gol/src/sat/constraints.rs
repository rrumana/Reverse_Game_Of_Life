//! Constraint generation for Game of Life SAT encoding

use super::VariableManager;
use crate::config::BoundaryCondition;
use crate::game_of_life::{GameOfLifeRules, Grid};
use anyhow::Result;

/// Constraint strength levels for adaptive symmetry breaking
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConstraintStrength {
    Full,   // Maximum constraints for early time steps
    Medium, // Balanced constraints for middle time steps
    Light,  // Minimal constraints for later time steps
}

/// Types of symmetry breaking constraints
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SymmetryType {
    Lexicographic, // Lexicographic ordering constraints
    Rotational,    // Rotational symmetry breaking
    Reflectional,  // Reflection symmetry breaking
    Translational, // Translation symmetry breaking
}

/// Represents a SAT clause (disjunction of literals)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clause {
    pub literals: Vec<i32>, // Positive for variable, negative for negation
}

impl Clause {
    /// Create a new clause from literals
    pub fn new(literals: Vec<i32>) -> Self {
        Self { literals }
    }

    /// Create a unit clause (single literal)
    pub fn unit(literal: i32) -> Self {
        Self {
            literals: vec![literal],
        }
    }

    /// Create a binary clause (two literals)
    pub fn binary(lit1: i32, lit2: i32) -> Self {
        Self {
            literals: vec![lit1, lit2],
        }
    }

    /// Check if clause is empty (unsatisfiable)
    pub fn is_empty(&self) -> bool {
        self.literals.is_empty()
    }

    /// Check if clause is unit
    pub fn is_unit(&self) -> bool {
        self.literals.len() == 1
    }
}

/// Generates SAT constraints for the reverse Game of Life problem
pub struct ConstraintGenerator {
    variable_manager: VariableManager,
    width: usize,
    height: usize,
    time_steps: usize,
    boundary_condition: BoundaryCondition,
    symmetry_breaking: bool,
}

impl ConstraintGenerator {
    /// Create a new constraint generator
    pub fn new(
        width: usize,
        height: usize,
        time_steps: usize,
        boundary_condition: BoundaryCondition,
        symmetry_breaking: bool,
    ) -> Self {
        let variable_manager = VariableManager::new(width, height, time_steps, false);

        Self {
            variable_manager,
            width,
            height,
            time_steps,
            boundary_condition,
            symmetry_breaking,
        }
    }

    /// Generate all constraints for the reverse Game of Life problem
    pub fn generate_all_constraints(&mut self, target_grid: &Grid) -> Result<Vec<Clause>> {
        let mut clauses = Vec::new();

        // 1. Target state constraints (final time step must match target)
        clauses.extend(self.generate_target_constraints(target_grid)?);

        // 2. Game of Life transition constraints for each time step
        for t in 0..self.time_steps - 1 {
            clauses.extend(self.generate_transition_constraints(t)?);
        }

        // 3. Symmetry breaking constraints (if enabled)
        if self.symmetry_breaking {
            clauses.extend(self.generate_symmetry_breaking_constraints()?);
        }

        Ok(clauses)
    }

    /// Generate constraints that fix the final state to match the target
    fn generate_target_constraints(&mut self, target_grid: &Grid) -> Result<Vec<Clause>> {
        let mut clauses = Vec::new();
        let final_time = self.time_steps - 1;

        if target_grid.width != self.width || target_grid.height != self.height {
            anyhow::bail!(
                "Target grid dimensions ({}, {}) don't match problem dimensions ({}, {})",
                target_grid.width,
                target_grid.height,
                self.width,
                self.height
            );
        }

        for y in 0..self.height {
            for x in 0..self.width {
                let cell_var = self.variable_manager.cell_variable(x, y, final_time)?;
                let target_alive = target_grid.get(y, x);

                if target_alive {
                    // Cell must be alive
                    clauses.push(Clause::unit(cell_var));
                } else {
                    // Cell must be dead
                    clauses.push(Clause::unit(-cell_var));
                }
            }
        }

        Ok(clauses)
    }

    /// Generate Game of Life transition constraints between time steps
    fn generate_transition_constraints(&mut self, t: usize) -> Result<Vec<Clause>> {
        let mut clauses = Vec::new();

        for y in 0..self.height {
            for x in 0..self.width {
                clauses.extend(self.generate_cell_transition_constraints(x, y, t)?);
            }
        }

        Ok(clauses)
    }

    /// Generate transition constraints for a specific cell
    fn generate_cell_transition_constraints(
        &mut self,
        x: usize,
        y: usize,
        t: usize,
    ) -> Result<Vec<Clause>> {
        let mut clauses = Vec::new();

        let current_cell = self.variable_manager.cell_variable(x, y, t)?;
        let next_cell = self.variable_manager.cell_variable(x, y, t + 1)?;

        clauses.extend(self.generate_direct_transition_constraints(
            x,
            y,
            t,
            current_cell,
            next_cell,
        )?);

        Ok(clauses)
    }

    /// Generate transition constraints without auxiliary variables (direct encoding)
    fn generate_direct_transition_constraints(
        &mut self,
        x: usize,
        y: usize,
        t: usize,
        current_cell: i32,
        next_cell: i32,
    ) -> Result<Vec<Clause>> {
        let mut clauses = Vec::new();

        // Get neighbor variables
        let neighbor_vars = self.get_neighbor_variables(x, y, t)?;

        // Generate constraints for each possible neighbor count
        for k in 0..=8 {
            // Generate all combinations of k neighbors being alive
            let neighbor_combinations = self.generate_neighbor_combinations(&neighbor_vars, k);

            for combination in neighbor_combinations {
                if GameOfLifeRules::should_be_alive(true, k) {
                    // If current cell is alive and exactly k neighbors are alive, next cell should be alive
                    let mut clause = vec![-current_cell, next_cell];
                    clause.extend(
                        combination
                            .iter()
                            .map(|&(var, alive)| if alive { -var } else { var }),
                    );
                    clauses.push(Clause::new(clause));
                } else {
                    // If current cell is alive and exactly k neighbors are alive, next cell should be dead
                    let mut clause = vec![-current_cell, -next_cell];
                    clause.extend(
                        combination
                            .iter()
                            .map(|&(var, alive)| if alive { -var } else { var }),
                    );
                    clauses.push(Clause::new(clause));
                }

                if GameOfLifeRules::should_be_alive(false, k) {
                    // If current cell is dead and exactly k neighbors are alive, next cell should be alive
                    let mut clause = vec![current_cell, next_cell];
                    clause.extend(
                        combination
                            .iter()
                            .map(|&(var, alive)| if alive { -var } else { var }),
                    );
                    clauses.push(Clause::new(clause));
                } else {
                    // If current cell is dead and exactly k neighbors are alive, next cell should be dead
                    let mut clause = vec![current_cell, -next_cell];
                    clause.extend(
                        combination
                            .iter()
                            .map(|&(var, alive)| if alive { -var } else { var }),
                    );
                    clauses.push(Clause::new(clause));
                }
            }
        }

        Ok(clauses)
    }

    /// Get neighbor variables for a cell, handling boundary conditions
    fn get_neighbor_variables(&mut self, x: usize, y: usize, t: usize) -> Result<Vec<i32>> {
        let mut neighbors = Vec::new();

        for dy in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue; // Skip the cell itself
                }

                let nx = x as isize + dx;
                let ny = y as isize + dy;

                if let Some(neighbor_var) = self.get_neighbor_variable_with_boundary(nx, ny, t)? {
                    neighbors.push(neighbor_var);
                }
            }
        }

        Ok(neighbors)
    }

    /// Get neighbor variable handling boundary conditions
    fn get_neighbor_variable_with_boundary(
        &mut self,
        x: isize,
        y: isize,
        t: usize,
    ) -> Result<Option<i32>> {
        match self.boundary_condition {
            BoundaryCondition::Dead => {
                if x >= 0 && x < self.width as isize && y >= 0 && y < self.height as isize {
                    Ok(Some(
                        self.variable_manager
                            .cell_variable(x as usize, y as usize, t)?,
                    ))
                } else {
                    Ok(None) // Out of bounds cells are always dead (no variable needed)
                }
            }
            BoundaryCondition::Wrap => {
                let wrapped_x = ((x % self.width as isize + self.width as isize)
                    % self.width as isize) as usize;
                let wrapped_y = ((y % self.height as isize + self.height as isize)
                    % self.height as isize) as usize;
                Ok(Some(
                    self.variable_manager
                        .cell_variable(wrapped_x, wrapped_y, t)?,
                ))
            }
            BoundaryCondition::Mirror => {
                let mirrored_x = if x < 0 {
                    (-x - 1) as usize
                } else if x >= self.width as isize {
                    self.width - 1 - (x - self.width as isize) as usize
                } else {
                    x as usize
                };

                let mirrored_y = if y < 0 {
                    (-y - 1) as usize
                } else if y >= self.height as isize {
                    self.height - 1 - (y - self.height as isize) as usize
                } else {
                    y as usize
                };

                if mirrored_x < self.width && mirrored_y < self.height {
                    Ok(Some(
                        self.variable_manager
                            .cell_variable(mirrored_x, mirrored_y, t)?,
                    ))
                } else {
                    Ok(None)
                }
            }
        }
    }

    /// Generate all combinations of exactly k neighbors being alive
    fn generate_neighbor_combinations(
        &self,
        neighbor_vars: &[i32],
        k: u8,
    ) -> Vec<Vec<(i32, bool)>> {
        let n = neighbor_vars.len();
        let k = k as usize;
        let mut combinations = Vec::new();

        if k > n {
            return combinations; // Impossible to have more alive than total neighbors
        }

        // Generate all combinations of choosing k positions to be alive
        let mut alive_indices = Vec::new();
        self.generate_k_combinations(
            n,
            k,
            0,
            &mut alive_indices,
            &mut combinations,
            neighbor_vars,
        );

        combinations
    }

    /// Generate combinations of exactly k alive neighbors using iterative approach
    fn generate_k_combinations(
        &self,
        n: usize,
        k: usize,
        start: usize,
        current_alive: &mut Vec<usize>,
        result: &mut Vec<Vec<(i32, bool)>>,
        neighbor_vars: &[i32],
    ) {
        if current_alive.len() == k {
            // Create a combination with exactly k alive neighbors
            let mut combination = Vec::new();
            for (i, &var) in neighbor_vars.iter().enumerate() {
                let is_alive = current_alive.contains(&i);
                combination.push((var, is_alive));
            }
            result.push(combination);
            return;
        }

        if start >= n || current_alive.len() + (n - start) < k {
            return; // Not enough remaining positions to reach k
        }

        // Include current position as alive
        current_alive.push(start);
        self.generate_k_combinations(n, k, start + 1, current_alive, result, neighbor_vars);
        current_alive.pop();

        // Skip current position (keep it dead)
        self.generate_k_combinations(n, k, start + 1, current_alive, result, neighbor_vars);
    }

    /// Generate symmetry breaking constraints for maximum speedup
    fn generate_symmetry_breaking_constraints(&mut self) -> Result<Vec<Clause>> {
        let mut clauses = Vec::new();

        for t in 0..self.time_steps {
            let constraint_strength = self.calculate_constraint_strength(t);

            match constraint_strength {
                ConstraintStrength::Full => {
                    clauses.extend(self.generate_all_symmetry_constraints(t)?);
                }
                ConstraintStrength::Medium => {
                    clauses.extend(self.generate_lexicographic_constraints(t)?);
                    clauses.extend(self.generate_rotational_constraints(t)?);
                }
                ConstraintStrength::Light => {
                    clauses.extend(self.generate_lexicographic_constraints(t)?);
                }
            }
        }

        Ok(clauses)
    }

    /// Calculate constraint strength based on time step for optimal performance
    fn calculate_constraint_strength(&self, t: usize) -> ConstraintStrength {
        match t {
            0 => ConstraintStrength::Full,       // Maximum early pruning
            1..=2 => ConstraintStrength::Medium, // Balanced approach
            _ => ConstraintStrength::Light,      // Minimal overhead
        }
    }

    /// Generate all symmetry constraints for maximum breaking
    fn generate_all_symmetry_constraints(&mut self, t: usize) -> Result<Vec<Clause>> {
        let mut clauses = Vec::new();

        if self.should_apply_symmetry_type(SymmetryType::Lexicographic) {
            clauses.extend(self.generate_lexicographic_constraints(t)?);
        }
        if self.should_apply_symmetry_type(SymmetryType::Rotational) {
            clauses.extend(self.generate_rotational_constraints(t)?);
        }
        if self.should_apply_symmetry_type(SymmetryType::Reflectional) {
            clauses.extend(self.generate_reflectional_constraints(t)?);
        }
        if self.should_apply_symmetry_type(SymmetryType::Translational) {
            clauses.extend(self.generate_translational_constraints(t)?);
        }

        Ok(clauses)
    }

    /// Check if a symmetry type should be applied based on grid size
    fn should_apply_symmetry_type(&self, symmetry_type: SymmetryType) -> bool {
        let grid_size = self.width * self.height;

        match (symmetry_type, grid_size) {
            (SymmetryType::Lexicographic, _) => true, // Always beneficial
            (SymmetryType::Rotational, size) if size <= 100 => true,
            (SymmetryType::Reflectional, size) if size <= 225 => true,
            (SymmetryType::Translational, size) if size <= 64 => true,
            _ => false, // Skip for large grids to avoid constraint explosion
        }
    }

    /// Generate lexicographic ordering constraints for maximum early pruning
    fn generate_lexicographic_constraints(&mut self, t: usize) -> Result<Vec<Clause>> {
        let mut clauses = Vec::new();

        // Only apply to initial time step for maximum early pruning with minimal risk
        if t == 0 {
            // Aggressive dominance-based symmetry breaking
            clauses.extend(self.generate_minimal_rotation_breaking(t)?);
        }

        Ok(clauses)
    }

    /// Generate dominance-based symmetry breaking constraints
    /// Uses the most effective techniques to eliminate symmetric search branches
    fn generate_minimal_rotation_breaking(&mut self, t: usize) -> Result<Vec<Clause>> {
        let mut clauses = Vec::new();

        // Strategy: Use "first living cell" dominance constraint
        // This is extremely effective for sparse patterns common in GoL reverse problems

        // For 180° rotation symmetry: compare first few positions with their rotated counterparts
        let total_cells = self.width * self.height;
        let constraint_positions = std::cmp::min(3, total_cells / 2);

        for i in 0..constraint_positions {
            let y1 = i / self.width;
            let x1 = i % self.width;

            // Calculate 180° rotated position
            let x2 = self.width - 1 - x1;
            let y2 = self.height - 1 - y1;
            let pos2 = y2 * self.width + x2;

            // Skip if same position (center)
            if i == pos2 {
                continue;
            }

            // Only add constraint if this is the lexicographically smaller position
            if i < pos2 {
                let var1 = self.variable_manager.cell_variable(x1, y1, t)?;
                let var2 = self.variable_manager.cell_variable(x2, y2, t)?;

                // Dominance constraint: var2 → var1 (if rotated position is alive, original must be)
                clauses.push(Clause::binary(-var2, var1));
            }
        }

        // Additional constraint: "first living cell" must be in canonical position
        // This is very effective for sparse patterns
        if self.width >= 3 && self.height >= 3 {
            // If any cell in the bottom-right quadrant is alive,
            // then at least one cell in the top-left quadrant must be alive
            let mid_x = self.width / 2;
            let mid_y = self.height / 2;

            // Sample key positions from bottom-right quadrant
            let mut br_vars = Vec::new();
            if mid_x < self.width - 1 && mid_y < self.height - 1 {
                br_vars.push(self.variable_manager.cell_variable(
                    self.width - 1,
                    self.height - 1,
                    t,
                )?);
                if mid_x + 1 < self.width - 1 {
                    br_vars.push(self.variable_manager.cell_variable(
                        self.width - 2,
                        self.height - 1,
                        t,
                    )?);
                }
                if mid_y + 1 < self.height - 1 {
                    br_vars.push(self.variable_manager.cell_variable(
                        self.width - 1,
                        self.height - 2,
                        t,
                    )?);
                }
            }

            // Sample key positions from top-left quadrant
            let mut tl_vars = Vec::new();
            tl_vars.push(self.variable_manager.cell_variable(0, 0, t)?);
            if mid_x > 0 {
                tl_vars.push(self.variable_manager.cell_variable(1, 0, t)?);
            }
            if mid_y > 0 {
                tl_vars.push(self.variable_manager.cell_variable(0, 1, t)?);
            }

            // Create constraints: if any bottom-right cell is alive, at least one top-left cell must be alive
            for &br_var in &br_vars {
                for &tl_var in &tl_vars {
                    // br_var → tl_var
                    clauses.push(Clause::binary(-br_var, tl_var));
                }
            }
        }

        Ok(clauses)
    }

    /// Generate horizontal reflection lexicographic constraints
    #[allow(dead_code)]
    fn generate_horizontal_reflection_lex_constraints(&mut self, t: usize) -> Result<Vec<Clause>> {
        let mut clauses = Vec::new();

        for y in 0..self.height {
            for x in 0..self.width {
                let orig_var = self.variable_manager.cell_variable(x, y, t)?;
                let refl_var = self
                    .variable_manager
                    .cell_variable(x, self.height - 1 - y, t)?;

                // Constraint: orig_var >= refl_var
                clauses.push(Clause::binary(-refl_var, orig_var));
            }
        }

        Ok(clauses)
    }

    /// Generate vertical reflection lexicographic constraints
    #[allow(dead_code)]
    fn generate_vertical_reflection_lex_constraints(&mut self, t: usize) -> Result<Vec<Clause>> {
        let mut clauses = Vec::new();

        for y in 0..self.height {
            for x in 0..self.width {
                let orig_var = self.variable_manager.cell_variable(x, y, t)?;
                let refl_var = self
                    .variable_manager
                    .cell_variable(self.width - 1 - x, y, t)?;

                // Constraint: orig_var >= refl_var
                clauses.push(Clause::binary(-refl_var, orig_var));
            }
        }

        Ok(clauses)
    }

    /// Generate rotational symmetry breaking constraints
    fn generate_rotational_constraints(&mut self, t: usize) -> Result<Vec<Clause>> {
        let mut clauses = Vec::new();

        if self.width == self.height {
            // Square grid: break 90° and 270° rotations
            clauses.extend(self.generate_90_rotation_constraints(t)?);
            clauses.extend(self.generate_270_rotation_constraints(t)?);
        }

        // Note: 180° rotation is handled by lexicographic constraints

        Ok(clauses)
    }

    /// Generate 90° rotation constraints for square grids
    fn generate_90_rotation_constraints(&mut self, t: usize) -> Result<Vec<Clause>> {
        let mut clauses = Vec::new();

        // For square grids only
        if self.width != self.height {
            return Ok(clauses);
        }

        let n = self.width;

        for y in 0..n {
            for x in 0..n {
                let orig_var = self.variable_manager.cell_variable(x, y, t)?;
                // 90° clockwise rotation: (x,y) -> (y, n-1-x)
                let rot_var = self.variable_manager.cell_variable(y, n - 1 - x, t)?;

                // Constraint: orig_var >= rot_var (lexicographic ordering)
                clauses.push(Clause::binary(-rot_var, orig_var));
            }
        }

        Ok(clauses)
    }

    /// Generate 270° rotation constraints for square grids
    fn generate_270_rotation_constraints(&mut self, t: usize) -> Result<Vec<Clause>> {
        let mut clauses = Vec::new();

        // For square grids only
        if self.width != self.height {
            return Ok(clauses);
        }

        let n = self.width;

        for y in 0..n {
            for x in 0..n {
                let orig_var = self.variable_manager.cell_variable(x, y, t)?;
                // 270° clockwise rotation: (x,y) -> (n-1-y, x)
                let rot_var = self.variable_manager.cell_variable(n - 1 - y, x, t)?;

                // Constraint: orig_var >= rot_var (lexicographic ordering)
                clauses.push(Clause::binary(-rot_var, orig_var));
            }
        }

        Ok(clauses)
    }

    /// Generate reflection symmetry breaking constraints
    fn generate_reflectional_constraints(&mut self, t: usize) -> Result<Vec<Clause>> {
        let mut clauses = Vec::new();

        // Diagonal reflections (for square grids)
        if self.width == self.height {
            clauses.extend(self.generate_diagonal_reflection_constraints(t)?);
            clauses.extend(self.generate_anti_diagonal_reflection_constraints(t)?);
        }

        // Note: Horizontal and vertical reflections are handled by lexicographic constraints

        Ok(clauses)
    }

    /// Generate diagonal reflection constraints (main diagonal)
    fn generate_diagonal_reflection_constraints(&mut self, t: usize) -> Result<Vec<Clause>> {
        let mut clauses = Vec::new();

        // For square grids only
        if self.width != self.height {
            return Ok(clauses);
        }

        let n = self.width;

        for y in 0..n {
            for x in 0..n {
                let orig_var = self.variable_manager.cell_variable(x, y, t)?;
                // Diagonal reflection: (x,y) -> (y, x)
                let refl_var = self.variable_manager.cell_variable(y, x, t)?;

                // Constraint: orig_var >= refl_var
                clauses.push(Clause::binary(-refl_var, orig_var));
            }
        }

        Ok(clauses)
    }

    /// Generate anti-diagonal reflection constraints
    fn generate_anti_diagonal_reflection_constraints(&mut self, t: usize) -> Result<Vec<Clause>> {
        let mut clauses = Vec::new();

        // For square grids only
        if self.width != self.height {
            return Ok(clauses);
        }

        let n = self.width;

        for y in 0..n {
            for x in 0..n {
                let orig_var = self.variable_manager.cell_variable(x, y, t)?;
                // Anti-diagonal reflection: (x,y) -> (n-1-y, n-1-x)
                let refl_var = self
                    .variable_manager
                    .cell_variable(n - 1 - y, n - 1 - x, t)?;

                // Constraint: orig_var >= refl_var
                clauses.push(Clause::binary(-refl_var, orig_var));
            }
        }

        Ok(clauses)
    }

    /// Generate translational symmetry breaking constraints
    fn generate_translational_constraints(&mut self, t: usize) -> Result<Vec<Clause>> {
        let mut clauses = Vec::new();

        // Strategy 1: Corner anchoring (if any cell alive, specific corner must be alive)
        clauses.extend(self.generate_corner_anchoring_constraints(t)?);

        Ok(clauses)
    }

    /// Generate corner anchoring constraints to eliminate translations
    fn generate_corner_anchoring_constraints(&mut self, t: usize) -> Result<Vec<Clause>> {
        let mut clauses = Vec::new();

        // If any cell is alive, then top-left corner must be alive
        let corner_var = self.variable_manager.cell_variable(0, 0, t)?;

        for y in 0..self.height {
            for x in 0..self.width {
                if x == 0 && y == 0 {
                    continue;
                }

                let cell_var = self.variable_manager.cell_variable(x, y, t)?;
                // Constraint: cell_var -> corner_var
                // In CNF: ¬cell_var ∨ corner_var
                clauses.push(Clause::binary(-cell_var, corner_var));
            }
        }

        Ok(clauses)
    }

    /// Get the variable manager (for external access)
    pub fn variable_manager(&mut self) -> &mut VariableManager {
        &mut self.variable_manager
    }

    /// Count the exact number of target-state clauses in the current encoding.
    pub fn target_clause_count(&self) -> usize {
        self.width * self.height
    }

    /// Count the exact number of transition clauses in the current encoding.
    pub fn transition_clause_count(&self) -> usize {
        let mut total = 0;

        for _t in 0..self.time_steps.saturating_sub(1) {
            for y in 0..self.height {
                for x in 0..self.width {
                    let neighbor_slots = self.neighbor_slot_count(x, y);
                    total += 1usize << (neighbor_slots + 1);
                }
            }
        }

        total
    }

    /// Count the exact number of symmetry-breaking clauses currently emitted.
    pub fn symmetry_clause_count(&self) -> usize {
        if !self.symmetry_breaking {
            return 0;
        }

        let mut total = 0;

        for t in 0..self.time_steps {
            let strength = self.calculate_constraint_strength(t);

            total += match strength {
                ConstraintStrength::Full => self.all_symmetry_clause_count(t),
                ConstraintStrength::Medium => {
                    self.lexicographic_clause_count(t) + self.rotational_clause_count()
                }
                ConstraintStrength::Light => self.lexicographic_clause_count(t),
            };
        }

        total
    }

    /// Count the exact number of clauses currently generated.
    pub fn total_clause_count(&self) -> usize {
        self.target_clause_count() + self.transition_clause_count() + self.symmetry_clause_count()
    }

    /// Get constraint generation statistics
    pub fn statistics(&self) -> ConstraintStatistics {
        ConstraintStatistics {
            width: self.width,
            height: self.height,
            time_steps: self.time_steps,
            total_variables: self.variable_manager.total_possible_variables(),
            target_clauses: self.target_clause_count(),
            transition_clauses: self.transition_clause_count(),
            symmetry_clauses: self.symmetry_clause_count(),
            total_clauses: self.total_clause_count(),
        }
    }

    fn neighbor_slot_count(&self, x: usize, y: usize) -> usize {
        match self.boundary_condition {
            BoundaryCondition::Dead => {
                let mut count = 0;

                for dy in -1..=1 {
                    for dx in -1..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }

                        let nx = x as isize + dx;
                        let ny = y as isize + dy;

                        if nx >= 0
                            && nx < self.width as isize
                            && ny >= 0
                            && ny < self.height as isize
                        {
                            count += 1;
                        }
                    }
                }

                count
            }
            BoundaryCondition::Wrap | BoundaryCondition::Mirror => 8,
        }
    }

    fn all_symmetry_clause_count(&self, t: usize) -> usize {
        let mut total = 0;

        if self.should_apply_symmetry_type(SymmetryType::Lexicographic) {
            total += self.lexicographic_clause_count(t);
        }
        if self.should_apply_symmetry_type(SymmetryType::Rotational) {
            total += self.rotational_clause_count();
        }
        if self.should_apply_symmetry_type(SymmetryType::Reflectional) {
            total += self.reflectional_clause_count();
        }
        if self.should_apply_symmetry_type(SymmetryType::Translational) {
            total += self.translational_clause_count();
        }

        total
    }

    fn lexicographic_clause_count(&self, t: usize) -> usize {
        if t == 0 {
            self.minimal_rotation_breaking_clause_count()
        } else {
            0
        }
    }

    fn minimal_rotation_breaking_clause_count(&self) -> usize {
        let total_cells = self.width * self.height;
        let constraint_positions = std::cmp::min(3, total_cells / 2);
        let mut total = 0;

        for i in 0..constraint_positions {
            let y1 = i / self.width;
            let x1 = i % self.width;
            let x2 = self.width - 1 - x1;
            let y2 = self.height - 1 - y1;
            let pos2 = y2 * self.width + x2;

            if i != pos2 && i < pos2 {
                total += 1;
            }
        }

        if self.width >= 3 && self.height >= 3 {
            total += self.bottom_right_sample_count() * self.top_left_sample_count();
        }

        total
    }

    fn bottom_right_sample_count(&self) -> usize {
        let mid_x = self.width / 2;
        let mid_y = self.height / 2;

        if mid_x >= self.width - 1 || mid_y >= self.height - 1 {
            return 0;
        }

        let mut count = 1;
        if mid_x + 1 < self.width - 1 {
            count += 1;
        }
        if mid_y + 1 < self.height - 1 {
            count += 1;
        }

        count
    }

    fn top_left_sample_count(&self) -> usize {
        let mut count = 1;

        if self.width / 2 > 0 {
            count += 1;
        }
        if self.height / 2 > 0 {
            count += 1;
        }

        count
    }

    fn rotational_clause_count(&self) -> usize {
        if self.width == self.height {
            2 * self.width * self.height
        } else {
            0
        }
    }

    fn reflectional_clause_count(&self) -> usize {
        if self.width == self.height {
            2 * self.width * self.height
        } else {
            0
        }
    }

    fn translational_clause_count(&self) -> usize {
        self.width * self.height - 1
    }
}

/// Statistics about constraint generation
#[derive(Debug, Clone)]
pub struct ConstraintStatistics {
    pub width: usize,
    pub height: usize,
    pub time_steps: usize,
    pub total_variables: usize,
    pub target_clauses: usize,
    pub transition_clauses: usize,
    pub symmetry_clauses: usize,
    pub total_clauses: usize,
}

impl std::fmt::Display for ConstraintStatistics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Constraint Generation Statistics:")?;
        writeln!(f, "  Grid size: {}x{}", self.width, self.height)?;
        writeln!(f, "  Time steps: {}", self.time_steps)?;
        writeln!(f, "  Total variables: {}", self.total_variables)?;
        writeln!(f, "  Target clauses: {}", self.target_clauses)?;
        writeln!(f, "  Transition clauses: {}", self.transition_clauses)?;
        writeln!(f, "  Symmetry clauses: {}", self.symmetry_clauses)?;
        writeln!(f, "  Total clauses: {}", self.total_clauses)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BoundaryCondition;

    #[test]
    fn test_clause_creation() {
        let clause = Clause::new(vec![1, -2, 3]);
        assert_eq!(clause.literals, vec![1, -2, 3]);
        assert!(!clause.is_empty());
        assert!(!clause.is_unit());

        let unit_clause = Clause::unit(5);
        assert!(unit_clause.is_unit());
        assert_eq!(unit_clause.literals, vec![5]);
    }

    #[test]
    fn test_constraint_generator_creation() {
        let cg = ConstraintGenerator::new(3, 3, 2, BoundaryCondition::Dead, false);

        assert_eq!(cg.width, 3);
        assert_eq!(cg.height, 3);
        assert_eq!(cg.time_steps, 2);
        assert_eq!(cg.symmetry_breaking, false);
    }

    #[test]
    fn test_target_constraints() {
        let mut cg = ConstraintGenerator::new(2, 2, 2, BoundaryCondition::Dead, false);

        let cells = vec![vec![true, false], vec![false, true]];
        let target_grid = Grid::from_cells(cells, BoundaryCondition::Dead).unwrap();

        let constraints = cg.generate_target_constraints(&target_grid).unwrap();
        assert_eq!(constraints.len(), 4); // 2x2 grid = 4 cells

        // Check that constraints fix the target state
        assert!(constraints
            .iter()
            .any(|c| c.literals == vec![cg.variable_manager.cell_variable(0, 0, 1).unwrap()]));
        assert!(constraints
            .iter()
            .any(|c| c.literals == vec![-cg.variable_manager.cell_variable(1, 0, 1).unwrap()]));
    }
}
