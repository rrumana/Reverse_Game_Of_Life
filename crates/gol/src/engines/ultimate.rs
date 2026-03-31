use crate::engines::{EngineInfo, GameOfLifeEngine};
use crate::grid::Grid;
use rayon::{ThreadPool, ThreadPoolBuilder};
use std::fmt::{Display, Formatter};
use std::mem::swap;
use std::simd::Simd;
use std::thread::available_parallelism;

/// Ultimate Game of Life engine with configurable SIMD width
pub struct UltimateEngine<const N: usize = 4> {
    pool: Option<ThreadPool>,
    field: Vec<u64>,
    new_field: Vec<u64>,
    height: usize,        // includes padding (+2)
    columns: usize,       // includes padding and SIMD alignment
    actual_width: usize,  // user-visible width
    actual_height: usize, // user-visible height
    boundary_masks: Vec<u64>,
    boundary_x_start: usize,
}

/// Helper function for ceiling division
fn div_ceil(x: usize, y: usize) -> usize {
    (x + y - 1) / y
}

/// Check if SIMD support is available at compile time
fn simd_supported() -> bool {
    true
}

impl<const N: usize> UltimateEngine<N> {
    /// Create a new ultimate engine with the specified grid dimensions
    pub fn new(width: usize, height: usize) -> Self {
        // Create thread pool only for native platforms, not WebAssembly
        let pool = if cfg!(target_arch = "wasm32") {
            // WebAssembly: No thread pool needed, we'll run everything sequentially
            None
        } else {
            // Native platforms: use available parallelism with fallback
            let threads = available_parallelism().map(|n| n.into()).unwrap_or(2);
            ThreadPoolBuilder::new().num_threads(threads).build().ok()
        };

        // Reference-style column calculation with SIMD alignment and padding
        let columns = div_ceil(div_ceil(width, 64), N) * N + 2;
        let padded_height = height + 2;

        // Pre-compute boundary masks for performance optimization
        let boundary_x_start = div_ceil(width, 64);
        let mut boundary_masks = vec![!0u64; columns];

        // Calculate masks for columns that cross the actual width boundary
        for col in 0..columns {
            let global_x = if col == 0 { 0 } else { (col - 1) * 64 };
            if global_x >= width {
                boundary_masks[col] = 0;
            } else if global_x + 64 > width {
                let bits_to_keep = width - global_x;
                boundary_masks[col] = !0u64 << (64 - bits_to_keep);
            }
        }

        Self {
            pool,
            field: vec![0; columns * padded_height],
            new_field: vec![0; columns * padded_height],
            height: padded_height,
            columns,
            actual_width: width,
            actual_height: height,
            boundary_masks,
            boundary_x_start,
        }
    }

    /// Set a cell in the grid (using 1-based indexing due to padding)
    pub fn set(&mut self, x: usize, y: usize) {
        if x >= self.actual_width || y >= self.actual_height {
            return;
        }

        let column = x / 64 + 1; // +1 for padding
        let bit = 0x8000_0000_0000_0000 >> (x % 64); // MSB first (reference style)
        self.field[(y + 1) * self.columns + column] |= bit;
    }

    /// Get a cell from the grid (using 1-based indexing due to padding)
    pub fn get(&self, x: usize, y: usize) -> bool {
        if x >= self.actual_width || y >= self.actual_height {
            return false;
        }

        let column = x / 64 + 1; // +1 for padding
        let bit = 0x8000_0000_0000_0000 >> (x % 64); // MSB first (reference style)
        (self.field[(y + 1) * self.columns + column] & bit) != 0
    }

    /// Count live cells in the grid
    pub fn count_live_cells(&self) -> usize {
        let mut count = 0;
        for y in 0..self.actual_height {
            for x in 0..self.actual_width {
                if self.get(x, y) {
                    count += 1;
                }
            }
        }
        count
    }

    /// Reference implementation's optimized full/half adder algorithm
    #[inline(always)]
    fn sub_step(mut center: Simd<u64, N>, nbs: &[Simd<u64, N>; 8]) -> Simd<u64, N> {
        // Stage 0: First level of addition using full/half adders
        let ta0 = nbs[0] ^ nbs[1];
        let a8 = ta0 ^ nbs[2];
        let b0 = (nbs[0] & nbs[1]) | (ta0 & nbs[2]);

        let ta3 = nbs[3] ^ nbs[4];
        let a9 = ta3 ^ nbs[5];
        let b1 = (nbs[3] & nbs[4]) | (ta3 & nbs[5]);

        let aa = nbs[6] ^ nbs[7];
        let b2 = nbs[6] & nbs[7];

        // Stage 1: Second level of addition
        let ta8 = a8 ^ a9;
        let ab = ta8 ^ aa;
        let b3 = (a8 & a9) | (ta8 & aa);

        let tb0 = b0 ^ b1;
        let b4 = tb0 ^ b2;
        let c0 = (b0 & b1) | (tb0 & b2);

        // Rules
        center |= ab;
        center &= b3 ^ b4;
        center &= !c0;

        center
    }

    /// Get SIMD chunk from field
    #[inline(always)]
    fn get_simd(field: &[u64], i: usize) -> Simd<u64, N> {
        Simd::from_slice(&field[i..i + N])
    }

    /// Step the simulation for the specified number of steps
    pub fn step_batch(&mut self, steps: u32) {
        for _ in 0..steps {
            let columns = self.columns;
            let boundary_x_start = self.boundary_x_start;
            let boundary_masks = &self.boundary_masks;

            if let Some(ref pool) = self.pool {
                // Use thread pool for parallel processing
                let threads = pool.current_num_threads();
                let simulation_rows = self.height - 2;
                let chunk_size = (simulation_rows + threads - 1) / threads;

                pool.scope(|scope| {
                    for (i, target) in self.new_field
                        [self.columns..self.columns * self.height - self.columns]
                        .chunks_mut(chunk_size * self.columns)
                        .enumerate()
                    {
                        let field = &self.field;
                        let boundary_masks = boundary_masks;
                        scope.spawn(move |_| {
                            for yl in 0..(target.len() / columns) {
                                let y = yl + i * chunk_size + 1;

                                // Prefetch next row for better cache performance (x86_64 only)
                                #[cfg(target_arch = "x86_64")]
                                if yl + 1 < target.len() / columns {
                                    let next_row_start = (y + 1) * columns;
                                    unsafe {
                                        std::arch::x86_64::_mm_prefetch(
                                            field.as_ptr().add(next_row_start) as *const i8,
                                            std::arch::x86_64::_MM_HINT_T0,
                                        );
                                    }
                                }

                                // Process columns in chunks for better cache locality
                                for x in (1..columns - 1).step_by(N) {
                                    let i = y * columns + x;

                                    let center = Self::get_simd(field, i);

                                    let mut nbs = [
                                        shr(Self::get_simd(field, i - columns)),
                                        Self::get_simd(field, i - columns),
                                        shl(Self::get_simd(field, i - columns)),
                                        shr(Self::get_simd(field, i)),
                                        shl(Self::get_simd(field, i)),
                                        shr(Self::get_simd(field, i + columns)),
                                        Self::get_simd(field, i + columns),
                                        shl(Self::get_simd(field, i + columns)),
                                    ];

                                    // fix bits in neighbouring columns
                                    nbs[0][0] |= (field[i - columns - 1] & 1) << 63;
                                    nbs[2][N - 1] |= (field[i - columns + N] & (1 << 63)) >> 63;
                                    nbs[3][0] |= (field[i - 1] & 0x1) << 63;
                                    nbs[4][N - 1] |= (field[i + N] & (1 << 63)) >> 63;
                                    nbs[5][0] |= (field[i + columns - 1] & 1) << 63;
                                    nbs[7][N - 1] |= (field[i + columns + N] & (1 << 63)) >> 63;

                                    let mut result = Self::sub_step(center, &nbs);

                                    // Optimized boundary masking using pre-computed masks
                                    // Only apply masking if we're at or beyond the boundary region
                                    if x >= boundary_x_start {
                                        for lane in 0..N {
                                            let col_idx = x + lane;
                                            if col_idx < boundary_masks.len() {
                                                result[lane] &= boundary_masks[col_idx];
                                            }
                                        }
                                    }

                                    target[yl * columns + x..yl * columns + x + N]
                                        .copy_from_slice(result.as_array());
                                }
                            }
                        });
                    }
                });
            } else {
                // Sequential processing for WebAssembly (no thread pool)
                for target in self.new_field
                    [self.columns..self.columns * self.height - self.columns]
                    .chunks_mut(self.columns)
                    .enumerate()
                {
                    let (i, target_row) = target;
                    let y = i + 1;
                    let field = &self.field;

                    // Process columns in chunks for better cache locality
                    for x in (1..columns - 1).step_by(N) {
                        let i = y * columns + x;

                        let center = Self::get_simd(field, i);

                        let mut nbs = [
                            shr(Self::get_simd(field, i - columns)),
                            Self::get_simd(field, i - columns),
                            shl(Self::get_simd(field, i - columns)),
                            shr(Self::get_simd(field, i)),
                            shl(Self::get_simd(field, i)),
                            shr(Self::get_simd(field, i + columns)),
                            Self::get_simd(field, i + columns),
                            shl(Self::get_simd(field, i + columns)),
                        ];

                        // fix bits in neighbouring columns
                        nbs[0][0] |= (field[i - columns - 1] & 1) << 63;
                        nbs[2][N - 1] |= (field[i - columns + N] & (1 << 63)) >> 63;
                        nbs[3][0] |= (field[i - 1] & 0x1) << 63;
                        nbs[4][N - 1] |= (field[i + N] & (1 << 63)) >> 63;
                        nbs[5][0] |= (field[i + columns - 1] & 1) << 63;
                        nbs[7][N - 1] |= (field[i + columns + N] & (1 << 63)) >> 63;

                        let mut result = Self::sub_step(center, &nbs);

                        // Optimized boundary masking using pre-computed masks
                        // Only apply masking if we're at or beyond the boundary region
                        if x >= boundary_x_start {
                            for lane in 0..N {
                                let col_idx = x + lane;
                                if col_idx < boundary_masks.len() {
                                    result[lane] &= boundary_masks[col_idx];
                                }
                            }
                        }

                        target_row[x..x + N].copy_from_slice(result.as_array());
                    }
                }
            }
            swap(&mut self.field, &mut self.new_field);
        }
    }

    /// Get columns for debugging
    pub fn get_columns(&self) -> usize {
        self.columns
    }

    /// Get height for debugging
    pub fn get_height(&self) -> usize {
        self.height
    }

    /// Get performance statistics
    pub fn performance_stats(&self) -> PerformanceStats {
        PerformanceStats {
            memory_usage_bytes: (self.field.len() + self.new_field.len()) * 8,
            bits_per_cell: 1.0,
            simd_enabled: true,
            simd_width: N,
            parallel_columns: self.columns,
        }
    }
}

/// Performance statistics for the engine
#[derive(Debug, Clone)]
pub struct PerformanceStats {
    pub memory_usage_bytes: usize,
    pub bits_per_cell: f64,
    pub simd_enabled: bool,
    pub simd_width: usize,
    pub parallel_columns: usize,
}

/// SIMD shift left with cross-lane handling (reference implementation)
#[inline(always)]
pub fn shl<const N: usize>(v: Simd<u64, N>) -> Simd<u64, N> {
    let mut mask = [0x00000_0000_0000_0001; N];
    mask[N - 1] = 0;

    let neighbouring_bits =
        (v >> Simd::splat(63)).rotate_elements_left::<1>() & Simd::from_array(mask);
    (v << Simd::splat(1)) | neighbouring_bits
}

/// SIMD shift right with cross-lane handling (reference implementation)
#[inline(always)]
pub fn shr<const N: usize>(v: Simd<u64, N>) -> Simd<u64, N> {
    let mut mask = [0x8000_0000_0000_0000; N];
    mask[0] = 0;

    let neighbouring_bits =
        (v << Simd::splat(63)).rotate_elements_right::<1>() & Simd::from_array(mask);
    (v >> Simd::splat(1)) | neighbouring_bits
}

impl<const N: usize> GameOfLifeEngine for UltimateEngine<N> {
    fn step(&mut self) {
        self.step_batch(1);
    }

    fn get_grid(&self) -> &dyn Grid {
        panic!("UltimateEngine doesn't support direct grid access - use get_cell instead")
    }

    fn set_grid(&mut self, grid: &dyn Grid) {
        self.field.fill(0);
        self.new_field.fill(0);

        for row in 0..grid.height().min(self.actual_height) {
            for col in 0..grid.width().min(self.actual_width) {
                if grid.get_cell(row, col) {
                    self.set(col, row);
                }
            }
        }
    }

    fn benchmark_info(&self) -> EngineInfo {
        EngineInfo {
            name: "Ultimate".to_string(),
            description: format!(
                "Ultimate optimization: bit-packed (64 cells/u64), SIMD ({}x u64), parallel arithmetic, multi-threading",
                N
            ),
            memory_per_cell_bits: 1.0,
            supports_parallel: true,
            supports_simd: true,
            min_grid_size: Some((64, 64)),
            max_grid_size: None,
        }
    }

    fn get_cell(&self, row: usize, col: usize) -> bool {
        self.get(col, row)
    }

    fn width(&self) -> usize {
        self.actual_width
    }

    fn height(&self) -> usize {
        self.actual_height
    }

    fn count_live_cells(&self) -> usize {
        let mut count = 0;
        for row in 0..self.actual_height {
            for col in 0..self.actual_width {
                if self.get(col, row) {
                    count += 1;
                }
            }
        }
        count
    }

    fn run_steps(&mut self, steps: usize) {
        self.step_batch(steps as u32);
    }
}

impl<const N: usize> Display for UltimateEngine<N> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut frame = String::new();

        for y in 0..self.actual_height {
            for x in 0..self.actual_width {
                if self.get(x, y) {
                    frame.push('█');
                } else {
                    frame.push('.');
                }
            }
            frame.push('\n');
        }

        write!(f, "{frame}")
    }
}

/// Create an UltimateEngine from an existing grid
impl<const N: usize> UltimateEngine<N> {
    pub fn from_grid(grid: &dyn Grid) -> Self {
        let mut engine = Self::new(grid.width(), grid.height());
        engine.set_grid(grid);
        engine
    }

    /// Get cell value (for compatibility)
    pub fn get_cell(&self, row: usize, col: usize) -> bool {
        self.get(col, row)
    }

    /// Get grid width
    pub fn width(&self) -> usize {
        self.actual_width
    }

    /// Get grid height
    pub fn height(&self) -> usize {
        self.actual_height
    }
}

/// Create an UltimateEngine with automatic SIMD width detection
pub fn auto_new_ultimate_engine(width: usize, height: usize) -> Box<dyn GameOfLifeEngine> {
    if simd_supported() {
        Box::new(UltimateEngine::<4>::new(width, height))
    } else {
        Box::new(crate::engines::NaiveEngine::new(width, height))
    }
}

/// Create an UltimateEngine from a grid with automatic SIMD width detection
pub fn auto_from_grid_ultimate_engine(grid: &dyn Grid) -> Box<dyn GameOfLifeEngine> {
    if simd_supported() {
        Box::new(UltimateEngine::<4>::from_grid(grid))
    } else {
        Box::new(crate::engines::NaiveEngine::from_grid(grid))
    }
}

/// Create an UltimateEngine with automatic SIMD width detection and runtime error handling
pub fn safe_auto_new_ultimate_engine(width: usize, height: usize) -> Box<dyn GameOfLifeEngine> {
    if simd_supported() {
        match std::panic::catch_unwind(|| UltimateEngine::<4>::new(width, height)) {
            Ok(engine) => Box::new(engine),
            Err(_) => Box::new(crate::engines::NaiveEngine::new(width, height)),
        }
    } else {
        Box::new(crate::engines::NaiveEngine::new(width, height))
    }
}

/// Runtime SIMD width detection and engine creation
pub fn create_optimal_engine(width: usize, height: usize) -> Box<dyn GameOfLifeEngine> {
    auto_new_ultimate_engine(width, height)
}
