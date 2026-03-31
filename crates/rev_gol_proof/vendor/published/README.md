# gol-backward-comp
Auxiliary files for article "Computing backwards with Game of Life"

## Gadgets and verification

The script `verify_pt1.py` can be run to verify all gadgets from the article.
Run `python verify_pt1.py -h` to see its options.
The gadgets are contained in the subfolders as plain text files and/or Golly-compatible patterns.

## Jeandel-Rao tile set

The file `jeander-rao.pat` contains a Golly-compatible 6210x37800 pattern that, when repeated periodically, has a preimage but no periodic preimage.
A proof of this property can be found in the article.