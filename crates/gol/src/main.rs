use clap::Parser;
use gol::grid::StandardGrid;
use gol::prelude::*;
use std::io::{self, Write};
use std::{thread, time};

#[derive(Parser)]
#[command(name = "gol")]
#[command(about = "A high-performance Conway's Game of Life simulator")]
#[command(version)]
struct Args {
    /// Input file containing the initial grid state (1s and 0s)
    #[arg(short, long, default_value = "default.txt")]
    input: String,

    /// Number of generations to simulate
    #[arg(short, long, default_value = "8")]
    generations: usize,

    /// Frame duration in milliseconds for visual simulation
    #[arg(short, long, default_value = "400")]
    frame_duration: u64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    println!("Game of Life Optimization Demo");
    println!("==============================");

    let grid = match StandardGrid::from_file(&args.input) {
        Ok(grid) => {
            println!("Loaded initial state from: {}", args.input);
            grid
        }
        Err(e) => {
            println!(
                "Could not load file '{}', using default pattern: {}",
                args.input, e
            );
            let initial_state = [
                "⬜███⬜⬜⬜⬜⬜⬜⬜⬜⬜⬜⬜⬜⬜⬜⬜",
                "⬜██⬜⬜██⬜⬜██⬜⬜███⬜██",
                "██⬜█⬜⬜█⬜⬜⬜⬜█⬜⬜██⬜⬜⬜",
                "⬜⬜⬜█⬜⬜██⬜█⬜⬜█⬜⬜██⬜⬜",
                "⬜██⬜⬜█⬜█⬜⬜⬜██⬜█⬜⬜█⬜",
                "⬜⬜███⬜⬜⬜█⬜███⬜██⬜██",
                "⬜⬜⬜⬜⬜⬜██⬜⬜█⬜███⬜██⬜",
            ];
            StandardGrid::from_string_pattern(&initial_state, '█', '⬜')?
        }
    };

    let mut engine = auto_from_grid_ultimate_engine(&grid as &dyn Grid);

    println!("\nRunning visual simulation with Ultimate Engine...");
    println!("Grid size: {}x{}", engine.width(), engine.height());
    println!("Initial live cells: {}", engine.count_live_cells());
    println!("Generations to simulate: {}", args.generations);

    print!("\x1b[?1049h"); // Enter alternate screen
    io::stdout().flush().unwrap();

    let frame_duration = time::Duration::from_millis(args.frame_duration);

    for step in 0..=args.generations {
        print!("\x1b[H"); // Move cursor to top
        print!("\x1b[2J"); // Clear screen

        println!("Step: {} | Live cells: {}", step, engine.count_live_cells());
        print_grid_from_engine(&engine);

        io::stdout().flush().unwrap();
        thread::sleep(frame_duration);

        if step < args.generations {
            engine.step();
        }
    }

    thread::sleep(time::Duration::from_millis(2000));
    print!("\x1b[?1049l"); // Exit alternate screen
    io::stdout().flush().unwrap();

    println!("\nSimulation complete!");
    println!("Ultimate Engine features demonstrated:");
    println!("- Bit-packed representation (64 cells per u64)");
    println!("- SIMD parallelism for massive speedup");
    println!("- Advanced bit manipulation algorithms");
    println!("- Multi-threading with Rayon");

    Ok(())
}

fn print_grid_from_engine(engine: &Box<dyn GameOfLifeEngine>) {
    let mut output = String::new();
    for row in 0..engine.height() {
        for col in 0..engine.width() {
            let cell = engine.get_cell(row, col);
            let square = if cell { "⬛" } else { "⬜" };
            output.push_str(square);
        }
        output.push('\n');
    }
    print!("{}", output);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_conversion() {
        let pattern = ["⬜█⬜", "█⬜█", "⬜█⬜"];
        let grid = StandardGrid::from_string_pattern(&pattern, '█', '⬜').unwrap();

        assert_eq!(grid.width(), 3);
        assert_eq!(grid.height(), 3);
        assert_eq!(grid.count_live_cells(), 4);

        assert!(!grid.get_cell(0, 0)); // ⬜
        assert!(grid.get_cell(0, 1)); // █
        assert!(!grid.get_cell(0, 2)); // ⬜
    }

    #[test]
    fn test_file_loading() {
        let test_content = "101\n010\n101";
        std::fs::write("test_pattern.txt", test_content).unwrap();

        let grid = StandardGrid::from_file("test_pattern.txt").unwrap();
        assert_eq!(grid.width(), 3);
        assert_eq!(grid.height(), 3);
        assert_eq!(grid.count_live_cells(), 5);

        std::fs::remove_file("test_pattern.txt").unwrap();
    }

    #[test]
    fn test_ultimate_engine_functionality() {
        let pattern = ["...", "###", "..."];
        let grid = StandardGrid::from_string_pattern(&pattern, '#', '.').unwrap();
        let mut engine = UltimateEngine::<4>::from_grid(&grid);

        assert_eq!(engine.count_live_cells(), 3);
        engine.step();
        assert_eq!(engine.count_live_cells(), 3);
        engine.step();
        assert_eq!(engine.count_live_cells(), 3);
    }

    #[test]
    fn test_engine_equivalence() {
        let pattern = [".....", ".###.", ".....", ".###.", "....."];
        let grid = StandardGrid::from_string_pattern(&pattern, '#', '.').unwrap();

        let mut naive_engine = NaiveEngine::from_grid(&grid as &dyn Grid);
        let mut ultimate_engine = UltimateEngine::<4>::from_grid(&grid as &dyn Grid);

        for step in 0..5 {
            assert_eq!(
                naive_engine.get_grid().count_live_cells(),
                ultimate_engine.count_live_cells(),
                "Engines diverged at step {}",
                step
            );

            naive_engine.step();
            ultimate_engine.step();
        }
    }
}
