use std::collections::HashMap;
use std::fmt;

/// Represents a variable-width character pattern
#[derive(Debug, Clone)]
pub struct CharacterPattern {
    /// The pixel data for the character (5 rows, variable width)
    pub pixels: Vec<Vec<u8>>,
    /// The width of this character
    pub width: usize,
}

impl CharacterPattern {
    /// Create a character pattern from variable width arrays
    pub fn new(rows: &[&[u8]]) -> Self {
        assert_eq!(rows.len(), 5, "Character must have exactly 5 rows");
        assert!(!rows.is_empty(), "Must provide at least one row");

        let width = rows[0].len();
        assert!(width > 0, "Character width must be at least 1");

        let pixels = rows
            .iter()
            .map(|row| {
                assert_eq!(row.len(), width, "All rows must have the same width");
                row.to_vec()
            })
            .collect();

        Self { pixels, width }
    }
}

/// Errors that can occur when working with pixel art
#[derive(Debug, Clone, PartialEq)]
pub enum PixelArtError {
    /// Input text is too long to process
    TextTooLong(usize),
    /// Unsupported character in input
    UnsupportedCharacter(char),
}

impl fmt::Display for PixelArtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PixelArtError::TextTooLong(len) => {
                write!(f, "Text too long: {} characters (max: 1000)", len)
            }
            PixelArtError::UnsupportedCharacter(ch) => {
                write!(f, "Unsupported character: '{}'", ch)
            }
        }
    }
}

impl std::error::Error for PixelArtError {}

/// Font data structure containing variable-width character patterns
pub struct PixelFont {
    characters: HashMap<char, CharacterPattern>,
}

impl Default for PixelFont {
    fn default() -> Self {
        Self::new()
    }
}

impl PixelFont {
    /// Create a new font with basic character set
    pub fn new() -> Self {
        let mut characters = HashMap::with_capacity(100);

        characters.insert(
            'A',
            CharacterPattern::new(&[
                &[0, 1, 1, 0],
                &[1, 0, 0, 1],
                &[1, 1, 1, 1],
                &[1, 0, 0, 1],
                &[1, 0, 0, 1],
            ]),
        );

        characters.insert(
            'B',
            CharacterPattern::new(&[
                &[1, 1, 1, 0],
                &[1, 0, 0, 1],
                &[1, 1, 1, 0],
                &[1, 0, 0, 1],
                &[1, 1, 1, 0],
            ]),
        );

        characters.insert(
            'C',
            CharacterPattern::new(&[
                &[0, 1, 1, 1],
                &[1, 0, 0, 0],
                &[1, 0, 0, 0],
                &[1, 0, 0, 0],
                &[0, 1, 1, 1],
            ]),
        );

        characters.insert(
            'D',
            CharacterPattern::new(&[
                &[1, 1, 1, 0],
                &[1, 0, 0, 1],
                &[1, 0, 0, 1],
                &[1, 0, 0, 1],
                &[1, 1, 1, 0],
            ]),
        );

        characters.insert(
            'E',
            CharacterPattern::new(&[
                &[1, 1, 1, 1],
                &[1, 0, 0, 0],
                &[1, 1, 1, 0],
                &[1, 0, 0, 0],
                &[1, 1, 1, 1],
            ]),
        );

        characters.insert(
            'F',
            CharacterPattern::new(&[
                &[1, 1, 1, 1],
                &[1, 0, 0, 0],
                &[1, 1, 1, 0],
                &[1, 0, 0, 0],
                &[1, 0, 0, 0],
            ]),
        );

        characters.insert(
            'G',
            CharacterPattern::new(&[
                &[0, 1, 1, 1],
                &[1, 0, 0, 0],
                &[1, 0, 1, 1],
                &[1, 0, 0, 1],
                &[0, 1, 1, 1],
            ]),
        );

        characters.insert(
            'H',
            CharacterPattern::new(&[
                &[1, 0, 0, 1],
                &[1, 0, 0, 1],
                &[1, 1, 1, 1],
                &[1, 0, 0, 1],
                &[1, 0, 0, 1],
            ]),
        );

        characters.insert('I', CharacterPattern::new(&[&[1], &[1], &[1], &[1], &[1]]));

        characters.insert(
            'J',
            CharacterPattern::new(&[
                &[0, 0, 0, 1],
                &[0, 0, 0, 1],
                &[0, 0, 0, 1],
                &[1, 0, 0, 1],
                &[0, 1, 1, 0],
            ]),
        );

        characters.insert(
            'K',
            CharacterPattern::new(&[
                &[1, 0, 0, 1],
                &[1, 0, 1, 0],
                &[1, 1, 0, 0],
                &[1, 0, 1, 0],
                &[1, 0, 0, 1],
            ]),
        );

        characters.insert(
            'L',
            CharacterPattern::new(&[&[1, 0, 0], &[1, 0, 0], &[1, 0, 0], &[1, 0, 0], &[1, 1, 1]]),
        );

        characters.insert(
            'M',
            CharacterPattern::new(&[
                &[1, 0, 0, 0, 1],
                &[1, 1, 0, 1, 1],
                &[1, 0, 1, 0, 1],
                &[1, 0, 0, 0, 1],
                &[1, 0, 0, 0, 1],
            ]),
        );

        characters.insert(
            'N',
            CharacterPattern::new(&[
                &[1, 0, 0, 1],
                &[1, 1, 0, 1],
                &[1, 0, 1, 1],
                &[1, 0, 0, 1],
                &[1, 0, 0, 1],
            ]),
        );

        characters.insert(
            'O',
            CharacterPattern::new(&[
                &[0, 1, 1, 0],
                &[1, 0, 0, 1],
                &[1, 0, 0, 1],
                &[1, 0, 0, 1],
                &[0, 1, 1, 0],
            ]),
        );

        characters.insert(
            'P',
            CharacterPattern::new(&[
                &[1, 1, 1, 0],
                &[1, 0, 0, 1],
                &[1, 1, 1, 0],
                &[1, 0, 0, 0],
                &[1, 0, 0, 0],
            ]),
        );

        characters.insert(
            'Q',
            CharacterPattern::new(&[
                &[0, 1, 1, 0],
                &[1, 0, 0, 1],
                &[1, 0, 0, 1],
                &[1, 0, 1, 1],
                &[0, 1, 1, 1],
            ]),
        );

        characters.insert(
            'R',
            CharacterPattern::new(&[
                &[1, 1, 1, 0],
                &[1, 0, 0, 1],
                &[1, 1, 1, 0],
                &[1, 0, 1, 0],
                &[1, 0, 0, 1],
            ]),
        );

        characters.insert(
            'S',
            CharacterPattern::new(&[
                &[0, 1, 1, 1],
                &[1, 0, 0, 0],
                &[0, 1, 1, 0],
                &[0, 0, 0, 1],
                &[1, 1, 1, 0],
            ]),
        );

        characters.insert(
            'T',
            CharacterPattern::new(&[&[1, 1, 1], &[0, 1, 0], &[0, 1, 0], &[0, 1, 0], &[0, 1, 0]]),
        );

        characters.insert(
            'U',
            CharacterPattern::new(&[
                &[1, 0, 0, 1],
                &[1, 0, 0, 1],
                &[1, 0, 0, 1],
                &[1, 0, 0, 1],
                &[1, 1, 1, 1],
            ]),
        );

        characters.insert(
            'V',
            CharacterPattern::new(&[&[1, 0, 1], &[1, 0, 1], &[1, 0, 1], &[1, 0, 1], &[0, 1, 0]]),
        );

        characters.insert(
            'W',
            CharacterPattern::new(&[
                &[1, 0, 0, 0, 1],
                &[1, 0, 0, 0, 1],
                &[1, 0, 0, 0, 1],
                &[1, 0, 1, 0, 1],
                &[0, 1, 0, 1, 0],
            ]),
        );

        characters.insert(
            'X',
            CharacterPattern::new(&[&[1, 0, 1], &[1, 0, 1], &[0, 1, 0], &[1, 0, 1], &[1, 0, 1]]),
        );

        characters.insert(
            'Y',
            CharacterPattern::new(&[&[1, 0, 1], &[1, 0, 1], &[1, 1, 1], &[0, 1, 0], &[0, 1, 0]]),
        );

        characters.insert(
            'Z',
            CharacterPattern::new(&[
                &[1, 1, 1, 1],
                &[0, 0, 0, 1],
                &[0, 0, 1, 0],
                &[0, 1, 0, 0],
                &[1, 1, 1, 1],
            ]),
        );

        characters.insert(
            'a',
            CharacterPattern::new(&[&[0, 0, 0], &[0, 1, 1], &[1, 0, 1], &[1, 0, 1], &[0, 1, 1]]),
        );

        characters.insert(
            'b',
            CharacterPattern::new(&[&[1, 0, 0], &[1, 0, 0], &[1, 1, 0], &[1, 0, 1], &[1, 1, 0]]),
        );

        characters.insert(
            'c',
            CharacterPattern::new(&[&[0, 0, 0], &[0, 1, 1], &[1, 0, 0], &[1, 0, 0], &[0, 1, 1]]),
        );

        characters.insert(
            'd',
            CharacterPattern::new(&[&[0, 0, 1], &[0, 0, 1], &[0, 1, 1], &[1, 0, 1], &[0, 1, 1]]),
        );

        characters.insert(
            'e',
            CharacterPattern::new(&[&[0, 0, 0], &[0, 1, 1], &[1, 0, 1], &[1, 1, 0], &[0, 1, 1]]),
        );

        characters.insert(
            'f',
            CharacterPattern::new(&[&[0, 1, 1], &[0, 1, 0], &[1, 1, 1], &[0, 1, 0], &[0, 1, 0]]),
        );

        characters.insert(
            'g',
            CharacterPattern::new(&[&[0, 1, 1], &[1, 0, 1], &[0, 1, 1], &[0, 0, 1], &[1, 1, 0]]),
        );

        characters.insert(
            'h',
            CharacterPattern::new(&[&[1, 0, 0], &[1, 0, 0], &[1, 1, 0], &[1, 0, 1], &[1, 0, 1]]),
        );

        characters.insert('i', CharacterPattern::new(&[&[1], &[0], &[1], &[1], &[1]]));

        characters.insert(
            'j',
            CharacterPattern::new(&[&[0, 1], &[0, 0], &[0, 1], &[0, 1], &[1, 0]]),
        );

        characters.insert(
            'k',
            CharacterPattern::new(&[&[1, 0, 0], &[1, 0, 0], &[1, 0, 1], &[1, 1, 0], &[1, 0, 1]]),
        );

        characters.insert(
            'l',
            CharacterPattern::new(&[&[1, 0], &[1, 0], &[1, 0], &[1, 0], &[1, 1]]),
        );

        characters.insert(
            'm',
            CharacterPattern::new(&[
                &[0, 0, 0, 0, 0],
                &[1, 1, 0, 1, 1],
                &[1, 0, 1, 0, 1],
                &[1, 0, 0, 0, 1],
                &[1, 0, 0, 0, 1],
            ]),
        );

        characters.insert(
            'n',
            CharacterPattern::new(&[
                &[0, 0, 0, 0],
                &[1, 1, 1, 0],
                &[1, 0, 0, 1],
                &[1, 0, 0, 1],
                &[1, 0, 0, 1],
            ]),
        );

        characters.insert(
            'o',
            CharacterPattern::new(&[
                &[0, 0, 0, 0],
                &[0, 1, 1, 0],
                &[1, 0, 0, 1],
                &[1, 0, 0, 1],
                &[0, 1, 1, 0],
            ]),
        );

        characters.insert(
            'p',
            CharacterPattern::new(&[&[0, 0, 0], &[1, 1, 0], &[1, 0, 1], &[1, 1, 0], &[1, 0, 0]]),
        );

        characters.insert(
            'q',
            CharacterPattern::new(&[&[0, 0, 0], &[0, 1, 1], &[1, 0, 1], &[0, 1, 1], &[0, 0, 1]]),
        );

        characters.insert(
            'r',
            CharacterPattern::new(&[&[0, 0, 0], &[1, 0, 1], &[1, 1, 0], &[1, 0, 0], &[1, 0, 0]]),
        );

        characters.insert(
            's',
            CharacterPattern::new(&[&[0, 0, 0], &[0, 1, 1], &[1, 1, 0], &[0, 0, 1], &[1, 1, 0]]),
        );

        characters.insert(
            't',
            CharacterPattern::new(&[&[0, 1, 0], &[1, 1, 1], &[0, 1, 0], &[0, 1, 0], &[0, 1, 1]]),
        );

        characters.insert(
            'u',
            CharacterPattern::new(&[&[0, 0, 0], &[1, 0, 1], &[1, 0, 1], &[1, 0, 1], &[0, 1, 1]]),
        );

        characters.insert(
            'v',
            CharacterPattern::new(&[&[0, 0, 0], &[1, 0, 1], &[1, 0, 1], &[1, 0, 1], &[0, 1, 0]]),
        );

        characters.insert(
            'w',
            CharacterPattern::new(&[
                &[0, 0, 0, 0, 0],
                &[1, 0, 0, 0, 1],
                &[1, 0, 0, 0, 1],
                &[1, 0, 1, 0, 1],
                &[0, 1, 0, 1, 0],
            ]),
        );

        characters.insert(
            'x',
            CharacterPattern::new(&[&[0, 0, 0], &[1, 0, 1], &[0, 1, 0], &[1, 0, 1], &[1, 0, 1]]),
        );

        characters.insert(
            'y',
            CharacterPattern::new(&[&[0, 0, 0], &[1, 0, 1], &[1, 0, 1], &[0, 1, 0], &[1, 0, 0]]),
        );

        characters.insert(
            'z',
            CharacterPattern::new(&[&[0, 0, 0], &[1, 1, 1], &[0, 0, 1], &[0, 1, 0], &[1, 1, 1]]),
        );

        characters.insert(
            '0',
            CharacterPattern::new(&[
                &[0, 1, 1, 0],
                &[1, 0, 0, 1],
                &[1, 0, 0, 1],
                &[1, 0, 0, 1],
                &[0, 1, 1, 0],
            ]),
        );

        characters.insert(
            '1',
            CharacterPattern::new(&[&[0, 1, 0], &[1, 1, 0], &[0, 1, 0], &[0, 1, 0], &[1, 1, 1]]),
        );

        characters.insert(
            '2',
            CharacterPattern::new(&[
                &[0, 1, 1, 0],
                &[1, 0, 0, 1],
                &[0, 0, 1, 0],
                &[0, 1, 0, 0],
                &[1, 1, 1, 1],
            ]),
        );

        characters.insert(
            '3',
            CharacterPattern::new(&[
                &[0, 1, 1, 0],
                &[1, 0, 0, 1],
                &[0, 0, 1, 0],
                &[1, 0, 0, 1],
                &[0, 1, 1, 0],
            ]),
        );

        characters.insert(
            '4',
            CharacterPattern::new(&[
                &[0, 1, 1, 0],
                &[1, 0, 1, 0],
                &[1, 1, 1, 1],
                &[0, 0, 1, 0],
                &[0, 0, 1, 0],
            ]),
        );

        characters.insert(
            '5',
            CharacterPattern::new(&[
                &[1, 1, 1, 1],
                &[1, 0, 0, 0],
                &[1, 1, 1, 0],
                &[0, 0, 0, 1],
                &[1, 1, 1, 0],
            ]),
        );

        characters.insert(
            '6',
            CharacterPattern::new(&[
                &[0, 1, 1, 0],
                &[1, 0, 0, 0],
                &[1, 1, 1, 0],
                &[1, 0, 0, 1],
                &[0, 1, 1, 0],
            ]),
        );

        characters.insert(
            '7',
            CharacterPattern::new(&[
                &[1, 1, 1, 1],
                &[0, 0, 0, 1],
                &[0, 0, 1, 0],
                &[0, 1, 0, 0],
                &[0, 1, 0, 0],
            ]),
        );

        characters.insert(
            '8',
            CharacterPattern::new(&[
                &[0, 1, 1, 0],
                &[1, 0, 0, 1],
                &[0, 1, 1, 0],
                &[1, 0, 0, 1],
                &[0, 1, 1, 0],
            ]),
        );

        characters.insert(
            '9',
            CharacterPattern::new(&[
                &[0, 1, 1, 0],
                &[1, 0, 0, 1],
                &[0, 1, 1, 1],
                &[0, 0, 0, 1],
                &[0, 1, 1, 0],
            ]),
        );

        // Add symbols
        characters.insert(
            '@',
            CharacterPattern::new(&[
                &[0, 1, 1, 0],
                &[1, 0, 0, 1],
                &[1, 0, 1, 1],
                &[1, 0, 0, 0],
                &[0, 1, 1, 1],
            ]),
        );

        characters.insert('!', CharacterPattern::new(&[&[1], &[1], &[1], &[0], &[1]]));

        characters.insert(
            '#',
            CharacterPattern::new(&[
                &[0, 1, 0, 1, 0],
                &[1, 1, 1, 1, 1],
                &[0, 1, 0, 1, 0],
                &[1, 1, 1, 1, 1],
                &[0, 1, 0, 1, 0],
            ]),
        );

        characters.insert(
            '$',
            CharacterPattern::new(&[
                &[0, 1, 1, 1, 0],
                &[1, 0, 1, 0, 0],
                &[0, 1, 1, 1, 0],
                &[0, 0, 1, 0, 1],
                &[0, 1, 1, 1, 0],
            ]),
        );

        characters.insert(
            '%',
            CharacterPattern::new(&[
                &[1, 1, 0, 0, 1],
                &[1, 1, 0, 1, 0],
                &[0, 0, 1, 0, 0],
                &[0, 1, 0, 1, 1],
                &[1, 0, 0, 1, 1],
            ]),
        );

        characters.insert(
            '^',
            CharacterPattern::new(&[&[0, 1, 0], &[1, 0, 1], &[0, 0, 0], &[0, 0, 0], &[0, 0, 0]]),
        );

        characters.insert(
            '&',
            CharacterPattern::new(&[
                &[0, 1, 0, 0, 0],
                &[1, 0, 1, 0, 0],
                &[0, 1, 0, 1, 0],
                &[1, 0, 1, 0, 0],
                &[0, 1, 0, 1, 0],
            ]),
        );

        characters.insert(
            '*',
            CharacterPattern::new(&[&[1, 0, 1], &[0, 1, 0], &[1, 0, 1], &[0, 0, 0], &[0, 0, 0]]),
        );

        characters.insert(
            '(',
            CharacterPattern::new(&[&[0, 1], &[1, 0], &[1, 0], &[1, 0], &[0, 1]]),
        );

        characters.insert(
            ')',
            CharacterPattern::new(&[&[1, 0], &[0, 1], &[0, 1], &[0, 1], &[1, 0]]),
        );

        characters.insert(
            '-',
            CharacterPattern::new(&[&[0, 0, 0], &[0, 0, 0], &[1, 1, 1], &[0, 0, 0], &[0, 0, 0]]),
        );

        characters.insert(
            '_',
            CharacterPattern::new(&[&[0, 0, 0], &[0, 0, 0], &[0, 0, 0], &[0, 0, 0], &[1, 1, 1]]),
        );

        characters.insert(
            '=',
            CharacterPattern::new(&[&[0, 0, 0], &[1, 1, 1], &[0, 0, 0], &[1, 1, 1], &[0, 0, 0]]),
        );

        characters.insert(
            '+',
            CharacterPattern::new(&[&[0, 0, 0], &[0, 1, 0], &[1, 1, 1], &[0, 1, 0], &[0, 0, 0]]),
        );

        characters.insert(
            '?',
            CharacterPattern::new(&[
                &[0, 1, 1, 0],
                &[1, 0, 0, 1],
                &[0, 0, 1, 0],
                &[0, 0, 0, 0],
                &[0, 0, 1, 0],
            ]),
        );

        characters.insert('.', CharacterPattern::new(&[&[0], &[0], &[0], &[0], &[1]]));

        characters.insert(
            '/',
            CharacterPattern::new(&[
                &[0, 0, 0, 0, 1],
                &[0, 0, 0, 1, 0],
                &[0, 0, 1, 0, 0],
                &[0, 1, 0, 0, 0],
                &[1, 0, 0, 0, 0],
            ]),
        );

        characters.insert('|', CharacterPattern::new(&[&[1], &[1], &[1], &[1], &[1]]));

        characters.insert(':', CharacterPattern::new(&[&[0], &[1], &[0], &[1], &[0]]));

        characters.insert(
            ';',
            CharacterPattern::new(&[&[0, 0], &[0, 1], &[0, 0], &[0, 1], &[1, 1]]),
        );

        characters.insert(
            ',',
            CharacterPattern::new(&[&[0, 0], &[0, 0], &[0, 0], &[0, 1], &[1, 1]]),
        );

        characters.insert(
            '<',
            CharacterPattern::new(&[&[0, 0, 1], &[0, 1, 0], &[1, 0, 0], &[0, 1, 0], &[0, 0, 1]]),
        );

        characters.insert(
            '>',
            CharacterPattern::new(&[&[1, 0, 0], &[0, 1, 0], &[0, 0, 1], &[0, 1, 0], &[1, 0, 0]]),
        );

        characters.insert(
            '[',
            CharacterPattern::new(&[&[1, 1], &[1, 0], &[1, 0], &[1, 0], &[1, 1]]),
        );

        characters.insert(
            ']',
            CharacterPattern::new(&[&[1, 1], &[0, 1], &[0, 1], &[0, 1], &[1, 1]]),
        );

        characters.insert(
            '{',
            CharacterPattern::new(&[&[0, 1, 1], &[0, 1, 0], &[1, 0, 0], &[0, 1, 0], &[0, 1, 1]]),
        );

        characters.insert(
            '}',
            CharacterPattern::new(&[&[1, 1, 0], &[0, 1, 0], &[0, 0, 1], &[0, 1, 0], &[1, 1, 0]]),
        );

        characters.insert(
            '~',
            CharacterPattern::new(&[&[0, 0, 0], &[0, 1, 1], &[1, 1, 0], &[0, 0, 0], &[0, 0, 0]]),
        );

        characters.insert(
            '"',
            CharacterPattern::new(&[&[1, 0, 1], &[1, 0, 1], &[0, 0, 0], &[0, 0, 0], &[0, 0, 0]]),
        );

        characters.insert(
            '\'',
            CharacterPattern::new(&[
                &[1, 0, 0, 0, 0],
                &[0, 1, 0, 0, 0],
                &[0, 0, 1, 0, 0],
                &[0, 0, 0, 1, 0],
                &[0, 0, 0, 0, 1],
            ]),
        );

        characters.insert(
            '`',
            CharacterPattern::new(&[&[1, 0], &[0, 1], &[0, 0], &[0, 0], &[0, 0]]),
        );

        PixelFont { characters }
    }

    /// Get the pattern for a specific character
    pub fn get_pattern(&self, ch: char) -> Option<&CharacterPattern> {
        self.characters.get(&ch)
    }

    /// Get all supported characters
    pub fn supported_characters(&self) -> Vec<char> {
        let mut chars: Vec<char> = self.characters.keys().cloned().collect();
        chars.sort();
        chars
    }
}

/// Validates that all characters in the input text are supported
fn validate_text(text: &str, font: &PixelFont) -> Result<(), PixelArtError> {
    for ch in text.chars() {
        if ch == ' ' {
            continue; // Space is handled specially
        }
        if !font.characters.contains_key(&ch) {
            return Err(PixelArtError::UnsupportedCharacter(ch));
        }
    }
    Ok(())
}

/// Convert text to pixel art representation
pub fn text_to_pixel_art(text: &str) -> Result<String, PixelArtError> {
    if text.is_empty() {
        return Ok(String::new());
    }

    let font = PixelFont::new();
    validate_text(text, &font)?;

    let chars: Vec<char> = text.chars().collect();

    // Calculate total width needed
    let mut content_width = 0;
    for (i, &ch) in chars.iter().enumerate() {
        if ch == ' ' {
            content_width += 2; // Space width
        } else if let Some(pattern) = font.get_pattern(ch) {
            content_width += pattern.width;
        }

        // Add spacing between characters (except after the last character)
        if i < chars.len() - 1 {
            content_width += 1;
        }
    }

    // Add padding: 1 pixel on each side horizontally, 1 pixel on top and bottom vertically
    let total_width = content_width + 2;
    let total_height = 7; // 5 rows for characters + 1 row padding top + 1 row padding bottom

    // Pre-allocate the result grid with padding
    let mut result = vec![vec![0u8; total_width]; total_height];
    let mut current_x = 1; // Start at x=1 to account for left padding

    for (i, &ch) in chars.iter().enumerate() {
        if ch == ' ' {
            current_x += 2;
        } else if let Some(pattern) = font.get_pattern(ch) {
            // Copy character pattern to result (offset by 1 row for top padding)
            for (row_idx, row) in pattern.pixels.iter().enumerate() {
                for (col_idx, &pixel) in row.iter().enumerate() {
                    result[row_idx + 1][current_x + col_idx] = pixel;
                }
            }
            current_x += pattern.width;
        }

        // Add spacing between characters (except after the last character)
        if i < chars.len() - 1 {
            current_x += 1; // Single column spacing
        }
    }

    // Convert to string representation
    let mut output = String::with_capacity(total_width * (total_height + 1)); // total_height rows + newlines
    for row in result {
        for pixel in row {
            output.push(if pixel == 1 { '1' } else { '0' });
        }
        output.push('\n');
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_character_pattern_creation() {
        let pattern =
            CharacterPattern::new(&[&[1, 0, 1], &[0, 1, 0], &[1, 0, 1], &[0, 1, 0], &[1, 0, 1]]);

        assert_eq!(pattern.width, 3);
        assert_eq!(pattern.pixels.len(), 5);
        assert_eq!(pattern.pixels[0], vec![1, 0, 1]);
    }

    #[test]
    fn test_font_creation() {
        let font = PixelFont::new();

        // Test that basic characters exist
        assert!(font.get_pattern('A').is_some());
        assert!(font.get_pattern('a').is_some());
        assert!(font.get_pattern('0').is_some());
        assert!(font.get_pattern('@').is_some());

        // Test that unsupported characters don't exist
        assert!(font.get_pattern('ñ').is_none());
    }

    #[test]
    fn test_empty_string() {
        let result = text_to_pixel_art("").unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_unsupported_character() {
        let result = text_to_pixel_art("ñ");
        assert!(matches!(
            result,
            Err(PixelArtError::UnsupportedCharacter('ñ'))
        ));
    }

    #[test]
    fn test_supported_characters() {
        let font = PixelFont::new();
        let supported = font.supported_characters();

        // Should include all letters, numbers, and symbols
        assert!(supported.contains(&'A'));
        assert!(supported.contains(&'a'));
        assert!(supported.contains(&'0'));
        assert!(supported.contains(&'@'));
        assert!(supported.contains(&'!'));

        // Should be sorted
        let mut sorted_supported = supported.clone();
        sorted_supported.sort();
        assert_eq!(supported, sorted_supported);
    }

    #[test]
    fn test_all_uppercase_letters() {
        for ch in 'A'..='Z' {
            let result = text_to_pixel_art(&ch.to_string());
            assert!(result.is_ok(), "Failed to convert character: {}", ch);
        }
    }

    #[test]
    fn test_all_lowercase_letters() {
        for ch in 'a'..='z' {
            let result = text_to_pixel_art(&ch.to_string());
            assert!(result.is_ok(), "Failed to convert character: {}", ch);
        }
    }

    #[test]
    fn test_all_numbers() {
        for ch in '0'..='9' {
            let result = text_to_pixel_art(&ch.to_string());
            assert!(result.is_ok(), "Failed to convert number: {}", ch);
        }
    }

    #[test]
    fn test_all_symbols() {
        let symbols = "@#$%^&*()-_=+?./|:;,<>[]{}~\"'`!";
        for ch in symbols.chars() {
            let result = text_to_pixel_art(&ch.to_string());
            assert!(result.is_ok(), "Failed to convert symbol: {}", ch);
        }
    }

    #[test]
    fn test_long_text() {
        let result = text_to_pixel_art("Hello World!");
        assert!(result.is_ok());

        let output = result.unwrap();
        let lines: Vec<&str> = output.trim().split('\n').collect();
        assert_eq!(lines.len(), 7);

        // Each line should have the same length
        let first_line_len = lines[0].len();
        for line in &lines {
            assert_eq!(line.len(), first_line_len);
        }
    }
}
