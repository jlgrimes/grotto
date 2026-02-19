// Grotto Crab Creator — HGSS-style pixel art crab
(async function () {
  'use strict';

  // --- Sprite buffer system ---
  // We define the crab as a 2D pixel grid with palette indices
  // Then render it scaled up. This gives authentic DS-era sprite feel.

  const SPRITE_W = 34;
  const SPRITE_H = 30;

  // Palette indices
  const _ = 0;  // transparent
  const O = 1;  // outline
  const B = 2;  // body
  const H = 3;  // highlight
  const S = 4;  // shadow
  const W = 5;  // eye white
  const P = 6;  // pupil
  const C = 7;  // claw
  const L = 8;  // claw highlight
  const D = 9;  // claw dark
  const M = 10; // mouth/marking
  const K = 11; // cheek blush
  const E = 12; // eye stalk
  const T = 13; // leg

  // Color palettes per crab color
  // Each palette maps index → hex color
  function makePalette(base) {
    const dk = darken(base, 0.55);
    const lt = lighten(base, 1.35);
    const sh = darken(base, 0.75);
    const clw = darken(base, 0.65);
    const clwLt = darken(base, 0.8);
    const clwDk = darken(base, 0.45);
    const stalk = darken(base, 0.85);
    const leg = darken(base, 0.6);
    return {
      [O]: 0x1a0e08,
      [B]: base,
      [H]: lt,
      [S]: sh,
      [W]: 0xf8f8f0,
      [P]: 0x181020,
      [C]: clw,
      [L]: clwLt,
      [D]: clwDk,
      [M]: dk,
      [K]: 0xf08888,
      [E]: stalk,
      [T]: leg,
    };
  }

  const CRAB_COLORS = [
    { name: 'coral',  hex: 0xe06050 },
    { name: 'azure',  hex: 0x50a0e0 },
    { name: 'sunny',  hex: 0xe0c050 },
    { name: 'kelp',   hex: 0x50c878 },
    { name: 'urchin', hex: 0xc070d0 },
    { name: 'tang',   hex: 0xe08040 },
    { name: 'tidal',  hex: 0x60d0d0 },
    { name: 'blush',  hex: 0xd06090 },
  ];

  const STATES = ['idle', 'working', 'completed', 'spawning'];
  const FEATURES_DEF = [
    { key: 'bubbles',     label: '🫧 Bubbles' },
    { key: 'blush',       label: '😊 Blush' },
    { key: 'hats',        label: '🎩 Hats' },
    { key: 'sleep',       label: '💤 Sleep' },
    { key: 'particles',   label: '🎉 Confetti' },
    { key: 'squash',      label: '🪗 Squash' },
    { key: 'expressions', label: '👀 Eyes' },
  ];

  let currentState = 'idle';
  let currentColorIdx = 0;
  const features = { bubbles:false, blush:false, hats:false, sleep:false, particles:false, squash:false, expressions:false };

  // --- HGSS-style crab sprite definition ---
  // 34 wide × 30 tall pixel grid
  const CRAB_IDLE = [
    [_,_,_,_,_,_,_,_,_,_,_,O,O,_,_,_,_,_,_,_,O,O,_,_,_,_,_,_,_,_,_,_,_,_],
    [_,_,_,_,_,_,_,_,_,_,O,E,E,O,_,_,_,_,_,O,E,E,O,_,_,_,_,_,_,_,_,_,_,_],
    [_,_,_,_,_,_,_,_,_,O,W,W,W,W,O,_,_,_,O,W,W,W,W,O,_,_,_,_,_,_,_,_,_,_],
    [_,_,_,_,_,_,_,_,_,O,W,W,P,P,O,_,_,_,O,W,W,P,P,O,_,_,_,_,_,_,_,_,_,_],
    [_,_,_,_,_,_,_,_,_,O,W,P,P,W,O,_,_,_,O,W,P,P,W,O,_,_,_,_,_,_,_,_,_,_],
    [_,_,_,_,_,_,_,_,_,_,O,O,O,O,_,_,_,_,_,O,O,O,O,_,_,_,_,_,_,_,_,_,_,_],
    [_,_,_,_,_,_,_,_,_,_,_,O,E,O,_,_,_,_,_,O,E,O,_,_,_,_,_,_,_,_,_,_,_,_],
    [_,_,_,_,_,_,_,_,O,O,O,O,O,O,O,O,O,O,O,O,O,O,O,O,O,O,_,_,_,_,_,_,_,_],
    [_,_,_,_,_,_,_,O,H,H,H,H,H,H,H,H,H,H,H,H,H,H,H,H,H,H,O,_,_,_,_,_,_,_],
    [_,_,_,_,_,_,O,H,H,H,B,B,B,B,B,B,B,B,B,B,B,B,B,B,H,H,H,O,_,_,_,_,_,_],
    [_,_,_,_,_,O,H,H,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,H,H,O,_,_,_,_,_],
    [_,_,O,O,O,O,H,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,H,O,O,O,O,_,_],
    [_,O,D,C,C,O,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,O,C,C,D,O,_],
    [O,D,C,C,L,O,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,O,L,C,C,D,O],
    [O,D,C,L,L,O,B,B,B,B,B,B,B,M,M,B,B,B,B,M,M,B,B,B,B,B,B,B,O,L,L,C,D,O],
    [O,D,C,C,L,O,B,B,B,B,B,B,B,B,M,M,M,M,M,M,B,B,B,B,B,B,B,B,O,L,C,C,D,O],
    [O,D,C,C,C,O,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,O,C,C,C,D,O],
    [_,O,D,C,O,O,B,B,B,S,S,B,B,B,B,B,B,B,B,B,B,B,B,S,S,B,B,B,O,O,C,D,O,_],
    [_,_,O,O,O,O,B,B,S,S,S,S,B,B,B,B,B,B,B,B,B,B,S,S,S,S,B,B,O,O,O,O,_,_],
    [_,_,_,_,_,O,B,S,S,S,S,S,S,B,B,B,B,B,B,B,B,S,S,S,S,S,S,B,O,_,_,_,_,_],
    [_,_,_,_,_,_,O,S,S,S,B,B,B,B,B,B,B,B,B,B,B,B,B,B,S,S,S,O,_,_,_,_,_,_],
    [_,_,_,_,_,_,_,O,S,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,B,S,O,_,_,_,_,_,_,_],
    [_,_,_,_,_,_,_,_,O,O,O,B,B,B,B,B,B,B,B,B,B,B,B,O,O,O,_,_,_,_,_,_,_,_],
    [_,_,_,_,_,_,_,_,_,_,O,O,O,O,O,O,O,O,O,O,O,O,O,O,_,_,_,_,_,_,_,_,_,_],
    [_,_,_,_,_,_,_,_,_,O,T,O,_,O,T,O,_,_,O,T,O,_,O,T,O,_,_,_,_,_,_,_,_,_],
    [_,_,_,_,_,_,_,_,O,T,T,O,_,O,T,T,O,_,O,T,T,O,_,O,T,T,O,_,_,_,_,_,_,_],
    [_,_,_,_,_,_,_,O,T,O,_,_,_,_,O,T,O,_,O,T,O,_,_,_,_,O,T,O,_,_,_,_,_,_],
    [_,_,_,_,_,_,_,O,O,_,_,_,_,_,O,O,_,_,_,O,O,_,_,_,_,_,O,O,_,_,_,_,_,_],
  ];

  // --- Pixi Setup ---
  const app = new PIXI.Application();
  const container = document.getElementById('stage-container');
  await app.init({
    canvas: document.getElementById('pixi-canvas'),
    resizeTo: container,
    background: 0x0a0e1a,
    antialias: false,
    resolution: window.devicePixelRatio || 1,
    autoDensity: true,
  });

  const bgLayer = new PIXI.Container();
  const particleLayer = new PIXI.Container();
  const crabLayer = new PIXI.Container();
  app.stage.addChild(bgLayer, particleLayer, crabLayer);

  // --- Helpers ---
  function darken(c, f) {
    return (Math.floor(((c>>16)&0xff)*f)<<16)|(Math.floor(((c>>8)&0xff)*f)<<8)|Math.floor((c&0xff)*f);
  }
  function lighten(c, f) {
    return (Math.min(255,Math.floor(((c>>16)&0xff)*f))<<16)|(Math.min(255,Math.floor(((c>>8)&0xff)*f))<<8)|Math.min(255,Math.floor((c&0xff)*f));
  }

  // --- Background (HGSS-inspired) ---
  function drawBackground() {
    bgLayer.removeChildren();
    const w = app.screen.width, h = app.screen.height;
    const sandY = h * 0.68;

    const waterDeep = new PIXI.Graphics();
    waterDeep.rect(0, 0, w, sandY * 0.4); waterDeep.fill(0x0e2840);
    bgLayer.addChild(waterDeep);

    const waterMid = new PIXI.Graphics();
    waterMid.rect(0, sandY * 0.4, w, sandY * 0.35); waterMid.fill(0x163858);
    bgLayer.addChild(waterMid);

    const waterShallow = new PIXI.Graphics();
    waterShallow.rect(0, sandY * 0.75, w, sandY * 0.25); waterShallow.fill(0x1e4868);
    bgLayer.addChild(waterShallow);

    for (let y = 15; y < sandY; y += 18) {
      const r = new PIXI.Graphics();
      r.moveTo(0, y);
      for (let x = 0; x < w; x += 14) {
        r.lineTo(x + 7, y + 2);
        r.lineTo(x + 14, y);
      }
      r.stroke({ width: 1, color: 0x2a5a8c, alpha: 0.2 });
      r.label = 'ripple';
      bgLayer.addChild(r);
    }

    const sandLight = new PIXI.Graphics();
    sandLight.rect(0, sandY, w, 6); sandLight.fill(0xe8c878);
    bgLayer.addChild(sandLight);

    const sand = new PIXI.Graphics();
    sand.rect(0, sandY + 6, w, h - sandY - 6); sand.fill(0xd4a857);
    bgLayer.addChild(sand);

    const dots = new PIXI.Graphics();
    for (let i = 0; i < 100; i++) {
      const dx = Math.random() * w;
      const dy = sandY + 8 + Math.random() * (h - sandY - 8);
      dots.rect(Math.floor(dx), Math.floor(dy), 2, 2);
    }
    dots.fill({ color: 0xc09840, alpha: 0.35 });
    bgLayer.addChild(dots);

    const hlDots = new PIXI.Graphics();
    for (let i = 0; i < 40; i++) {
      hlDots.rect(Math.floor(Math.random()*w), Math.floor(sandY+8+Math.random()*(h-sandY-8)), 2, 2);
    }
    hlDots.fill({ color: 0xf0d888, alpha: 0.3 });
    bgLayer.addChild(hlDots);
  }

  // --- Render sprite from pixel grid ---
  function renderSprite(pixelGrid, palette, pixelSize) {
    const container = new PIXI.Container();
    for (let y = 0; y < pixelGrid.length; y++) {
      const row = pixelGrid[y];
      let x = 0;
      while (x < row.length) {
        const idx = row[x];
        if (idx === 0) { x++; continue; }
        let runEnd = x + 1;
        while (runEnd < row.length && row[runEnd] === idx) runEnd++;
        const color = palette[idx];
        if (color !== undefined) {
          const g = new PIXI.Graphics();
          g.rect(x * pixelSize, y * pixelSize, (runEnd - x) * pixelSize, pixelSize);
          g.fill(color);
          container.addChild(g);
        }
        x = runEnd;
      }
    }
    return container;
  }

  // --- Crab ---
  let crabContainer = null;
  let crabSprite = null;
  let crabScale = 1;
  let pixelSize = 2;
  let anim = { frame: 0, bobPhase: 0, spawnProgress: 1, bubbleTimer: 0, zzzTimer: 0 };

  function calcScale() {
    const w = app.screen.width;
    const h = app.screen.height * 0.68;
    const spriteW = SPRITE_W * pixelSize;
    const spriteH = CRAB_IDLE.length * pixelSize;
    const scaleW = (w * 0.55) / spriteW;
    const scaleH = (h * 0.55) / spriteH;
    return Math.min(scaleW, scaleH);
  }

  function buildCrab() {
    crabLayer.removeChildren();
    const baseColor = CRAB_COLORS[currentColorIdx].hex;
    const palette = makePalette(baseColor);
    crabScale = calcScale();

    crabContainer = new PIXI.Container();
    crabSprite = renderSprite(CRAB_IDLE, palette, pixelSize);
    crabSprite.label = 'body';

    const blushOverlay = new PIXI.Graphics();
    blushOverlay.label = 'blush';
    blushOverlay.visible = features.blush;
    const bp = pixelSize;
    blushOverlay.circle(10 * bp, 15 * bp, 2.5 * bp);
    blushOverlay.circle(23 * bp, 15 * bp, 2.5 * bp);
    blushOverlay.fill({ color: 0xff8888, alpha: 0.35 });

    const hatOverlay = new PIXI.Container();
    hatOverlay.label = 'hat';

    crabContainer.addChild(crabSprite);
    crabContainer.addChild(blushOverlay);
    crabContainer.addChild(hatOverlay);

    const totalW = SPRITE_W * pixelSize;
    const totalH = CRAB_IDLE.length * pixelSize;
    crabContainer.pivot.set(totalW / 2, totalH / 2);
    crabContainer.scale.set(crabScale);

    crabContainer.x = app.screen.width / 2;
    crabContainer.y = app.screen.height * 0.68 - (totalH * crabScale) / 2 + 10;

    crabLayer.addChild(crabContainer);
    drawHat();
  }

  // --- Hat drawing ---
  function drawHat() {
    if (!crabContainer) return;
    const hatC = crabContainer.children.find(c => c.label === 'hat');
    if (!hatC) return;
    hatC.removeChildren();
    if (!features.hats) { hatC.visible = false; return; }
    hatC.visible = true;

    const p = pixelSize;
    const hatSprite = new PIXI.Graphics();

    if (currentState === 'working') {
      const y0 = 5 * p;
      hatSprite.rect(10*p, y0, 14*p, p); hatSprite.fill(0xe0b020);
      hatSprite.rect(9*p, y0-p, 16*p, p); hatSprite.fill(0xf0c830);
      hatSprite.rect(11*p, y0-2*p, 12*p, p); hatSprite.fill(0xf0c830);
      hatSprite.rect(13*p, y0-3*p, 8*p, p); hatSprite.fill(0xf8d840);
      hatSprite.rect(8*p, y0, p, p); hatSprite.rect(25*p, y0, p, p); hatSprite.fill(0x806010);
    } else if (currentState === 'completed') {
      const y0 = 5 * p;
      hatSprite.rect(15*p, y0-4*p, 4*p, p); hatSprite.fill(0xd060d0);
      hatSprite.rect(14*p, y0-3*p, 6*p, p); hatSprite.fill(0xc050c0);
      hatSprite.rect(13*p, y0-2*p, 8*p, p); hatSprite.fill(0xb040b0);
      hatSprite.rect(12*p, y0-p, 10*p, p); hatSprite.fill(0xa030a0);
      hatSprite.rect(11*p, y0, 12*p, p); hatSprite.fill(0x9020a0);
      hatSprite.rect(14*p, y0-3*p, 2*p, p); hatSprite.fill(0xf0e060);
      hatSprite.rect(13*p, y0-p, 2*p, p); hatSprite.fill(0xf0e060);
      hatSprite.rect(15*p, y0-5*p, 2*p, p); hatSprite.rect(16*p, y0-6*p, 2*p, p);
      hatSprite.fill(0xf0f060);
    } else if (currentState === 'spawning') {
      const y0 = 5 * p;
      hatSprite.rect(11*p, y0, 3*p, p); hatSprite.fill(0xf0f0e0);
      hatSprite.rect(12*p, y0-p, 2*p, p); hatSprite.fill(0xf0f0e0);
      hatSprite.rect(20*p, y0, 3*p, p); hatSprite.fill(0xf0f0e0);
      hatSprite.rect(20*p, y0-p, 2*p, p); hatSprite.fill(0xf0f0e0);
    } else {
      const y0 = 4 * p;
      const fx = 20 * p;
      hatSprite.rect(fx, y0-p, 2*p, p); hatSprite.fill(0xf06080);
      hatSprite.rect(fx-p, y0-2*p, p, p); hatSprite.rect(fx+2*p, y0-2*p, p, p);
      hatSprite.rect(fx-p, y0, p, p); hatSprite.rect(fx+2*p, y0, p, p);
      hatSprite.fill(0xff90a0);
      hatSprite.rect(fx, y0, p, 2*p); hatSprite.fill(0x40a040);
    }
    hatC.addChild(hatSprite);
  }

  // --- Particles ---
  let bubbles = [], zzzs = [], confettiList = [];

  function spawnBubble() {
    if (!features.bubbles || !crabContainer) return;
    const b = new PIXI.Graphics();
    const r = 2 + Math.random() * 4;
    b.circle(0,0,r);
    b.stroke({ width: 1, color: 0x80c0ff, alpha: 0.5 });
    b.x = crabContainer.x + (Math.random()-0.5)*40;
    b.y = crabContainer.y - crabScale * 15;
    particleLayer.addChild(b);
    bubbles.push({ g:b, vy:-0.4-Math.random()*0.6, vx:(Math.random()-0.5)*0.3, life:100+Math.random()*60 });
  }

  function spawnZzz() {
    if (!features.sleep || !crabContainer) return;
    const t = new PIXI.Text({
      text: ['z','Z','z'][Math.floor(Math.random()*3)],
      style: { fontFamily:'Courier New', fontSize:10+Math.random()*8, fill:0x8899aa, fontStyle:'italic' }
    });
    t.anchor.set(0.5);
    t.x = crabContainer.x + 25 + Math.random()*10;
    t.y = crabContainer.y - crabScale * 14;
    particleLayer.addChild(t);
    zzzs.push({ g:t, vy:-0.25, vx:0.15+Math.random()*0.2, life:70+Math.random()*40 });
  }

  function spawnConfetti() {
    if (!features.particles || !crabContainer) return;
    const colors = [0xf06060,0x60f060,0x6060f0,0xf0f060,0xf060f0,0x60f0f0,0xffa040,0x40ffa0];
    for (let i = 0; i < 14; i++) {
      const c = new PIXI.Graphics();
      const s = 2 + Math.random()*3;
      if (Math.random() > 0.5) {
        c.rect(-s/2,-s/2,s,s);
      } else {
        c.rect(-s/2,-s/4,s,s/2);
      }
      c.fill(colors[i%colors.length]);
      c.x = crabContainer.x; c.y = crabContainer.y - crabScale * 10;
      particleLayer.addChild(c);
      confettiList.push({ g:c, vx:(Math.random()-0.5)*5, vy:-3-Math.random()*4, gravity:0.1, spin:(Math.random()-0.5)*0.25, life:90+Math.random()*40 });
    }
  }

  // --- Animation ---
  let frameCount = 0;

  app.ticker.add(() => {
    if (!crabContainer) return;
    frameCount++;
    const dt = app.ticker.deltaTime;
    anim.frame += dt;

    const sandY = app.screen.height * 0.68;
    const totalH = CRAB_IDLE.length * pixelSize;
    const baseY = sandY - (totalH * crabScale) / 2 + 10;

    if (currentState === 'spawning') {
      anim.spawnProgress = Math.min(1, anim.spawnProgress + 0.008 * dt);
      crabContainer.y = baseY + (1 - anim.spawnProgress) * 50;
      crabContainer.alpha = anim.spawnProgress;
      if (features.squash) {
        const sq = 0.85 + anim.spawnProgress * 0.15;
        crabContainer.scale.set(crabScale * (2-sq), crabScale * sq);
      }
      if (anim.spawnProgress >= 1) {
        currentState = 'idle';
        crabContainer.scale.set(crabScale);
        drawHat(); updateUI();
      }
    } else if (currentState === 'idle') {
      const bob = Math.sin(anim.frame * 0.025 + anim.bobPhase) * 2;
      crabContainer.y = baseY + bob;
      crabContainer.rotation = 0;

      if (features.squash) {
        const s = 1 + Math.sin(anim.frame * 0.025 + anim.bobPhase) * 0.015;
        crabContainer.scale.set(crabScale, crabScale * s);
      } else {
        crabContainer.scale.set(crabScale);
      }

      anim.zzzTimer += dt;
      if (features.sleep && anim.zzzTimer > 55) { spawnZzz(); anim.zzzTimer = 0; }

    } else if (currentState === 'working') {
      const bob = Math.sin(anim.frame * 0.08) * 2;
      crabContainer.y = baseY + bob;
      crabContainer.rotation = Math.sin(anim.frame * 0.1) * 0.025;

      if (features.squash) {
        const s = 1 + Math.sin(anim.frame * 0.16) * 0.03;
        crabContainer.scale.set(crabScale * s, crabScale * (2-s));
      }

      anim.bubbleTimer += dt;
      if (anim.bubbleTimer > 18) { spawnBubble(); anim.bubbleTimer = 0; }

    } else if (currentState === 'completed') {
      const bounce = Math.abs(Math.sin(anim.frame * 0.09)) * 20;
      crabContainer.y = baseY - bounce;
      crabContainer.rotation = Math.sin(anim.frame * 0.12) * 0.1;

      if (features.squash) {
        const land = Math.sin(anim.frame * 0.09);
        if (Math.abs(land) < 0.12) {
          crabContainer.scale.set(crabScale * 1.1, crabScale * 0.9);
        } else {
          crabContainer.scale.set(crabScale);
        }
      }

      if (features.particles && anim.frame % 100 < 1) spawnConfetti();
    }

    const blush = crabContainer?.children?.find(c => c.label === 'blush');
    if (blush) blush.visible = features.blush;

    for (let i = bubbles.length-1; i >= 0; i--) {
      const b = bubbles[i]; b.g.x += b.vx; b.g.y += b.vy; b.life -= dt;
      b.g.alpha = Math.max(0, b.life/100);
      if (b.life <= 0) { particleLayer.removeChild(b.g); bubbles.splice(i,1); }
    }
    for (let i = zzzs.length-1; i >= 0; i--) {
      const z = zzzs[i]; z.g.x += z.vx; z.g.y += z.vy; z.life -= dt;
      z.g.alpha = Math.max(0, z.life/70); z.g.scale.set(0.8+(1-z.life/110)*0.6);
      if (z.life <= 0) { particleLayer.removeChild(z.g); zzzs.splice(i,1); }
    }
    for (let i = confettiList.length-1; i >= 0; i--) {
      const c = confettiList[i]; c.g.x += c.vx; c.g.y += c.vy; c.vy += c.gravity;
      c.g.rotation += c.spin; c.life -= dt; c.g.alpha = Math.max(0, c.life/60);
      if (c.life <= 0) { particleLayer.removeChild(c.g); confettiList.splice(i,1); }
    }

    for (const child of bgLayer.children) {
      if (child.label === 'ripple' && child.position) {
        child.x = Math.sin(frameCount * 0.004 + child.y * 0.01) * 2;
      }
    }
  });

  // --- UI ---
  function updateUI() {
    document.querySelectorAll('#state-buttons button').forEach(b => {
      b.classList.toggle('active', b.dataset.state === currentState);
    });
    document.querySelectorAll('#color-buttons .color-btn').forEach((b,i) => {
      b.classList.toggle('active', i === currentColorIdx);
    });
    document.querySelectorAll('#feature-buttons button').forEach(b => {
      b.classList.toggle('feature-on', features[b.dataset.key]);
    });
    document.getElementById('crab-name').textContent = '🦀 ' + CRAB_COLORS[currentColorIdx].name;
  }

  const stateDiv = document.getElementById('state-buttons');
  for (const state of STATES) {
    const icons = { idle:'😌', working:'🔨', completed:'🎉', spawning:'🥚' };
    const btn = document.createElement('button');
    btn.textContent = `${icons[state]} ${state}`;
    btn.dataset.state = state;
    btn.addEventListener('click', () => {
      currentState = state;
      anim.frame = 0;
      if (state === 'spawning') { anim.spawnProgress = 0; crabContainer.alpha = 0; }
      if (state === 'completed') { spawnConfetti(); }
      drawHat(); updateUI();
    });
    stateDiv.appendChild(btn);
  }

  const colorDiv = document.getElementById('color-buttons');
  for (let i = 0; i < CRAB_COLORS.length; i++) {
    const btn = document.createElement('button');
    btn.className = 'color-btn';
    btn.style.background = `#${CRAB_COLORS[i].hex.toString(16).padStart(6,'0')}`;
    btn.addEventListener('click', () => { currentColorIdx = i; buildCrab(); updateUI(); });
    colorDiv.appendChild(btn);
  }

  const featDiv = document.getElementById('feature-buttons');
  for (const f of FEATURES_DEF) {
    const btn = document.createElement('button');
    btn.textContent = f.label;
    btn.dataset.key = f.key;
    btn.addEventListener('click', () => {
      features[f.key] = !features[f.key];
      if (f.key === 'hats') drawHat();
      updateUI();
    });
    featDiv.appendChild(btn);
  }

  window.addEventListener('resize', () => {
    requestAnimationFrame(() => {
      drawBackground();
      crabScale = calcScale();
      if (crabContainer) {
        crabContainer.scale.set(crabScale);
        crabContainer.x = app.screen.width / 2;
      }
    });
  });

  // --- Init ---
  drawBackground();
  buildCrab();
  updateUI();

})();
