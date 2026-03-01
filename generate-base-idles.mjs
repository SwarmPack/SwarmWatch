import fs from 'node:fs/promises';

const SRC = 'Happy Dog.json';

function rgb(hex) {
  const h = hex.replace('#', '').trim();
  const n = parseInt(h.length === 3 ? h.split('').map(c => c + c).join('') : h, 16);
  return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255, 1];
}

function kf(t, s) {
  return {
    i: { x: [0.667], y: [1] },
    o: { x: [0.333], y: [0] },
    t,
    s: [s],
  };
}

function opacityPulse({ frames, on = 100, off = 0, pulses }) {
  // pulses: array of { start, rise, hold, fall, gap }
  const k = [];
  let lastT = 0;
  k.push(kf(0, off));

  for (const p of pulses) {
    const start = p.start;
    const rise = p.rise ?? 2;
    const hold = p.hold ?? 2;
    const fall = p.fall ?? 2;

    k.push(kf(start, off));
    k.push(kf(start + rise, on));
    k.push(kf(start + rise + hold, on));
    k.push(kf(start + rise + hold + fall, off));

    lastT = Math.max(lastT, start + rise + hold + fall);
  }

  if (lastT < frames) k.push(kf(frames, off));
  return { a: 1, k };
}

function shapeGroup({ nm, items, p = [0, 0], s = [100, 100], r = 0, o = 100 }) {
  return {
    ty: 'gr',
    nm,
    np: items.length,
    cix: 2,
    bm: 0,
    it: [
      ...items,
      {
        ty: 'tr',
        p: { a: 0, k: p, ix: 2 },
        a: { a: 0, k: [0, 0], ix: 1 },
        s: { a: 0, k: s, ix: 3 },
        r: { a: 0, k: r, ix: 6 },
        o: { a: 0, k: o, ix: 7 },
        sk: { a: 0, k: 0, ix: 4 },
        sa: { a: 0, k: 0, ix: 5 },
        nm: 'Transform',
      },
    ],
  };
}

function rect({ nm = 'Rect', size, pos, r = 10 }) {
  return {
    ty: 'rc',
    nm,
    d: 1,
    s: { a: 0, k: size, ix: 2 },
    p: { a: 0, k: pos, ix: 3 },
    r: { a: 0, k: r, ix: 4 },
  };
}

function fill({ color, o = 100, nm = 'Fill' }) {
  return {
    ty: 'fl',
    nm,
    c: { a: 0, k: color, ix: 4 },
    o: { a: 0, k: o, ix: 5 },
    r: 1,
    bm: 0,
  };
}

function stroke({ color, w = 6, o = 100, nm = 'Stroke' }) {
  return {
    ty: 'st',
    nm,
    c: { a: 0, k: color, ix: 3 },
    o: { a: 0, k: o, ix: 4 },
    w: { a: 0, k: w, ix: 5 },
    lc: 2,
    lj: 2,
    ml: 4,
    bm: 0,
  };
}

function path({ nm = 'Path', v, i, o, c = true }) {
  return {
    ty: 'sh',
    nm,
    ix: 1,
    ks: {
      a: 0,
      k: { v, i, o, c },
      ix: 2,
    },
  };
}

function layerBase({ ind, nm, p, a = [0, 0, 0], s = [100, 100, 100], r = 0, o = 100, ip, op, st = 0, bm = 0, parent }) {
  const layer = {
    ddd: 0,
    ind,
    ty: 4,
    nm,
    sr: 1,
    ks: {
      o: typeof o === 'object' ? o : { a: 0, k: o, ix: 11 },
      r: { a: 0, k: r, ix: 10 },
      p: { a: 0, k: p, ix: 2, l: 2 },
      a: { a: 0, k: a, ix: 1, l: 2 },
      s: { a: 0, k: s, ix: 6, l: 2 },
    },
    ao: 0,
    shapes: [],
    ip,
    op,
    st,
    bm,
  };
  if (parent != null) layer.parent = parent;
  return layer;
}

function makeBoltShape({ colorStroke, colorGlow, boltScale = 100, boltPos = [0, 0] }) {
  // Simple zig-zag bolt
  const v = [
    [-30, -55],
    [10, -55],
    [-5, -10],
    [30, -10],
    [-10, 55],
    [5, 10],
    [-30, 10],
  ];
  // crude tangents (mostly corners)
  const i = Array(v.length).fill([0, 0]);
  const o = Array(v.length).fill([0, 0]);

  const boltPath = path({ v, i, o, c: true });

  const core = shapeGroup({
    nm: 'BoltCore',
    p: boltPos,
    s: [boltScale, boltScale],
    items: [
      boltPath,
      // Add a faint fill so it stays visible even at small sizes.
      fill({ color: colorStroke, o: 22, nm: 'CoreFill' }),
      stroke({ color: colorStroke, w: 14, o: 100, nm: 'CoreStroke' }),
    ],
  });

  const glow = shapeGroup({
    nm: 'BoltGlow',
    p: boltPos,
    s: [boltScale, boltScale],
    items: [
      boltPath,
      stroke({ color: colorGlow, w: 34, o: 55, nm: 'GlowStroke' }),
    ],
  });

  return { core, glow };
}

function makeScales({ variant }) {
  const plateFill = rgb('#58646f');
  const plateStroke = rgb('#2b3138');
  const accent = variant === 2 ? rgb('#3a8cff') : variant === 3 ? rgb('#00c2ff') : rgb('#67b7ff');

  const plates = [];
  const plateLayout =
    variant === 1
      ? [
          { size: [140, 70], pos: [-60, -40], r: 18 },
          { size: [170, 78], pos: [50, 10], r: 18 },
          { size: [120, 60], pos: [-20, 70], r: 18 },
          { size: [90, 46], pos: [120, 70], r: 16 },
          { size: [70, 40], pos: [-110, 65], r: 16 },
        ]
      : variant === 2
        ? [
            { size: [160, 76], pos: [-40, -25], r: 20 },
            { size: [140, 70], pos: [80, 20], r: 20 },
            { size: [120, 58], pos: [10, 75], r: 18 },
            { size: [90, 44], pos: [-115, 60], r: 16 },
          ]
        : [
            { size: [170, 84], pos: [-25, -30], r: 22 },
            { size: [130, 64], pos: [90, 20], r: 18 },
            { size: [150, 70], pos: [20, 82], r: 18 },
            { size: [85, 42], pos: [-120, 75], r: 16 },
            { size: [80, 40], pos: [150, 75], r: 16 },
          ];

  for (let idx = 0; idx < plateLayout.length; idx++) {
    const p = plateLayout[idx];
    plates.push(
      shapeGroup({
        nm: `Plate_${idx + 1}`,
        items: [
          rect({ size: p.size, pos: p.pos, r: p.r }),
          fill({ color: plateFill, o: 40, nm: 'PlateFill' }),
          stroke({ color: plateStroke, w: 6, o: 100, nm: 'PlateStroke' }),
        ],
      }),
    );

    // tiny accent rivet
    plates.push(
      shapeGroup({
        nm: `Rivet_${idx + 1}`,
        items: [
          { ty: 'el', d: 1, s: { a: 0, k: [16, 16], ix: 2 }, p: { a: 0, k: [p.pos[0] - p.size[0] / 2 + 18, p.pos[1] - p.size[1] / 2 + 18], ix: 3 }, nm: 'Ellipse Path 1' },
          fill({ color: accent, o: 75, nm: 'RivetFill' }),
        ],
      }),
    );
  }

  return plates;
}

function trimPaths({ s = 0, e = 50, o = 0, nm = 'Trim Paths' }) {
  return {
    ty: 'tm',
    nm,
    s: { a: 0, k: s, ix: 1 },
    e: { a: 0, k: e, ix: 2 },
    o: { a: 0, k: o, ix: 3 },
    m: 1,
    ix: 1,
  };
}

function metalCuffGroup({ nm, ellipseSize = [72, 52], strokeW = 16, arcStart = 0, arcEnd = 55, arcOffset = 0 }) {
  // A “half ring” metal wrap around a leg.
  // We draw a thick stroked ellipse and trim it to show only part of the ring.
  const metalStroke = stroke({ color: rgb('#c6d0dc'), w: strokeW, o: 100, nm: 'MetalStroke' });
  const metalOutline = stroke({ color: rgb('#2b3138'), w: Math.max(4, Math.floor(strokeW * 0.35)), o: 90, nm: 'MetalOutline' });
  const highlight = stroke({ color: rgb('#eaf2ff'), w: Math.max(2, Math.floor(strokeW * 0.18)), o: 75, nm: 'Highlight' });

  return {
    ty: 'gr',
    nm,
    np: 6,
    cix: 2,
    bm: 0,
    it: [
      {
        ty: 'el',
        d: 1,
        s: { a: 0, k: ellipseSize, ix: 2 },
        p: { a: 0, k: [0, 0], ix: 3 },
        nm: 'CuffEllipse',
      },
      metalStroke,
      metalOutline,
      highlight,
      trimPaths({ s: arcStart, e: arcEnd, o: arcOffset, nm: 'CuffTrim' }),
      {
        ty: 'tr',
        p: { a: 0, k: [0, 0], ix: 2 },
        a: { a: 0, k: [0, 0], ix: 1 },
        s: { a: 0, k: [100, 100], ix: 3 },
        r: { a: 0, k: 0, ix: 6 },
        o: { a: 0, k: 100, ix: 7 },
        sk: { a: 0, k: 0, ix: 4 },
        sa: { a: 0, k: 0, ix: 5 },
        nm: 'Transform',
      },
    ],
  };
}

function addLegCuffs(comp) {
  const { ip, op } = comp;
  const layers = comp.layers;

  // These are hand-tuned positions derived from the Happy Dog bboxes.
  // (We can refine visually after preview.)
  const cuffs = [
    { nm: 'LEG_CUFF_front_right', p: [1014, 858, 0], size: [74, 54], w: 16, off: 12 },
    { nm: 'LEG_CUFF_front_left', p: [864, 862, 0], size: [72, 52], w: 16, off: 55 },
    { nm: 'LEG_CUFF_mid', p: [939, 830, 0], size: [66, 48], w: 14, off: 8 },
    { nm: 'LEG_CUFF_back', p: [1093, 845, 0], size: [70, 50], w: 15, off: 40 },
  ];

  const cuffLayers = cuffs.map((c, idx) => {
    const l = layerBase({
      ind: 240 + idx,
      nm: c.nm,
      p: c.p,
      a: [0, 0, 0],
      s: [100, 100, 100],
      r: 0,
      o: 100,
      ip,
      op,
      bm: 0,
    });
    l.shapes = [
      metalCuffGroup({
        nm: 'Cuff',
        ellipseSize: c.size,
        strokeW: c.w,
        arcStart: 0,
        arcEnd: 55,
        arcOffset: c.off,
      }),
    ];
    return l;
  });

  // Insert cuffs above the body & foot layers.
  // Earlier in `layers[]` renders on top for this export.
  const bodyIdx = layers.findIndex(l => l && l.ind === 4);
  const footIdx = layers.findIndex(l => l && l.ind === 5);
  const insertAt = bodyIdx >= 0 ? bodyIdx : (footIdx >= 0 ? footIdx : layers.length);
  layers.splice(insertAt, 0, ...cuffLayers);
}

function addCyberLayers(comp, variant) {
  const { ip, op } = comp;

  // Head lightning
  // Happy Dog head is around p=[928,429]. Place the lightning on the forehead.
  const headPos = variant === 2 ? [920, 388, 0] : variant === 3 ? [945, 382, 0] : [930, 386, 0];
  const bolt = makeBoltShape({
    colorStroke: rgb(variant === 2 ? '#4bd2ff' : variant === 3 ? '#00e5ff' : '#67b7ff'),
    colorGlow: rgb(variant === 2 ? '#85e8ff' : variant === 3 ? '#66f2ff' : '#9be7ff'),
    boltScale: variant === 3 ? 110 : 100,
    boltPos: [0, 0],
  });

  const pulse = opacityPulse({
    frames: op,
    pulses: [
      { start: 12, rise: 2, hold: 4, fall: 2 },
      { start: 46, rise: 2, hold: 3, fall: 2 },
      { start: 82, rise: 2, hold: 4, fall: 2 },
      { start: 120, rise: 2, hold: 3, fall: 2 },
      { start: 150, rise: 1, hold: 2, fall: 1 },
    ],
  });

  const headGlowLayer = layerBase({
    ind: 200 + variant * 10 + 1,
    nm: `HEAD_GLOW_base${variant}`,
    p: headPos,
    a: [0, 0, 0],
    s: [100, 100, 100],
    r: variant === 2 ? -8 : variant === 3 ? 6 : 0,
    o: pulse,
    ip,
    op,
    bm: 0,
  });
  headGlowLayer.ef = [
    {
      ty: 29,
      nm: 'Gaussian Blur',
      np: 5,
      mn: 'ADBE Gaussian Blur 2',
      ix: 1,
      en: 1,
      ef: [
        { ty: 0, nm: 'Blurriness', mn: 'ADBE Gaussian Blur 2-0001', ix: 1, v: { a: 0, k: variant === 3 ? 24 : 20, ix: 1 } },
        { ty: 7, nm: 'Blur Dimensions', mn: 'ADBE Gaussian Blur 2-0002', ix: 2, v: { a: 0, k: 1, ix: 2 } },
        { ty: 7, nm: 'Repeat Edge Pixels', mn: 'ADBE Gaussian Blur 2-0003', ix: 3, v: { a: 0, k: 0, ix: 3 } },
      ],
    },
  ];
  headGlowLayer.shapes = [bolt.glow];

  const headCoreLayer = layerBase({
    ind: 200 + variant * 10 + 2,
    nm: `HEAD_LIGHTNING_base${variant}`,
    p: headPos,
    a: [0, 0, 0],
    s: [100, 100, 100],
    r: variant === 2 ? -8 : variant === 3 ? 6 : 0,
    o: 100,
    ip,
    op,
    bm: 0,
  });
  // subtle shimmer scale
  headCoreLayer.ks.s = {
    a: 1,
    k: [
      { i: { x: [0.667, 0.667, 0.667], y: [1, 1, 1] }, o: { x: [0.333, 0.333, 0.333], y: [0, 0, 0] }, t: 0, s: [100, 100, 100] },
      { i: { x: [0.667, 0.667, 0.667], y: [1, 1, 1] }, o: { x: [0.333, 0.333, 0.333], y: [0, 0, 0] }, t: 12, s: [105, 105, 100] },
      { i: { x: [0.667, 0.667, 0.667], y: [1, 1, 1] }, o: { x: [0.333, 0.333, 0.333], y: [0, 0, 0] }, t: 24, s: [100, 100, 100] },
      { i: { x: [0.667, 0.667, 0.667], y: [1, 1, 1] }, o: { x: [0.333, 0.333, 0.333], y: [0, 0, 0] }, t: 82, s: [106, 106, 100] },
      { t: op, s: [100, 100, 100] },
    ],
    ix: 6,
    l: 2,
  };

  // Add a tiny antenna on variant 3
  const antenna =
    variant === 3
      ? shapeGroup({
          nm: 'Antenna',
          items: [
            path({
              v: [
                [20, -70],
                [20, -155],
              ],
              i: [
                [0, 0],
                [0, 0],
              ],
              o: [
                [0, 0],
                [0, 0],
              ],
              c: false,
            }),
            stroke({ color: rgb('#2b3138'), w: 8, o: 100, nm: 'AntennaStroke' }),
            // Bigger, bluer bulb
            { ty: 'el', d: 1, s: { a: 0, k: [30, 30], ix: 2 }, p: { a: 0, k: [20, -155], ix: 3 }, nm: 'Tip' },
            fill({ color: rgb('#007bff'), o: 95, nm: 'TipFill' }),
            stroke({ color: rgb('#bfe6ff'), w: 4, o: 75, nm: 'TipGlowStroke' }),
          ],
        })
      : null;

  headCoreLayer.shapes = [bolt.core, ...(antenna ? [antenna] : [])];

  // Layer ordering: in Lottie, earlier entries in `layers[]` tend to draw on top.
  // So to make our overlays visible, we need to insert them *before* the dog layers.
  const layers = comp.layers;
  const headIdx = layers.findIndex(l => l && l.ind === 3);

  // Put lightning above head but below eyes => insert right before the head layer.
  const insertLightningAt = headIdx >= 0 ? headIdx : 2;
  layers.splice(insertLightningAt, 0, headGlowLayer, headCoreLayer);

  // Add metal leg cuffs (cyborg vibe)
  addLegCuffs(comp);
}

async function main() {
  const raw = await fs.readFile(SRC, 'utf8');
  const base = JSON.parse(raw);

  // Focus on base3 only.
  for (const variant of [3]) {
    const comp = structuredClone(base);
    comp.nm = `base${variant}-idle`;

    addCyberLayers(comp, variant);

    // Slightly more "robotic" tone: cool down the main head/body fill colors a tiny bit
    // (very conservative to avoid breaking the cute vibe)
    // We'll just rename as well to avoid confusion.
    const outPath = `base${variant}-idle.json`;
    await fs.writeFile(outPath, JSON.stringify(comp));
    console.log(`Wrote ${outPath}`);
  }
}

main().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
