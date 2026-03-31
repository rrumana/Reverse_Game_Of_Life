# Game of Life - High-Performance Rust Implementation

A highly optimized Conway's Game of Life simulator written in Rust, featuring advanced SIMD parallelism and automatic hardware detection for maximum performance.

## Overview

This implementation demonstrates multiple optimization techniques for Conway's Game of Life, from basic cell-by-cell simulation to advanced bit-packed SIMD operations. The program automatically detects your hardware capabilities and selects the optimal engine configuration.

## Key Optimizations

The **UltimateEngine** provides massive performance improvements over naive implementations through:

- **Bit-packed representation**: 64 cells stored per u64 (64x memory efficiency)
- **SIMD parallelism**: Vectorized operations using Rust's portable SIMD
- **Automatic hardware detection**: Optimal SIMD width selection (4, 8, or 16)
- **Advanced bit manipulation**: Full/half adder algorithms for neighbor counting
- **Multi-threading**: Parallel processing with Rayon thread pools
- **Boundary optimization**: Ghost cells eliminate boundary checks
- **Memory prefetching**: Cache-friendly memory access patterns

These optimizations deliver **25x+ performance improvements** over basic implementations.

## Usage

### Basic Usage

Run the visual Game of Life simulation with default settings:

```bash
cargo run -p gol --release
```

### Command Line Options

The program supports several command-line arguments:

```bash
cargo run -p gol --release -- [OPTIONS]
```

**Options:**
- `-i, --input <FILE>`: Input file containing the initial grid state (default: `default.txt`)
- `-g, --generations <NUM>`: Number of generations to simulate (default: 9)
- `-f, --frame-duration <MS>`: Frame duration in milliseconds for visual simulation (default: 400)
- `-h, --help`: Print help information
- `-V, --version`: Print version information

### Examples

```bash
# Run with default settings
cargo run -p gol --release

# Use a custom input file and run for 20 generations
cargo run -p gol --release -- --input my_pattern.txt --generations 20

# Run faster animation (200ms per frame)
cargo run -p gol --release -- --frame-duration 200

# Use short flags
cargo run -p gol --release -- -i custom.txt -g 15 -f 300
```

### Input File Format

Input files should contain a grid of 1s and 0s, where:
- `1` represents a live cell
- `0` represents a dead cell

Example input file (`my_pattern.txt`):
```
0110
1001
1001
0110
```

The program will automatically detect the grid dimensions from the file. All rows must have the same length.

### Default Pattern

The included `default.txt` file contains the original demo pattern converted to the 1s and 0s format.

### What the simulation does:

1. Automatically detect your hardware capabilities
2. Select the optimal SIMD configuration
3. Load the initial state from the specified input file (or use default pattern)
4. Run a visual simulation showing the evolution over the specified number of generations
5. Display performance information and live cell counts

## Requirements

- Rust nightly toolchain (for portable SIMD support)
- Modern CPU with SIMD support (SSE2/AVX/AVX-512)

## Performance

The implementation achieves:
- **118+ GCells/s** on modern hardware
- **Automatic fallback** to compatible engines on older systems
- **Zero-copy** grid operations where possible
- **Optimal memory layout** for cache efficiency

The program automatically benchmarks and selects the best SIMD width for your specific hardware configuration.
