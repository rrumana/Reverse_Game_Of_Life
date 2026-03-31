use std::io::{self, Write};
use text_to_input::{text_to_pixel_art, PixelArtError};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    print!("Enter your text input: ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let text = input.trim();

    match text_to_pixel_art(text) {
        Ok(pixel_art) => {
            println!("\noutput:");
            for line in pixel_art.lines() {
                println!("{}", line);
            }
        }
        Err(PixelArtError::UnsupportedCharacter(ch)) => {
            eprintln!("Error: Character '{}' is not supported by the font.", ch);
            eprintln!("Supported characters: A-Z, a-z, 0-9, and various symbols");
            std::process::exit(1);
        }
        Err(PixelArtError::TextTooLong(len)) => {
            eprintln!(
                "Error: Text is too long ({} characters). Maximum length is 1000 characters.",
                len
            );
            std::process::exit(1);
        }
    }

    Ok(())
}
