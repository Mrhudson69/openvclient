# OpenVClient design system (master)

Source: `ui-ux-pro-max` skill, query `"internal database admin console developer tool" --design-system --variance 3 --motion 3 --density 9`, plus targeted `--domain ux` searches. The skill has no egui stack, so the mapping to egui is ours. Its suggested *page pattern* (FAQ/Documentation landing) was off-topic for a desktop tool and was **not adopted**.

Implementation: `crates/openvclient-app/src/theme.rs`. Views use `theme::tokens(ui)` roles, never raw hex.

## Style
Minimalism / Swiss: dense, grid-based, high contrast, functional. Dark by default; light fully supported.

## Colour tokens
| Role | Dark | Light |
|---|---|---|
| bg | #0F172A | #F8FAFC |
| card | #1B2336 | #FFFFFF |
| muted | #272F42 | #F1F5F9 |
| fg | #F8FAFC | #0F172A |
| muted_fg | #94A3B8 | #475569 |
| border | #334155 | #CBD5E1 |
| accent (Run/Connect/Save) | #22C55E | #15803D |
| destructive | #F87171 | #B91C1C |
| warning | #FBBF24 | #A16207 |
| info / ring (focus) | #60A5FA / #93C5FD | #1D4ED8 |

Text roles meet 4.5:1 on bg and card in both themes. The light values are darkened from the dark palette to reach that threshold (the skill's palette is dark-first). Syntax colours also meet 4.5:1.

## Typography
The skill recommends JetBrains Mono (code/grid) with IBM Plex Sans (UI). The app currently uses egui's bundled fonts, because adding those fonts needs the TTF files (OFL licence). Body text is at least 13 px, small text at least 12 px.

## Spacing and sizes
Dense scale 4/8/12/16/24/32 (`theme::space`). Icons are 14/16/20 (`theme::icon`). Desktop pointer targets are at least 26 px tall.

## Rules adopted from the skill
- Vector icons from one family (Material Symbols via `egui_material_icons`), never emoji.
- A visible focus ring on every focused control (`selection.stroke` = ring).
- Never colour alone: status dots and badges always come with text or a tooltip.
- Confirm before destructive actions. Production connections require typing the object name.
- A toast on success and an inline error next to the cause. Loading feedback without flashing for near-instant work.
- Multi-select and bulk actions in grids.
- Motion: one short duration (120 ms), no decorative animation.
