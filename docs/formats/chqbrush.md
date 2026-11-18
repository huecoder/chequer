# The `.chqbrush` format

A `.chqbrush` file stores one or more brushes: their tips, paper and bristle textures, and settings. Chequer saves its brush library in this format, and a brush can be shared as a single file. This page describes the file in full, so other programs can read and write brushes too. The current version is 1.3.

## Container

All numbers are little-endian.

| Offset | Bytes | Field |
|---|---|---|
| 0 | 4 | magic `CHB\0` |
| 4 | 2 | major version (1). A newer major is an error. |
| 6 | 2 | minor version |
| 8 | | blocks until the end of the file |

A block:

| Offset | Bytes | Field |
|---|---|---|
| 0 | 4 | ASCII tag |
| 4 | 1 | 1 required, 0 optional |
| 5 | 4 | data length N, at most 256 MiB |
| 9 | N | data |
| 9+N | 4 | CRC-32 (zlib, PNG) of the first 9+N bytes of the block |

An optional block with an unknown tag is skipped. An unknown required block is a load error.

## Brushes in a file

The file is a sequence of groups. A group starts with a `META` block and lasts until the next `META`. Blocks before the first `META` are ignored. A group is `META`, `PRST`, then `TIPM` blocks, optional `GRAN` and `HAIR`, and one required `LAYR` for each track layer.

### META (JSON)

All fields are optional, unknown fields are ignored.

| Field | Type | Meaning |
|---|---|---|
| `id` | string | Key of the brush (favorites, seeding). If empty, built from the name. |
| `name` | string | Default name. |
| `names` | object | Language to name, `ru` and `en`. |
| `author`, `license` | strings | |
| `category` | string | Group in the list: `basic`, `paint`, `soft`, `texture`, `decor`, `drawing`, `painting` are translated by the interface. Empty means *My brushes*. Other values are shown as they are. |
| `tags` | array of strings | |

### PRST (JSON)

```json
{ "tip": "set", "weights": [1.0, 0.4], "settings": { "spacing": 0.1, "flow": 0.5 } }
```

- `tip`: `round` has no `TIPM` blocks. `mask` has one. `set` has several, the stamps pick one at random in proportion to its weight. `generated` builds the tip from a description and has no `TIPM` blocks.
- `generated` (only with `tip: generated`): `{ "generator": "split", "params": { "gap": 0.45 }, "seed": 0 }`. The mask is generated again on load (side 256), so the file stays small. An unknown generator is an error. Unknown keys in `params` are ignored and values out of range are clamped.
- `weights` (only with `set`): weight of each `TIPM` in order, 0.01 to 1. Missing means 1.
- `paper`, `hairs` (optional): how the grain texture or the bristle pattern was generated (`kind`: `noise`, `fibers`, `dots`, `cells`, `weave`, `bristles`; `scale`, `density`, `contrast` 0 to 1; `seed`). The brush dialog needs them to continue editing. The pixels come from `GRAN` and `HAIR`.
- `layers` (ribbon brushes only, at most 8): the track layers in order. The number must match the number of `LAYR` blocks. Layer fields:

| Field | Meaning |
|---|---|
| `pitch` | Length of one tile along the stroke, in brush sizes, 0.02 to 20. Default 1. |
| `pitches` | Up to 16 multipliers from 0.2 to 5. Each next repeat of the tile is stretched by its multiplier. The list loops. |
| `amount` | Layer strength, 0 to 1. Default 1. |
| `marks` | Optional list of up to 8 marks. If present, the layer image is only a baked result and is rebuilt from the marks when read. |

A mark: `start`, `length`, `offset`, `thickness`, `rounding`, `sway`, `sway_cycles`. Lengths are in brush sizes (the stroke width is 1): position and length along the stroke inside the `pitch` period, shift and thickness across, rounding from 0 (sharp corners) to 1 (round ends), the swing and the number of waves per mark. One mark per period gives dashes or dots, two give dash-dot, a mark as long as the period with a swing gives a wave. Layers multiply: a pixel of 255 lets paint through, 0 holds it back.

- `settings`: the parameters from the table below. Missing ones take the default, out-of-range ones are clamped on load.

### TIPM (PNG)

An 8-bit grayscale image (any PNG is converted to gray on load). A pixel is coverage: 255 full paint, 0 nothing. Side from 1 to 4096. The order of blocks is the order of variants in `set`.

### GRAN, HAIR (PNG, optional)

8-bit grayscale, side 1 to 1024. Without them a built-in texture is used, so files of format 1.0 still load, and older readers skip these blocks.

- `GRAN` is the paper for the grain, tiled. Light is a bump (paint lands), dark is a pit (paint skips). The tile size comes from `grain_scale` and the image size.
- `HAIR` is the bristle pattern of a ribbon brush: one row, left to right across the brush (for a taller image the middle row is used). The value is the paint load of a bristle. Lower values drop out first when `dryness` grows.

### LAYR (PNG, required, one per track layer)

8-bit grayscale, side 1 to 1024, in the order of `layers` in `PRST`. Across the stroke the image is stretched over the whole ribbon (left edge is the left side in the direction of travel). Along the stroke it is a tile: the top of the image is the direction of travel, and the bottom edge connects to the top. A one-row image does not change along the stroke (a profile of a tire edge or a stroke edge). The tile length is `pitch` times the brush size.

## Tip generators

The mask is built in a square from -1 to 1, with coverage 0 to 1. The same `seed` gives the same result. Each parameter has its own range, and values outside are clamped.

| `generator` | What it draws | Parameters (default, range) | `seed` |
|---|---|---|---|
| `bristles` | Scattered round bristles with different paint load | `count` (90; 1-400), `radius` (0.03; 0.005-0.3) | placement and load |
| `comb` | A row of parallel teeth | `teeth` (7; 2-24), `fill` (0.55; 0.1-0.95), `reach` (0.9; 0.2-1) | not used |
| `split` | Two narrow points on one axis (a split nib) | `gap` (0.45; 0.1-0.9), `width` (0.09; 0.02-0.4), `reach` (0.9; 0.3-1) | not used |
| `snowflake` | A snowflake grown with the Reiter crystal model on a hexagonal grid | `vapor` (0.4; 0.25-0.95), `diffusion` (1; 0.4-2), `sticking` (1; 0.01-10), `unevenness` (0.1; 0-0.5), `detail` (64; 30-140, cost grows with the cube) | irregularities and branching |
| `frost` | Feathery frost, branches with branches | `branches` (14; 4-24), `depth` (2; 1-4), `spread` (0.95; 0.4-1.4), `curl` (0.3; 0-1) | bends and branch lengths |
| `noise` | A thresholded noise blot (sponge, moss) | `frequency` (6; 1-24), `threshold` (0.45; 0-0.9), `contrast` (8; 1-20), `reach` (0.95; 0.3-1) | noise pattern |

## `settings`

| Field | In the dialog | Default | Range |
|---|---|---|---|
| `engine` | Brush type: `stamp` or `ribbon` | `stamp` | |
| `spacing` | Spacing | 0.1 | 0.01-4 |
| `angle` | Angle (degrees) | 0 | modulo 360 |
| `follow_direction` | Rotate along the stroke | false | |
| `angle_jitter` | Angle jitter | 0 | 0-1 |
| `size_jitter` | Size jitter | 0 | 0-1 |
| `opacity_jitter` | Opacity jitter | 0 | 0-1 |
| `scatter` | Scatter | 0 | 0-5 |
| `roundness` | Roundness | 1 | 0.05-1 |
| `flow` | Flow | 1 | 0.01-1 |
| `accumulate` | Build up paint on overlap | false | |
| `pressure_size` | Pressure affects size | 1 | 0-1 |
| `pressure_size_curve` | Pressure to size curve | linear | curve |
| `pressure_opacity` | Pressure affects opacity | 0 | 0-1 |
| `pressure_opacity_curve` | Pressure to opacity curve | linear | curve |
| `pressure_roundness` | Pressure affects roundness | 0 | 0-1 |
| `pressure_scatter` | Light pressure scatters | 0 | 0-1 |
| `pressure_hardness` | Pressure affects edge hardness | 0 | 0-1 |
| `pressure_spacing` | Light pressure widens spacing | 0 | 0-1 |
| `tilt_angle` | Tilt rotates the tip | 0 | 0-1 |
| `tilt_roundness` | Tilt flattens the tip | 0 | 0-1 |
| `velocity_size` | Speed shrinks size | 0 | 0-1 |
| `velocity_opacity` | Speed lowers opacity | 0 | 0-1 |
| `hue_jitter` | Hue jitter | 0 | 0-1 |
| `saturation_jitter` | Saturation jitter | 0 | 0-1 |
| `value_jitter` | Value jitter | 0 | 0-1 |
| `hardness` | Hardness | 0.75 | 0-1 |
| `size_start` | Size at start | 1 | 0-1 |
| `size_end` | Size at end | 1 | 0-1 |
| `size_fade` | Transition (× size) | 10 | 0.1-100 |
| `grain` | Grain strength | 0 | 0-1 |
| `grain_scale` | Grain size | 3 | 1-32 |
| `dryness` | Dryness | 0 | 0-1 |
| `velocity_dryness` | Speed dries | 0 | 0-1 |
| `pressure_dryness` | Light pressure dries | 0 | 0-1 |
| `cycle_tips` | Tips in turn: a `set` goes strictly in order, not by weights | false | |
| `bristles` | Bristles (ribbon only) | 0 | 0-1 |
| `depletion` | Paint runs out along the stroke (over 60 brush sizes) | 0 | 0-1 |

A ribbon brush needs no tip (`tip: round`). `spacing`, `scatter`, the jitters, `hardness` and `accumulate` do nothing for it (`accumulate` is reset on load), and `dryness` strengthens the paper grain. The other parameters work as in stamp brushes.

The grain reads its height from a tileable paper map with fibers. Without `accumulate` the grain acts as a threshold: light pressure paints only the bumps, strong pressure fills the pits. With `accumulate` the grain sets how fast paint builds up: pits fill slower than bumps. For a round tip `dryness` strengthens the grain, for a mask it cuts off the weak bristles.

A curve is an array of `[x, y]` pairs. Values are clamped to 0-1, the points are sorted by `x`, points too close in `x` are merged, at most 16. Fewer than two points mean the linear curve `[[0, 0], [1, 1]]`.

## Importing other brushes

The brush dialog also opens `.gbr`, `.abr`, `.kpp` and plain images and saves them to the library as `.chqbrush`.

## Limits

- A tip is a circle or an 8-bit mask. There are no color tips.
- The only integrity check is the CRC-32 of a block. There are no signatures.
