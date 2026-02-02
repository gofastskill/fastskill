# Images Directory

This directory contains images and diagrams for the FastSkill documentation.

## Files

- `hero-diagram.svg` - FastSkill architecture diagram (SVG format for optimal web display)

## Converting SVG to PNG

If you need a PNG version of the hero diagram, you can convert it using:

```bash
# Using ImageMagick
convert hero-diagram.svg hero-diagram.png

# Using Inkscape (command line)
inkscape hero-diagram.svg --export-filename=hero-diagram.png

# Using rsvg-convert
rsvg-convert -h 500 hero-diagram.svg -o hero-diagram.png
```

The SVG format is recommended for documentation as it:

- Scales perfectly at any size
- Has smaller file sizes
- Supports better accessibility
- Works well with Mintlify's responsive design
