# text_to_input

A Rust crate that converts text input into pixel art using 1s and 0s with a variable-width font system.

## Features

- Converts text to pixel art representation using 1s and 0s
- **Variable-width font** - characters can be 1-5 pixels wide for optimal spacing
- Automatic buffer rows of 0s on top, bottom, left, and right
- Single column spacing between characters for readability
- Supports A-Z uppercase, a-z lowercase, and space characters
- Case-sensitive rendering (preserves upper/lower case distinction)
- Proper error handling with custom error types
- Performance optimized with pre-allocated collections
- No `unwrap()` calls - all errors are handled gracefully

## Variable Width Font

The font system uses variable character widths for optimal space usage:

- **Narrow characters** (1-3 pixels): `i`, `l`, `j`, `f`, `r`, `t`, `I`
- **Medium characters** (4 pixels): Most lowercase letters
- **Wide characters** (5 pixels): Most uppercase letters, `m`, `w`

This results in much more compact and readable output compared to fixed-width fonts.

## Usage

### As a binary

```bash
cargo run -p text_to_input
```

Then enter your text when prompted. The program will display helpful error messages for unsupported characters or text that's too long.

### As a library

```rust
use text_to_input::{text_to_pixel_art, PixelArtError};

// Basic usage with error handling
match text_to_pixel_art("Hello World") {
    Ok(pixel_art) => {
        for line in pixel_art {
            println!("{}", line);
        }
    }
    Err(PixelArtError::CharacterNotFound(ch)) => {
        eprintln!("Character '{}' not supported", ch);
    }
    Err(PixelArtError::TextTooLong(len)) => {
        eprintln!("Text too long: {} characters", len);
    }
}

// Validate text before conversion
use text_to_input::validate_text;

if let Err(e) = validate_text("Hello!") {
    println!("Validation error: {}", e);
}

// Check font capabilities
use text_to_input::PixelFont;

let font = PixelFont::new();
if font.supports_char('A') {
    println!("Character 'A' is supported");
}
```

## Example Output

Input: "Hello World"

```
00000000000000000000000000000000000000000000000
01000100000010100000000001000100000000001000010
01000100110010100110000001000100110010101000010
01111101111010101001000001010101001011001001110
01000101000010101001000001101101001010001010010
01000100110010100110000001000100110010001001110
00000000000000000000000000000000000000000000000
```

Input: "ill" (demonstrating narrow characters)

```
0000000
0101010
0001010
0101010
0101010
0101010
0000000
```

## Font Customization

The font patterns are defined in the `PixelFont` struct using the `CharacterPattern::new()` method. Each character can have a different width (1-5 pixels) and is represented as 5 rows of variable-width arrays.

Example of adding a custom character:

```rust
// 2-pixel wide character
characters.insert('!', CharacterPattern::new(&[
    &[0, 1],
    &[0, 1],
    &[0, 1],
    &[0, 0],
    &[0, 1],
]));
```

## API Reference

### Functions

- `text_to_pixel_art(text: &str) -> Result<Vec<String>, PixelArtError>` - Convert text to pixel art
- `text_to_pixel_art_lossy(text: &str) -> Result<Vec<String>, PixelArtError>` - Convert text, using spaces for unsupported chars
- `validate_text(text: &str) -> Result<(), PixelArtError>` - Validate that all characters are supported

### Types

- `PixelFont` - Font data structure with variable-width character patterns
- `CharacterPattern` - Variable-width character representation with pixel data and width
- `PixelArtError` - Error type for conversion failures

## Building

```bash
cargo build -p text_to_input --release
```

## Testing

```bash
cargo test -p text_to_input
```
