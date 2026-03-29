# rusted-chain

A real-time guitar pedalboard written in Rust, with a terminal UI built with [ratatui](https://github.com/ratatui-org/ratatui).

Plug in your guitar, pick a preset, and tweak effects live from your terminal.

## Requirements

- Rust (stable)
- A working audio input/output device
- PipeWire or ALSA

## Run

```bash
cargo run --release
```

## Effects

- Distortion (soft/hard)
- Bitcrusher
- Delay (with ping pong)
- Chorus
- Compressor
- Reverb
- Tremolo

## Presets

Presets are stored as JSON files in `~/.rusted-chain/`. They can be saved and loaded directly from the UI.

## Keybindings

| Key | Action |
|-----|--------|
| `Tab` | Switch panel |
| `↑ / ↓` | Navigate effects / parameters |
| `Enter` | Grab / release parameter |
| `← / →` | Adjust parameter value |
| `A` | Add effect |
| `D` | Delete effect |
| `S` | Save preset |
| `L` | Load preset |
| `PgUp / PgDn` | Adjust volume |
| `Q` or `Crtl+C` | Quit |
