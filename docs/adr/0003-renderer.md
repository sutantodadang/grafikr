# ADR 0003 — SDF quad renderer + glyph atlas

**Status:** accepted (M1)

## Context

M1 targets rects, rounded rects, borders, glyphs, clipping, resize/surface
recovery, and running on more than one GPU.

## Decision

- One quad pipeline with an SDF fragment shader for rounded rects and borders.
  Borders are drawn as the difference between the outer and inner SDF.
- One text pipeline: every glyph is a textured quad from an R8 atlas.
- Glyphs are rasterized by Swash at physical size (`font_size × scale`), hinted,
  then packed into a 1024×1024 atlas with simple row allocation.
- The atlas cache is bounded; when full, it resets at the start of the next
  frame. Glyphs that do not fit are skipped (no crash, only tofu).
- Vertex colors are sRGB and converted to linear in the shader. The surface uses
  an sRGB format so composition is correct.
- The display list uses logical coordinates (DIP); the renderer multiplies by the
  scale.
- Parley layout runs at scale 1.0 so sizes remain DIP. Glyphs are rasterized at
  `font_size × window_scale` and origins are rounded to physical pixels.
- Scissor rect per batch; batches merge while clip and pipeline type match.
- Vertex buffers are reused and grow as needed.

## Consequences

- Shadow, images, transform, layers, damage tracking, and partial present were
  not present initially. Shadow, images, and transform were added later; layers
  and partial present remain future work.
- Subpixel positioning is disabled (glyphs are rounded to full pixels). Text
  quality is good at 100%, but could be sharper with 3–4 subpixel bins later.
- Atlas reset discards all glyphs; the text-layout cache is unaffected.
- Scissor is not a rounded clip; clipping follows the node rect. Rounded clipping
  needs a mask/stencil if required.
- One atlas is enough for one window. Multi-window uses one atlas per
  renderer/surface.
