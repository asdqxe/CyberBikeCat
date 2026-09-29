//! SKETCH ONLY. Not part of the crate (it lives outside src/), not compiled, not tested.
//! It shows where a window backend would plug into avcii-sketch. It does NOT demonstrate
//! any GPU effect; the "expected improvement" notes are predictions until someone builds it.
//! wgpu/winit API shapes below are from memory of their general structure and must be
//! checked against current versions before use.

// ── Step 1: plain Rust, no GPU. Stop rounding; hand backends fractional glyphs. ──────────
//
// main.rs already has everything needed: `Frame` (continuous state) and
// `Projector::project() -> Screen { col, row, scale }` (fractional). Today only
// `draw_scene` / `draw_lamp` / `draw_building` turn those into whole cells.
// A second consumer would take the same inputs and emit instances instead of cells.

pub struct GlyphInstance {
    pub col: f32,   // fractional cell coordinates: the same space the terminal rounds
    pub row: f32,
    pub height: f32, // glyph height in cells. The terminal ignores this and swaps '*' 'O' '@'.
    pub angle: f32,  // radians; the terminal has no equivalent (sprites can only step by whole cells)
    pub ch: char,
    pub rgb: [f32; 3],
    pub glow: f32,   // 0..1, drives an additive halo in the window; the terminal has no equivalent
}

pub fn lamp_instances(proj: &Projector, lamp_z_rel: f32, side: f32, out: &mut Vec<GlyphInstance>) {
    if let Some(head) = proj.project(side * WALK_X, 5.2, lamp_z_rel) {
        out.push(GlyphInstance {
            col: head.col,                          // e.g. 46.59, not 47
            row: head.row,
            height: (head.scale * 0.12).clamp(0.5, 3.0), // grows smoothly with closeness
            angle: 0.0,
            ch: '@',
            rgb: [1.0, 0.92, 0.6],
            glow: (head.scale / 12.0).clamp(0.1, 1.0),
        });
        // pole: one '|' quad stretched from base to head, instead of N whole-cell '|' glyphs
    }
}

// The rider is authored on a cell grid (RIDER_* arrays), so a window backend would draw each
// glyph of the sprite at (sprite_origin + fractional sway) and rotate the group by a lean
// angle around the rear wheel. The terminal can only shift by -1/0/+1 cells.

// ── Step 2: the seam ─────────────────────────────────────────────────────────────────────

pub trait Backend {
    fn present(&mut self, frame: &Frame, proj: &Projector);
}

// TerminalBackend == what main.rs does now: draw_scene() -> Grid -> present() (diffed).

pub struct WindowBackend {
    // wgpu: Instance/Adapter/Device/Queue, Surface + config, one RenderPipeline,
    // a glyph atlas texture (rasterized once with a font crate), a growable instance buffer.
    // winit: window + event loop that calls present() on RedrawRequested.
}

impl Backend for WindowBackend {
    fn present(&mut self, frame: &Frame, proj: &Projector) {
        let mut instances: Vec<GlyphInstance> = Vec::new();
        // for each visible lamp / building window / road-paint dash / rider glyph:
        //     push GlyphInstance { fractional position, height, angle, glow, ... }
        //
        // queue.write_buffer(&self.instance_buf, 0, bytemuck::cast_slice(&packed(&instances)));
        // one render pass:  pass.draw(0..6, 0..instances.len() as u32);   // 2 triangles per glyph
        // optional second pass: additive blend of glow, or a blur for bloom.
    }
}

// ── Step 3: the smallest shader (sketch) ─────────────────────────────────────────────────
// Per-instance: position (cells), height (cells), angle, atlas UV rect, color, glow.
//
// @vertex fn vs(@builtin(vertex_index) v: u32, inst: Instance) -> VsOut {
//     let corner = QUAD[v];                            // (0,0)..(1,1)
//     let local  = (corner - 0.5) * vec2(inst.height * ASPECT_INV, inst.height);
//     let rot    = vec2(local.x * cos(inst.angle) - local.y * sin(inst.angle),
//                       local.x * sin(inst.angle) + local.y * cos(inst.angle));
//     let cells  = inst.pos + rot;                     // fractional cell space
//     out.clip   = cells_to_clip(cells);               // uniform: window size / cell size
//     out.uv     = mix(inst.uv_min, inst.uv_max, corner);
//     out.color  = inst.color;
// }
// @fragment fn fs(in: VsOut) -> @location(0) vec4<f32> {
//     let a = textureSample(atlas, samp, in.uv).r;     // glyph coverage from the atlas
//     return vec4(in.color, a);
// }
//
// What to look for once built (predictions, not results):
//  1. `--trace` shows the lamp moving ~0.06 cell/frame far away, then 8+ cells/frame near the camera.
//     The terminal shows that as cell hops; the window would draw the fractional position.
//  2. Glyph height would vary continuously with depth instead of three steps ('*' 'O' '@').
//  3. The rider could lean by rotating the quads instead of shifting by whole cells.
//  4. Glow/bloom around lamps, which no terminal cell can express.
