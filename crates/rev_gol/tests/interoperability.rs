use rev_gol::{
    config::BoundaryCondition,
    game_of_life::io::{grid_to_string, parse_grid_from_string},
    utils::resolve_workspace_path,
};
use text_to_input::text_to_pixel_art;

#[test]
fn text_to_input_output_parses_as_rev_gol_grid() {
    let rendered = text_to_pixel_art("GoL").expect("text rendering should succeed");
    let grid = parse_grid_from_string(&rendered, BoundaryCondition::Dead)
        .expect("rendered text should be valid rev_gol input");

    assert_eq!(grid.height, rendered.lines().count());
    assert_eq!(grid.width, rendered.lines().next().unwrap().len());
    assert!(grid.living_count() > 0);
    assert_eq!(grid_to_string(&grid).trim(), rendered.trim());
}

#[test]
fn workspace_example_target_is_parseable() {
    let path = resolve_workspace_path("input/target_states/blinker.txt");
    let text = std::fs::read_to_string(&path).expect("workspace example should exist");
    parse_grid_from_string(&text, BoundaryCondition::Dead)
        .expect("workspace example target should parse");
}
