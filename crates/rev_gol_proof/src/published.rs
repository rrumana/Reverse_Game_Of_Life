//! Support for loading and verifying the vendored published gadget library.

use crate::verifier::{
    life_neighborhood, CellCoord, CellLiteral, ChargingCheck, GadgetPattern, GadgetVerifier, Port,
    PortAssignment, RelationCheckReport,
};
use anyhow::{Context, Result};
use rev_gol::config::BoundaryCondition;
use rev_gol::game_of_life::Grid;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishedSize(pub usize, pub usize);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedSpec {
    pub path: &'static str,
    pub name: &'static str,
    pub size: Option<PublishedSize>,
    pub align: Option<(Option<i8>, Option<i8>, Option<i8>, Option<i8>)>,
    pub charging: Vec<(&'static str, &'static str)>,
    pub relation_wires: &'static str,
    pub relation_items: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedPattern {
    pub width: usize,
    pub height: usize,
    pub cells: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WireAnchors {
    pub east: Option<CellCoord>,
    pub north: Option<CellCoord>,
    pub west: Option<CellCoord>,
    pub south: Option<CellCoord>,
}

impl PublishedPattern {
    pub fn from_csv_file(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read published gadget file {}", path.display()))?;
        Self::from_csv_str(&text)
    }

    pub fn from_csv_str(text: &str) -> Result<Self> {
        let mut rows = Vec::new();

        for raw_line in text.lines() {
            let line = raw_line.trim_end();
            if line.starts_with('%') || line.is_empty() {
                continue;
            }

            let digits: Vec<u8> = if line.contains(char::is_whitespace) {
                line.split_whitespace()
                    .map(str::parse::<u8>)
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .context("Failed to parse published gadget CSV row")?
            } else {
                line.chars()
                    .map(|ch| {
                        ch.to_digit(10).map(|digit| digit as u8).with_context(|| {
                            format!("Failed to parse digit '{}' in published row", ch)
                        })
                    })
                    .collect::<Result<Vec<_>>>()?
            };

            if !digits.is_empty() {
                rows.push(digits);
            }
        }

        if rows.is_empty() {
            anyhow::bail!("Published gadget CSV is empty");
        }

        let width = rows[0].len();
        if rows.iter().any(|row| row.len() != width) {
            anyhow::bail!("Published gadget CSV has inconsistent row lengths");
        }

        Ok(Self {
            width,
            height: rows.len(),
            cells: rows,
        })
    }

    pub fn to_target_grid(&self) -> Result<Grid> {
        let bool_rows: Vec<Vec<bool>> = self
            .cells
            .iter()
            .map(|row| row.iter().map(|&value| value > 0).collect())
            .collect();
        Grid::from_cells(bool_rows, BoundaryCondition::Dead)
    }

    pub fn find_wires(&self) -> WireAnchors {
        let mut west = None;
        let mut east = None;
        let mut north = None;
        let mut south = None;

        for y in 0..self.height.saturating_sub(1) {
            if (0..=1).all(|x| (0..=1).all(|dy| self.cells[y + dy][x] > 0)) {
                for x in 0..=2 {
                    if (0..=1).all(|dx| {
                        (0..=1).all(|dy| {
                            self.cells[y + dy][x + dx] == 1 || self.cells[y + dy][x + dx] == 3
                        })
                    }) {
                        west = Some(CellCoord::new(x as isize, y as isize));
                    }
                }
            }

            if ((self.width - 2)..=self.width - 1)
                .all(|x| (0..=1).all(|dy| self.cells[y + dy][x] > 0))
            {
                for x in (self.width - 4..=self.width - 2).rev() {
                    if (0..=1).all(|dx| {
                        (0..=1).all(|dy| {
                            self.cells[y + dy][x + dx] == 1 || self.cells[y + dy][x + dx] == 3
                        })
                    }) {
                        east = Some(CellCoord::new(x as isize, y as isize));
                    }
                }
            }
        }

        for x in 0..self.width.saturating_sub(1) {
            if (0..=1).all(|dx| (0..=1).all(|y| self.cells[y][x + dx] > 0)) {
                for y in 0..=2 {
                    if (0..=1).all(|dx| {
                        (0..=1).all(|dy| {
                            self.cells[y + dy][x + dx] == 1 || self.cells[y + dy][x + dx] == 3
                        })
                    }) {
                        north = Some(CellCoord::new(x as isize, y as isize));
                    }
                }
            }

            if (0..=1)
                .all(|dx| ((self.height - 2)..=self.height - 1).all(|y| self.cells[y][x + dx] > 0))
            {
                for y in (self.height - 4..=self.height - 2).rev() {
                    if (0..=1).all(|dx| {
                        (0..=1).all(|dy| {
                            self.cells[y + dy][x + dx] == 1 || self.cells[y + dy][x + dx] == 3
                        })
                    }) {
                        south = Some(CellCoord::new(x as isize, y as isize));
                    }
                }
            }
        }

        WireAnchors {
            east,
            north,
            west,
            south,
        }
    }

    pub fn phase_alignment(&self) -> (Option<i8>, Option<i8>, Option<i8>, Option<i8>) {
        let wires = self.find_wires();
        (
            wires
                .east
                .map(|coord| ((coord.x - self.width as isize + 2).rem_euclid(3) - 1) as i8),
            wires
                .north
                .map(|coord| ((coord.y as isize - 1).rem_euclid(3) - 1) as i8),
            wires
                .west
                .map(|coord| ((coord.x as isize - 1).rem_euclid(3) - 1) as i8),
            wires
                .south
                .map(|coord| ((coord.y - self.height as isize + 2).rem_euclid(3) - 1) as i8),
        )
    }

    pub fn to_gadget_pattern(&self, name: impl Into<String>) -> Result<GadgetPattern> {
        let wires = self.find_wires();
        let mut gadget = GadgetPattern::new(name, self.to_target_grid()?);

        if let Some(anchor) = wires.east {
            gadget = gadget.with_port(east_port(anchor));
        }
        if let Some(anchor) = wires.north {
            gadget = gadget.with_port(north_port(anchor));
        }
        if let Some(anchor) = wires.west {
            gadget = gadget.with_port(west_port(anchor));
        }
        if let Some(anchor) = wires.south {
            gadget = gadget.with_port(south_port(anchor));
        }

        gadget = gadget.with_base_predecessor_literals(self.thick_border_literals(wires));
        Ok(gadget)
    }

    fn thick_border_literals(&self, wires: WireAnchors) -> Vec<CellLiteral> {
        let mut excepted = HashSet::new();

        if let Some(anchor) = wires.east {
            for y in anchor.y - 1..=anchor.y + 2 {
                excepted.insert(CellCoord::new(self.width as isize - 1, y));
                excepted.insert(CellCoord::new(self.width as isize, y));
            }
        }
        if let Some(anchor) = wires.north {
            for x in anchor.x - 1..=anchor.x + 2 {
                excepted.insert(CellCoord::new(x, -1));
                excepted.insert(CellCoord::new(x, 0));
            }
        }
        if let Some(anchor) = wires.west {
            for y in anchor.y - 1..=anchor.y + 2 {
                excepted.insert(CellCoord::new(-1, y));
                excepted.insert(CellCoord::new(0, y));
            }
        }
        if let Some(anchor) = wires.south {
            for x in anchor.x - 1..=anchor.x + 2 {
                excepted.insert(CellCoord::new(x, self.height as isize - 1));
                excepted.insert(CellCoord::new(x, self.height as isize));
            }
        }

        let mut result = HashSet::new();
        for y in 0..self.height {
            for x in 0..self.width {
                let vec = CellCoord::new(x as isize, y as isize);
                for neighbor in life_neighborhood(vec) {
                    let inside_pattern = neighbor.x >= 0
                        && neighbor.x < self.width as isize
                        && neighbor.y >= 0
                        && neighbor.y < self.height as isize;
                    if !inside_pattern {
                        if !excepted.contains(&vec) {
                            result.insert(CellLiteral::dead(vec.x, vec.y));
                        }
                        if !excepted.contains(&neighbor) {
                            result.insert(CellLiteral::dead(neighbor.x, neighbor.y));
                        }
                    }
                }
            }
        }

        let mut out: Vec<CellLiteral> = result.into_iter().collect();
        out.sort_by_key(|literal| literal.coord);
        out
    }
}

fn east_port(anchor: CellCoord) -> Port {
    Port::new("E")
        .with_state(
            "0",
            (0isize..=1)
                .flat_map(|dx| {
                    (-1isize..=2).map(move |dy| CellLiteral {
                        coord: CellCoord::new(anchor.x + dx, anchor.y + dy),
                        alive: dx == 0,
                    })
                })
                .collect(),
        )
        .with_state(
            "1",
            (0isize..=1)
                .flat_map(|dx| {
                    (-1isize..=2).map(move |dy| CellLiteral {
                        coord: CellCoord::new(anchor.x + dx, anchor.y + dy),
                        alive: dx == 1,
                    })
                })
                .collect(),
        )
}

fn west_port(anchor: CellCoord) -> Port {
    Port::new("W")
        .with_state(
            "0",
            (0isize..=1)
                .flat_map(|dx| {
                    (-1isize..=2).map(move |dy| CellLiteral {
                        coord: CellCoord::new(anchor.x + dx, anchor.y + dy),
                        alive: dx == 0,
                    })
                })
                .collect(),
        )
        .with_state(
            "1",
            (0isize..=1)
                .flat_map(|dx| {
                    (-1isize..=2).map(move |dy| CellLiteral {
                        coord: CellCoord::new(anchor.x + dx, anchor.y + dy),
                        alive: dx == 1,
                    })
                })
                .collect(),
        )
}

fn north_port(anchor: CellCoord) -> Port {
    Port::new("N")
        .with_state(
            "0",
            (-1isize..=2)
                .flat_map(|dx| {
                    (0isize..=1).map(move |dy| CellLiteral {
                        coord: CellCoord::new(anchor.x + dx, anchor.y + dy),
                        alive: dy == 0,
                    })
                })
                .collect(),
        )
        .with_state(
            "1",
            (-1isize..=2)
                .flat_map(|dx| {
                    (0isize..=1).map(move |dy| CellLiteral {
                        coord: CellCoord::new(anchor.x + dx, anchor.y + dy),
                        alive: dy == 1,
                    })
                })
                .collect(),
        )
}

fn south_port(anchor: CellCoord) -> Port {
    Port::new("S")
        .with_state(
            "0",
            (-1isize..=2)
                .flat_map(|dx| {
                    (0isize..=1).map(move |dy| CellLiteral {
                        coord: CellCoord::new(anchor.x + dx, anchor.y + dy),
                        alive: dy == 0,
                    })
                })
                .collect(),
        )
        .with_state(
            "1",
            (-1isize..=2)
                .flat_map(|dx| {
                    (0isize..=1).map(move |dy| CellLiteral {
                        coord: CellCoord::new(anchor.x + dx, anchor.y + dy),
                        alive: dy == 1,
                    })
                })
                .collect(),
        )
}

pub fn published_basic_specs() -> Vec<PublishedSpec> {
    vec![
        PublishedSpec {
            path: "gadgets/charger.cvs",
            name: "charger gadget",
            size: None,
            align: None,
            charging: vec![("", "N")],
            relation_wires: "N",
            relation_items: vec![vec![0], vec![1]],
        },
        PublishedSpec {
            path: "gadgets/splitter.cvs",
            name: "splitter gadget",
            size: None,
            align: None,
            charging: vec![("N", "ENS"), ("S", "ENS")],
            relation_wires: "ENS",
            relation_items: vec![vec![1, 0, 0], vec![0, 1, 1]],
        },
        PublishedSpec {
            path: "gadgets/CScombo1.cvs",
            name: "charged turn gadget 1",
            size: None,
            align: None,
            charging: vec![("", "ES")],
            relation_wires: "ES",
            relation_items: vec![vec![0, 1], vec![1, 0]],
        },
        PublishedSpec {
            path: "gadgets/CScombo2.cvs",
            name: "charged turn gadget 2",
            size: None,
            align: None,
            charging: vec![("", "ES")],
            relation_wires: "ES",
            relation_items: vec![vec![0, 0], vec![1, 1]],
        },
        PublishedSpec {
            path: "gadgets/not.cvs",
            name: "inverter gadget",
            size: None,
            align: None,
            charging: vec![("E", "W")],
            relation_wires: "EW",
            relation_items: vec![vec![0, 1], vec![1, 0]],
        },
        PublishedSpec {
            path: "gadgets/crossing.cvs",
            name: "crossing gadget",
            size: None,
            align: None,
            charging: vec![],
            relation_wires: "ENWS",
            relation_items: vec![
                vec![0, 0, 1, 1],
                vec![0, 1, 1, 0],
                vec![1, 0, 0, 1],
                vec![1, 1, 0, 0],
            ],
        },
        PublishedSpec {
            path: "gadgets/and.cvs",
            name: "logic gate gadget",
            size: None,
            align: None,
            charging: vec![],
            relation_wires: "ENW",
            relation_items: vec![vec![1, 1, 1], vec![0, 1, 1], vec![1, 0, 0], vec![0, 1, 0]],
        },
        PublishedSpec {
            path: "gadgets/enforcer.cvs",
            name: "enforcer gadget",
            size: None,
            align: None,
            charging: vec![("", "W")],
            relation_wires: "W",
            relation_items: vec![vec![1]],
        },
    ]
}

pub fn published_tile_specs() -> Vec<PublishedSpec> {
    vec![
        PublishedSpec {
            path: "small_squares/hor-wire.cvs",
            name: "horizontal wire tile",
            size: Some(PublishedSize(90, 90)),
            align: Some((Some(0), None, Some(0), None)),
            charging: vec![],
            relation_wires: "EW",
            relation_items: vec![vec![0, 0], vec![1, 1]],
        },
        PublishedSpec {
            path: "small_squares/ver-wire.cvs",
            name: "vertical wire tile",
            size: Some(PublishedSize(90, 90)),
            align: Some((None, Some(0), None, Some(0))),
            charging: vec![],
            relation_wires: "NS",
            relation_items: vec![vec![0, 0], vec![1, 1]],
        },
        PublishedSpec {
            path: "small_squares/ne-turn.cvs",
            name: "NE turn tile",
            size: Some(PublishedSize(90, 90)),
            align: Some((Some(1), Some(1), None, None)),
            charging: vec![],
            relation_wires: "NE",
            relation_items: vec![vec![0, 0], vec![1, 1]],
        },
        PublishedSpec {
            path: "small_squares/nw-turn.cvs",
            name: "NW turn tile",
            size: Some(PublishedSize(90, 90)),
            align: Some((None, Some(1), Some(1), None)),
            charging: vec![],
            relation_wires: "NW",
            relation_items: vec![vec![0, 0], vec![1, 1]],
        },
        PublishedSpec {
            path: "small_squares/sw-turn.cvs",
            name: "SW turn tile",
            size: Some(PublishedSize(90, 90)),
            align: Some((None, None, Some(-1), Some(-1))),
            charging: vec![],
            relation_wires: "SW",
            relation_items: vec![vec![0, 0], vec![1, 1]],
        },
        PublishedSpec {
            path: "small_squares/se-turn.cvs",
            name: "SE turn tile",
            size: Some(PublishedSize(90, 90)),
            align: Some((Some(-1), None, None, Some(-1))),
            charging: vec![],
            relation_wires: "SE",
            relation_items: vec![vec![0, 0], vec![1, 1]],
        },
        PublishedSpec {
            path: "small_squares/not.cvs",
            name: "NOT gate tile",
            size: Some(PublishedSize(90, 90)),
            align: Some((Some(0), None, Some(1), None)),
            charging: vec![],
            relation_wires: "EW",
            relation_items: vec![vec![0, 1], vec![1, 0]],
        },
        PublishedSpec {
            path: "small_squares/one.cvs",
            name: "always-1 tile",
            size: Some(PublishedSize(90, 90)),
            align: Some((None, None, Some(0), None)),
            charging: vec![],
            relation_wires: "W",
            relation_items: vec![vec![1]],
        },
        PublishedSpec {
            path: "small_squares/split.cvs",
            name: "splitter tile",
            size: Some(PublishedSize(90, 90)),
            align: Some((Some(-1), Some(-1), None, Some(-1))),
            charging: vec![],
            relation_wires: "ENS",
            relation_items: vec![vec![0, 0, 0], vec![1, 1, 1]],
        },
        PublishedSpec {
            path: "small_squares/cross.cvs",
            name: "crossing tile",
            size: Some(PublishedSize(90, 90)),
            align: Some((Some(-1), Some(0), Some(0), Some(-1))),
            charging: vec![],
            relation_wires: "ENWS",
            relation_items: vec![
                vec![0, 0, 0, 0],
                vec![0, 1, 0, 1],
                vec![1, 0, 1, 0],
                vec![1, 1, 1, 1],
            ],
        },
        PublishedSpec {
            path: "small_squares/or.cvs",
            name: "OR gate tile",
            size: Some(PublishedSize(90, 90)),
            align: Some((Some(0), Some(0), None, Some(-1))),
            charging: vec![],
            relation_wires: "ENS",
            relation_items: vec![vec![0, 0, 0], vec![1, 0, 1], vec![1, 1, 0], vec![1, 1, 1]],
        },
    ]
}

pub fn published_connector_specs() -> Vec<PublishedSpec> {
    vec![
        PublishedSpec {
            path: "connectors/connector-n1-n1.cvs",
            name: "connector -1 to -1",
            size: Some(PublishedSize(180, 90)),
            align: Some((Some(-1), None, Some(-1), None)),
            charging: vec![("", "EW")],
            relation_wires: "EW",
            relation_items: vec![vec![0, 0], vec![1, 1]],
        },
        PublishedSpec {
            path: "connectors/connector-0-n1.cvs",
            name: "connector 0 to -1",
            size: Some(PublishedSize(180, 90)),
            align: Some((Some(-1), None, Some(0), None)),
            charging: vec![("", "EW")],
            relation_wires: "EW",
            relation_items: vec![vec![0, 0], vec![1, 1]],
        },
        PublishedSpec {
            path: "connectors/connector-1-n1.cvs",
            name: "connector 1 to -1",
            size: Some(PublishedSize(180, 90)),
            align: Some((Some(-1), None, Some(1), None)),
            charging: vec![("", "EW")],
            relation_wires: "EW",
            relation_items: vec![vec![0, 0], vec![1, 1]],
        },
        PublishedSpec {
            path: "connectors/connector-n1-0.cvs",
            name: "connector -1 to 0",
            size: Some(PublishedSize(180, 90)),
            align: Some((Some(0), None, Some(-1), None)),
            charging: vec![("", "EW")],
            relation_wires: "EW",
            relation_items: vec![vec![0, 0], vec![1, 1]],
        },
        PublishedSpec {
            path: "connectors/connector-0-0.cvs",
            name: "connector 0 to 0",
            size: Some(PublishedSize(180, 90)),
            align: Some((Some(0), None, Some(0), None)),
            charging: vec![("", "EW")],
            relation_wires: "EW",
            relation_items: vec![vec![0, 0], vec![1, 1]],
        },
        PublishedSpec {
            path: "connectors/connector-1-0.cvs",
            name: "connector 1 to 0",
            size: Some(PublishedSize(180, 90)),
            align: Some((Some(0), None, Some(1), None)),
            charging: vec![("", "EW")],
            relation_wires: "EW",
            relation_items: vec![vec![0, 0], vec![1, 1]],
        },
        PublishedSpec {
            path: "connectors/connector-n1-1.cvs",
            name: "connector -1 to 1",
            size: Some(PublishedSize(180, 90)),
            align: Some((Some(1), None, Some(-1), None)),
            charging: vec![("", "EW")],
            relation_wires: "EW",
            relation_items: vec![vec![0, 0], vec![1, 1]],
        },
        PublishedSpec {
            path: "connectors/connector-0-1.cvs",
            name: "connector 0 to 1",
            size: Some(PublishedSize(180, 90)),
            align: Some((Some(1), None, Some(0), None)),
            charging: vec![("", "EW")],
            relation_wires: "EW",
            relation_items: vec![vec![0, 0], vec![1, 1]],
        },
        PublishedSpec {
            path: "connectors/connector-1-1.cvs",
            name: "connector 1 to 1",
            size: Some(PublishedSize(180, 90)),
            align: Some((Some(1), None, Some(1), None)),
            charging: vec![("", "EW")],
            relation_wires: "EW",
            relation_items: vec![vec![0, 0], vec![1, 1]],
        },
    ]
}

pub fn published_part1_specs() -> Vec<PublishedSpec> {
    let mut specs = published_basic_specs();
    specs.extend(published_tile_specs());
    specs.extend(published_connector_specs());
    specs
}

pub fn published_spec_named(name: &str) -> Option<PublishedSpec> {
    published_part1_specs()
        .into_iter()
        .find(|spec| spec.name == name)
}

pub fn published_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/published")
}

pub fn load_published_gadget(
    root: &Path,
    spec: &PublishedSpec,
) -> Result<(PublishedPattern, GadgetPattern)> {
    let path = root.join(spec.path);
    let published = PublishedPattern::from_csv_file(&path)?;
    let gadget = published.to_gadget_pattern(spec.name)?;
    Ok((published, gadget))
}

pub fn compose_published_patterns(
    placements: &[(&PublishedPattern, isize, isize)],
) -> Result<PublishedPattern> {
    if placements.is_empty() {
        anyhow::bail!("Cannot compose an empty published-pattern set");
    }

    let min_x = placements
        .iter()
        .map(|(_, x, _)| *x)
        .min()
        .context("Missing minimum x for composition")?;
    let min_y = placements
        .iter()
        .map(|(_, _, y)| *y)
        .min()
        .context("Missing minimum y for composition")?;
    let max_x = placements
        .iter()
        .map(|(pattern, x, _)| *x + pattern.width as isize)
        .max()
        .context("Missing maximum x for composition")?;
    let max_y = placements
        .iter()
        .map(|(pattern, _, y)| *y + pattern.height as isize)
        .max()
        .context("Missing maximum y for composition")?;

    let width = (max_x - min_x) as usize;
    let height = (max_y - min_y) as usize;
    let mut cells = vec![vec![0u8; width]; height];

    for (pattern, origin_x, origin_y) in placements {
        let translated_x = origin_x - min_x;
        let translated_y = origin_y - min_y;

        for (row_idx, row) in pattern.cells.iter().enumerate() {
            for (col_idx, &cell) in row.iter().enumerate() {
                if cell == 0 {
                    continue;
                }

                let x = (translated_x + col_idx as isize) as usize;
                let y = (translated_y + row_idx as isize) as usize;
                cells[y][x] = cells[y][x].max(cell);
            }
        }
    }

    Ok(PublishedPattern {
        width,
        height,
        cells,
    })
}

pub fn relation_assignments(spec: &PublishedSpec) -> (Vec<PortAssignment>, Vec<PortAssignment>) {
    let wires: Vec<String> = spec.relation_wires.chars().map(|c| c.to_string()).collect();
    let allowed = spec
        .relation_items
        .iter()
        .map(|bits| {
            let mut assignment = PortAssignment::new();
            for (wire, bit) in wires.iter().zip(bits.iter()) {
                assignment = assignment.with_state(wire.clone(), bit.to_string());
            }
            assignment
        })
        .collect::<Vec<_>>();

    let allowed_set = allowed
        .iter()
        .map(|assignment| assignment.states.clone())
        .collect::<HashSet<_>>();

    let mut forbidden = Vec::new();
    let total = 1usize << wires.len();
    for mask in 0..total {
        let mut assignment = PortAssignment::new();
        for (i, wire) in wires.iter().enumerate() {
            let bit = ((mask >> i) & 1) as u8;
            assignment = assignment.with_state(wire.clone(), bit.to_string());
        }
        if !allowed_set.contains(&assignment.states) {
            forbidden.push(assignment);
        }
    }

    (allowed, forbidden)
}

pub fn charging_input_assignments(spec: &PublishedSpec) -> Vec<(PortAssignment, Vec<String>)> {
    let mut out = Vec::new();

    for (inputs, outputs) in &spec.charging {
        let inputs_vec: Vec<String> = inputs.chars().map(|c| c.to_string()).collect();
        let outputs_vec: Vec<String> = outputs.chars().map(|c| c.to_string()).collect();

        let total = 1usize << inputs_vec.len();
        if inputs_vec.is_empty() {
            out.push((PortAssignment::new(), outputs_vec.clone()));
            continue;
        }

        for mask in 0..total {
            let mut assignment = PortAssignment::new();
            for (i, wire) in inputs_vec.iter().enumerate() {
                assignment =
                    assignment.with_state(wire.clone(), (((mask >> i) & 1) as u8).to_string());
            }
            out.push((assignment, outputs_vec.clone()));
        }
    }

    out
}

#[derive(Debug, Clone)]
pub struct PublishedVerificationReport {
    pub size_matches: bool,
    pub alignment_matches: bool,
    pub relation_report: RelationCheckReport,
    pub charging_reports: Vec<ChargingCheck>,
}

impl PublishedVerificationReport {
    pub fn is_success(&self) -> bool {
        self.size_matches
            && self.alignment_matches
            && self.relation_report.allowed_assignments_hold
            && self.relation_report.forbidden_assignments_hold
            && self
                .charging_reports
                .iter()
                .all(|report| report.all_outputs_are_named_states)
    }
}

pub fn verify_published_spec(
    verifier: &GadgetVerifier,
    root: &Path,
    spec: &PublishedSpec,
) -> Result<PublishedVerificationReport> {
    let (published, gadget) = load_published_gadget(root, spec)?;

    verify_published_pattern(verifier, &published, &gadget, spec)
}

pub fn verify_published_pattern(
    verifier: &GadgetVerifier,
    published: &PublishedPattern,
    gadget: &GadgetPattern,
    spec: &PublishedSpec,
) -> Result<PublishedVerificationReport> {

    let size_matches = spec
        .size
        .map(|PublishedSize(w, h)| published.width == w && published.height == h)
        .unwrap_or(true);
    let alignment_matches = spec
        .align
        .map(|expected| published.phase_alignment() == expected)
        .unwrap_or(true);

    let (allowed, forbidden) = relation_assignments(spec);
    let relation_report = verifier.verify_relation(&gadget, &allowed, &forbidden)?;

    let mut charging_reports = Vec::new();
    for (assignment, outputs) in charging_input_assignments(spec) {
        charging_reports.push(verifier.verify_charging_rule(&gadget, &assignment, &outputs)?);
    }

    Ok(PublishedVerificationReport {
        size_matches,
        alignment_matches,
        relation_report,
        charging_reports,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositionSearchPiece {
    pub spec_name: String,
    pub x_values: Vec<isize>,
    pub y_values: Vec<isize>,
}

impl CompositionSearchPiece {
    pub fn fixed(spec_name: impl Into<String>, x: isize, y: isize) -> Self {
        Self {
            spec_name: spec_name.into(),
            x_values: vec![x],
            y_values: vec![y],
        }
    }

    pub fn with_options(
        spec_name: impl Into<String>,
        x_values: Vec<isize>,
        y_values: Vec<isize>,
    ) -> Self {
        Self {
            spec_name: spec_name.into(),
            x_values,
            y_values,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompositionSearchResult {
    pub placements: Vec<(String, isize, isize)>,
    pub report: PublishedVerificationReport,
}

#[derive(Debug, Clone)]
pub struct CompositionSearchProgress {
    pub checked_candidates: usize,
    pub total_candidates: usize,
    pub matches_found: usize,
    pub current_candidate: Vec<(String, isize, isize)>,
}

pub fn count_composition_candidates(pieces: &[CompositionSearchPiece]) -> usize {
    pieces.iter().fold(1usize, |acc, piece| {
        acc.saturating_mul(piece.x_values.len().saturating_mul(piece.y_values.len()))
    })
}

pub fn search_composed_pattern_positions(
    verifier: &GadgetVerifier,
    root: &Path,
    pieces: &[CompositionSearchPiece],
    target_spec: &PublishedSpec,
    max_results: usize,
) -> Result<Vec<CompositionSearchResult>> {
    search_composed_pattern_positions_with_progress(
        verifier,
        root,
        pieces,
        target_spec,
        max_results,
        &mut |_| {},
    )
}

pub fn search_composed_pattern_positions_with_progress(
    verifier: &GadgetVerifier,
    root: &Path,
    pieces: &[CompositionSearchPiece],
    target_spec: &PublishedSpec,
    max_results: usize,
    on_progress: &mut dyn FnMut(CompositionSearchProgress),
) -> Result<Vec<CompositionSearchResult>> {
    if pieces.is_empty() {
        anyhow::bail!("Composition search requires at least one piece");
    }

    let loaded = pieces
        .iter()
        .map(|piece| {
            let spec = published_spec_named(&piece.spec_name)
                .with_context(|| format!("Unknown published spec '{}'", piece.spec_name))?;
            let (pattern, _) = load_published_gadget(root, &spec)?;
            Ok((piece.clone(), pattern))
        })
        .collect::<Result<Vec<_>>>()?;

    let mut results = Vec::new();
    let mut current = Vec::new();
    let mut checked_candidates = 0usize;
    let total_candidates = count_composition_candidates(pieces);
    search_composed_pattern_positions_recursive(
        verifier,
        &loaded,
        target_spec,
        max_results,
        &mut current,
        &mut results,
        &mut checked_candidates,
        total_candidates,
        on_progress,
    )?;
    Ok(results)
}

fn search_composed_pattern_positions_recursive(
    verifier: &GadgetVerifier,
    loaded: &[(CompositionSearchPiece, PublishedPattern)],
    target_spec: &PublishedSpec,
    max_results: usize,
    current: &mut Vec<(String, PublishedPattern, isize, isize)>,
    results: &mut Vec<CompositionSearchResult>,
    checked_candidates: &mut usize,
    total_candidates: usize,
    on_progress: &mut dyn FnMut(CompositionSearchProgress),
) -> Result<()> {
    if results.len() >= max_results {
        return Ok(());
    }

    if current.len() == loaded.len() {
        *checked_candidates += 1;
        on_progress(CompositionSearchProgress {
            checked_candidates: *checked_candidates,
            total_candidates,
            matches_found: results.len(),
            current_candidate: current
                .iter()
                .map(|(name, _, x, y)| (name.clone(), *x, *y))
                .collect(),
        });

        let placements = current
            .iter()
            .map(|(_, pattern, x, y)| (pattern, *x, *y))
            .collect::<Vec<_>>();
        let composed = compose_published_patterns(&placements)?;

        if let Some(PublishedSize(w, h)) = target_spec.size {
            if composed.width != w || composed.height != h {
                return Ok(());
            }
        }
        if let Some(expected_align) = target_spec.align {
            if composed.phase_alignment() != expected_align {
                return Ok(());
            }
        }

        let gadget = composed.to_gadget_pattern(format!("search:{}", target_spec.name))?;
        let report = verify_published_pattern(verifier, &composed, &gadget, target_spec)?;
        if report.is_success() {
            results.push(CompositionSearchResult {
                placements: current
                    .iter()
                    .map(|(name, _, x, y)| (name.clone(), *x, *y))
                    .collect(),
                report,
            });
            on_progress(CompositionSearchProgress {
                checked_candidates: *checked_candidates,
                total_candidates,
                matches_found: results.len(),
                current_candidate: current
                    .iter()
                    .map(|(name, _, x, y)| (name.clone(), *x, *y))
                    .collect(),
            });
        }
        return Ok(());
    }

    let (piece, pattern) = &loaded[current.len()];
    for &x in &piece.x_values {
        for &y in &piece.y_values {
            current.push((piece.spec_name.clone(), pattern.clone(), x, y));
            search_composed_pattern_positions_recursive(
                verifier,
                loaded,
                target_spec,
                max_results,
                current,
                results,
                checked_candidates,
                total_candidates,
                on_progress,
            )?;
            current.pop();
            if results.len() >= max_results {
                return Ok(());
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verifier::{GadgetVerifier, GadgetVerifierConfig};
    use rev_gol::config::SolverBackend;

    fn large_pattern_verifier() -> GadgetVerifier {
        GadgetVerifier::new(GadgetVerifierConfig {
            backend: SolverBackend::Parkissat,
            num_threads: None,
            enable_preprocessing: true,
            verbosity: 0,
            timeout: None,
        })
    }

    #[test]
    fn test_parse_published_csv_pattern() {
        let pattern = PublishedPattern::from_csv_str(
            "\
0 1 0\n\
1 2 3\n\
0 0 0\n",
        )
        .unwrap();

        assert_eq!(pattern.width, 3);
        assert_eq!(pattern.height, 3);
        assert_eq!(pattern.cells[1][1], 2);
    }

    #[test]
    fn test_relation_assignments_cover_allowed_and_forbidden() {
        let spec = PublishedSpec {
            path: "",
            name: "test",
            size: None,
            align: None,
            charging: vec![],
            relation_wires: "EW",
            relation_items: vec![vec![0, 1]],
        };

        let (allowed, forbidden) = relation_assignments(&spec);
        assert_eq!(allowed.len(), 1);
        assert_eq!(forbidden.len(), 3);
    }

    #[test]
    fn test_compose_published_patterns_merges_bounding_box_and_cells() {
        let left = PublishedPattern::from_csv_str(
            "\
10\n\
01\n",
        )
        .unwrap();
        let right = PublishedPattern::from_csv_str(
            "\
01\n\
10\n",
        )
        .unwrap();

        let composed = compose_published_patterns(&[(&left, 0, 0), (&right, 1, 1)]).unwrap();

        assert_eq!(composed.width, 3);
        assert_eq!(composed.height, 3);
        assert_eq!(composed.cells[0], vec![1, 0, 0]);
        assert_eq!(composed.cells[1], vec![0, 1, 1]);
        assert_eq!(composed.cells[2], vec![0, 1, 0]);
    }

    #[test]
    fn test_verify_imported_enforcer_when_sources_are_present() {
        let root = published_root();
        if !root.exists() {
            return;
        }

        let verifier = GadgetVerifier::new(GadgetVerifierConfig::default());
        let spec = published_basic_specs()
            .into_iter()
            .find(|spec| spec.name == "enforcer gadget")
            .unwrap();
        let report = verify_published_spec(&verifier, &root, &spec).unwrap();

        assert!(report.is_success());
    }

    #[test]
    fn test_all_published_basic_specs_verify_when_sources_are_present() {
        let root = published_root();
        if !root.exists() {
            return;
        }

        let verifier = GadgetVerifier::new(GadgetVerifierConfig::default());
        for spec in published_basic_specs() {
            let report = verify_published_spec(&verifier, &root, &spec)
                .unwrap_or_else(|err| panic!("{} failed to verify: {err}", spec.name));
            assert!(
                report.is_success(),
                "{} did not satisfy its published spec",
                spec.name
            );
        }
    }

    #[test]
    fn test_representative_published_tile_verifies_when_sources_are_present() {
        let root = published_root();
        if !root.exists() {
            return;
        }

        let verifier = large_pattern_verifier();
        let spec = published_part1_specs()
            .into_iter()
            .find(|spec| spec.name == "horizontal wire tile")
            .unwrap();
        let report = verify_published_spec(&verifier, &root, &spec)
            .unwrap_or_else(|err| panic!("{} failed to verify: {err}", spec.name));
        assert!(
            report.is_success(),
            "{} did not satisfy its published spec",
            spec.name
        );
    }

    #[test]
    #[ignore = "connector verification is available but currently too slow for the default test path"]
    fn test_representative_published_connector_verifies_when_sources_are_present() {
        let root = published_root();
        if !root.exists() {
            return;
        }

        let verifier = large_pattern_verifier();
        let spec = published_part1_specs()
            .into_iter()
            .find(|spec| spec.name == "connector 0 to 0")
            .unwrap();
        let report = verify_published_spec(&verifier, &root, &spec)
            .unwrap_or_else(|err| panic!("{} failed to verify: {err}", spec.name));
        assert!(
            report.is_success(),
            "{} did not satisfy its published spec",
            spec.name
        );
    }

    #[test]
    #[ignore = "full part-1 published verification is comprehensive and intentionally slow"]
    fn test_all_published_part1_specs_verify_when_sources_are_present() {
        let root = published_root();
        if !root.exists() {
            return;
        }

        let verifier = large_pattern_verifier();
        for spec in published_part1_specs() {
            let report = verify_published_spec(&verifier, &root, &spec)
                .unwrap_or_else(|err| panic!("{} failed to verify: {err}", spec.name));
            assert!(
                report.is_success(),
                "{} did not satisfy its published spec",
                spec.name
            );
        }
    }
}
