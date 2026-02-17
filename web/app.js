// Grotto Agent Monitor — pixi.js frontend
// Connects to ws://localhost:9090/ws for real-time agent events

(async function () {
  'use strict';

  // --- Constants ---
  const WS_URL = `ws://${location.host}/ws`;
  const SAND_Y_RATIO = 0.75; // sand line at 75% of stage height
  const CRAB_SCALE = 2;
  const PIXEL = 4; // size of one "pixel" in the pixel art

  // Crab colors for each agent
  const CRAB_COLORS = [
    0xe06050, // coral red
    0x50a0e0, // ocean blue
    0xe0c050, // sandy gold
    0x50c878, // sea green
    0xc070d0, // purple
    0xe08040, // orange
  ];

  // --- State ---
  let agents = {};
  let tasks = [];
  let config = {};
  let crabSprites = {};
  let ws = null;
  let reconnectTimer = null;

  // --- Pixi.js Setup ---
  const app = new PIXI.Application();
  const container = document.getElementById('stage-container');

  await app.init({
    canvas: document.getElementById('pixi-canvas'),
    resizeTo: container,
    background: 0x0a0e1a,
    antialias: false,
    resolution: 1,
  });

  // --- Scene layers ---
  const bgLayer = new PIXI.Container();
  const crabLayer = new PIXI.Container();
  const labelLayer = new PIXI.Container();
  app.stage.addChild(bgLayer, crabLayer, labelLayer);

  // --- Draw ocean background ---
  function drawBackground() {
    bgLayer.removeChildren();

    const w = app.screen.width;
    const h = app.screen.height;
    const sandY = h * SAND_Y_RATIO;

    // Water
    const water = new PIXI.Graphics();
    water.rect(0, 0, w, sandY);
    water.fill(0x1a3a5c);
    bgLayer.addChild(water);

    // Water ripple lines
    for (let y = 20; y < sandY; y += 30) {
      const ripple = new PIXI.Graphics();
      ripple.moveTo(0, y);
      for (let x = 0; x < w; x += 20) {
        ripple.lineTo(x + 10, y + 3);
        ripple.lineTo(x + 20, y);
      }
      ripple.stroke({ width: 1, color: 0x2a5a8c, alpha: 0.3 });
      bgLayer.addChild(ripple);
    }

    // Sand
    const sand = new PIXI.Graphics();
    sand.rect(0, sandY, w, h - sandY);
    sand.fill(0xd4a857);
    bgLayer.addChild(sand);

    // Sand texture dots
    const dots = new PIXI.Graphics();
    for (let i = 0; i < 80; i++) {
      const dx = Math.random() * w;
      const dy = sandY + Math.random() * (h - sandY);
      dots.circle(dx, dy, 1);
    }
    dots.fill({ color: 0xb08930, alpha: 0.5 });
    bgLayer.addChild(dots);

    // Coral decorations on the sand
    drawCoral(w * 0.1, sandY - 10);
    drawCoral(w * 0.85, sandY - 8);
    drawCoral(w * 0.5, sandY - 12);
  }

  function drawCoral(x, y) {
    const coral = new PIXI.Graphics();
    // Simple coral: a few branching rectangles
    const colors = [0xe06050, 0xc04838, 0xff7868];
    const c = colors[Math.floor(Math.random() * colors.length)];

    // trunk
    coral.rect(x - 2, y, 4, 14);
    coral.fill(c);
    // left branch
    coral.rect(x - 8, y - 6, 4, 10);
    coral.fill(c);
    // right branch
    coral.rect(x + 4, y - 4, 4, 8);
    coral.fill(c);
    // tips
    coral.circle(x, y - 2, 3);
    coral.circle(x - 6, y - 8, 2);
    coral.circle(x + 6, y - 6, 2);
    coral.fill(0xff9080);

    bgLayer.addChild(coral);
  }

  // --- Pixel Art Crab ---
  // Draws a crab using rectangles (pixel art style)
  // Returns a Container with the crab graphics
  function createCrab(agentId, colorIndex) {
    const p = PIXEL;
    const color = CRAB_COLORS[colorIndex % CRAB_COLORS.length];
    const darkColor = darken(color, 0.7);
    const lightColor = lighten(color, 1.3);

    const crab = new PIXI.Container();
    crab.label = agentId;

    // Body (elliptical blob made of pixel rects)
    const bodyPixels = [
      // row 0 (top) — narrow
      [2, 0], [3, 0], [4, 0], [5, 0],
      // row 1 — wider
      [1, 1], [2, 1], [3, 1], [4, 1], [5, 1], [6, 1],
      // row 2 — widest
      [0, 2], [1, 2], [2, 2], [3, 2], [4, 2], [5, 2], [6, 2], [7, 2],
      // row 3
      [0, 3], [1, 3], [2, 3], [3, 3], [4, 3], [5, 3], [6, 3], [7, 3],
      // row 4 — narrowing
      [1, 4], [2, 4], [3, 4], [4, 4], [5, 4], [6, 4],
      // row 5 (bottom)
      [2, 5], [3, 5], [4, 5], [5, 5],
    ];

    const body = new PIXI.Graphics();
    for (const [bx, by] of bodyPixels) {
      body.rect(bx * p, by * p, p, p);
    }
    body.fill(color);

    // Shell highlight
    const highlights = [[3, 1], [4, 1], [2, 2], [3, 2]];
    for (const [hx, hy] of highlights) {
      body.rect(hx * p, hy * p, p, p);
    }
    body.fill(lightColor);

    crab.addChild(body);

    // Eyes
    const eyes = new PIXI.Graphics();
    // Eye stalks
    eyes.rect(2 * p, -1 * p, p, p);
    eyes.rect(5 * p, -1 * p, p, p);
    eyes.fill(color);
    // Eyeballs
    eyes.rect(2 * p, -2 * p, p, p);
    eyes.rect(5 * p, -2 * p, p, p);
    eyes.fill(0xffffff);
    // Pupils
    eyes.rect(2 * p + p / 2, -2 * p, p / 2, p);
    eyes.rect(5 * p + p / 2, -2 * p, p / 2, p);
    eyes.fill(0x111111);
    crab.addChild(eyes);

    // Claws (left)
    const leftClaw = new PIXI.Graphics();
    leftClaw.rect(-2 * p, 1 * p, p, p);
    leftClaw.rect(-3 * p, 0 * p, p, 2 * p);
    leftClaw.rect(-4 * p, 0 * p, p, p);
    leftClaw.fill(darkColor);
    leftClaw.label = 'leftClaw';
    crab.addChild(leftClaw);

    // Claws (right)
    const rightClaw = new PIXI.Graphics();
    rightClaw.rect(8 * p, 1 * p, p, p);
    rightClaw.rect(9 * p, 0 * p, p, 2 * p);
    rightClaw.rect(10 * p, 0 * p, p, p);
    rightClaw.fill(darkColor);
    rightClaw.label = 'rightClaw';
    crab.addChild(rightClaw);

    // Legs (3 per side)
    const legs = new PIXI.Graphics();
    for (let i = 0; i < 3; i++) {
      const ly = (2 + i) * p + p;
      // Left legs
      legs.rect(-1 * p, ly, p, p / 2);
      legs.rect(-2 * p, ly + p / 2, p, p / 2);
      // Right legs
      legs.rect(8 * p, ly, p, p / 2);
      legs.rect(9 * p, ly + p / 2, p, p / 2);
    }
    legs.fill(darkColor);
    legs.label = 'legs';
    crab.addChild(legs);

    // Center the crab
    crab.pivot.set((7 * p) / 2, (5 * p) / 2);
    crab.scale.set(CRAB_SCALE);

    // Agent name label
    const label = new PIXI.Text({
      text: agentId,
      style: {
        fontFamily: 'Courier New',
        fontSize: 11,
        fill: 0xe0d8c8,
        align: 'center',
      },
    });
    label.anchor.set(0.5, 1);
    label.label = 'nameLabel';

    // Status label below
    const statusLabel = new PIXI.Text({
      text: '',
      style: {
        fontFamily: 'Courier New',
        fontSize: 9,
        fill: 0x8899aa,
        align: 'center',
      },
    });
    statusLabel.anchor.set(0.5, 0);
    statusLabel.label = 'statusLabel';

    // Wrapper container that holds crab + labels
    const wrapper = new PIXI.Container();
    wrapper.addChild(crab);
    wrapper.addChild(label);
    wrapper.addChild(statusLabel);
    wrapper.label = agentId;

    // Position labels relative to crab
    label.position.set(0, -CRAB_SCALE * 3 * p);
    statusLabel.position.set(0, CRAB_SCALE * 4 * p);

    // Animation state
    wrapper._anim = {
      state: 'idle',
      frame: 0,
      walkDir: 1,
      walkSpeed: 0.3 + Math.random() * 0.2,
      bobPhase: Math.random() * Math.PI * 2,
      targetX: 0,
      spawnProgress: 0,
    };

    return wrapper;
  }

  function darken(color, factor) {
    const r = Math.floor(((color >> 16) & 0xff) * factor);
    const g = Math.floor(((color >> 8) & 0xff) * factor);
    const b = Math.floor((color & 0xff) * factor);
    return (r << 16) | (g << 8) | b;
  }

  function lighten(color, factor) {
    const r = Math.min(255, Math.floor(((color >> 16) & 0xff) * factor));
    const g = Math.min(255, Math.floor(((color >> 8) & 0xff) * factor));
    const b = Math.min(255, Math.floor((color & 0xff) * factor));
    return (r << 16) | (g << 8) | b;
  }

  // --- Animation Loop ---
  let frameCount = 0;

  app.ticker.add(() => {
    frameCount++;
    const dt = app.ticker.deltaTime;

    for (const [id, wrapper] of Object.entries(crabSprites)) {
      const anim = wrapper._anim;
      const crab = wrapper.children[0]; // the crab container
      const leftClaw = crab.children.find(c => c.label === 'leftClaw');
      const rightClaw = crab.children.find(c => c.label === 'rightClaw');
      anim.frame += dt;

      if (anim.state === 'spawning') {
        // Rise from sand
        anim.spawnProgress = Math.min(1, anim.spawnProgress + 0.01 * dt);
        const sandY = app.screen.height * SAND_Y_RATIO;
        const baseY = sandY - 10;
        wrapper.y = baseY + (1 - anim.spawnProgress) * 40;
        wrapper.alpha = anim.spawnProgress;

        if (anim.spawnProgress >= 1) {
          anim.state = agents[id]?.state === 'working' ? 'working' : 'idle';
        }
      } else if (anim.state === 'idle') {
        // Slow walk back and forth + bob
        const bob = Math.sin(anim.frame * 0.03 + anim.bobPhase) * 2;
        wrapper.y = app.screen.height * SAND_Y_RATIO - 10 + bob;

        // Occasional direction change
        if (Math.random() < 0.003) {
          anim.walkDir *= -1;
          crab.scale.x = CRAB_SCALE * anim.walkDir;
        }

        wrapper.x += anim.walkDir * anim.walkSpeed * 0.3 * dt;

        // Keep on screen
        const margin = 60;
        if (wrapper.x < margin) { anim.walkDir = 1; crab.scale.x = CRAB_SCALE; }
        if (wrapper.x > app.screen.width - margin) { anim.walkDir = -1; crab.scale.x = -CRAB_SCALE; }

        // Gentle claw wave
        if (leftClaw) leftClaw.y = Math.sin(anim.frame * 0.05) * 2;
        if (rightClaw) rightClaw.y = Math.sin(anim.frame * 0.05 + 1) * 2;

      } else if (anim.state === 'working') {
        // Busy hammering — faster bob, claw movement
        const bob = Math.sin(anim.frame * 0.08) * 1;
        wrapper.y = app.screen.height * SAND_Y_RATIO - 10 + bob;

        // Hammering claws
        if (leftClaw) {
          leftClaw.y = Math.sin(anim.frame * 0.2) * 4;
          leftClaw.rotation = Math.sin(anim.frame * 0.15) * 0.15;
        }
        if (rightClaw) {
          rightClaw.y = Math.sin(anim.frame * 0.2 + Math.PI) * 4;
          rightClaw.rotation = Math.sin(anim.frame * 0.15 + Math.PI) * 0.15;
        }

        // Slight rocking
        crab.rotation = Math.sin(anim.frame * 0.1) * 0.03;

      } else if (anim.state === 'completed') {
        // Victory dance — bounce + spin
        const bounce = Math.abs(Math.sin(anim.frame * 0.12)) * 15;
        wrapper.y = app.screen.height * SAND_Y_RATIO - 10 - bounce;

        crab.rotation = Math.sin(anim.frame * 0.15) * 0.2;

        if (leftClaw) leftClaw.y = Math.sin(anim.frame * 0.3) * 6;
        if (rightClaw) rightClaw.y = Math.sin(anim.frame * 0.3 + Math.PI) * 6;

        // Transition to idle after some time
        if (anim.frame > anim.danceStart + 200) {
          anim.state = 'idle';
          crab.rotation = 0;
        }
      }

      // Update status label
      const statusLabel = wrapper.children.find(c => c.label === 'statusLabel');
      if (statusLabel && agents[id]) {
        const a = agents[id];
        statusLabel.text = a.state || '';
        statusLabel.style.fill =
          a.state === 'working' ? 0xe0c050 :
          a.state === 'idle' ? 0x8899aa :
          a.state === 'spawning' ? 0x50c878 :
          0x8899aa;
      }
    }

    // Animate water ripples gently
    const ripples = bgLayer.children.filter((_, i) => i > 0 && i < bgLayer.children.length - 4);
    for (let i = 0; i < ripples.length; i++) {
      const r = ripples[i];
      if (r && r.position) {
        r.x = Math.sin(frameCount * 0.005 + i) * 3;
      }
    }
  });

  // --- Place crabs on screen ---
  function layoutCrabs() {
    const ids = Object.keys(crabSprites);
    const count = ids.length;
    if (count === 0) return;

    const w = app.screen.width;
    const spacing = w / (count + 1);

    for (let i = 0; i < ids.length; i++) {
      const wrapper = crabSprites[ids[i]];
      const targetX = spacing * (i + 1);
      if (wrapper._anim.state === 'spawning') {
        wrapper.x = targetX;
      }
      wrapper._anim.targetX = targetX;
    }
  }

  // --- Ensure crab sprites match agent state ---
  function syncCrabs() {
    const agentIds = Object.keys(agents);

    // Create missing crabs
    for (let i = 0; i < agentIds.length; i++) {
      const id = agentIds[i];
      if (!crabSprites[id]) {
        const wrapper = createCrab(id, i);
        wrapper.x = app.screen.width / 2;
        wrapper.y = app.screen.height * SAND_Y_RATIO + 30;
        wrapper._anim.state = 'spawning';
        crabSprites[id] = wrapper;
        crabLayer.addChild(wrapper);
      }
    }

    // Update states
    for (const [id, agent] of Object.entries(agents)) {
      const wrapper = crabSprites[id];
      if (!wrapper) continue;

      const anim = wrapper._anim;
      const newState = agent.state || 'idle';

      if (newState === 'working' && anim.state !== 'working' && anim.state !== 'spawning') {
        anim.state = 'working';
        anim.frame = 0;
      } else if (newState === 'idle' && anim.state === 'working') {
        // Transition through completed dance
        anim.state = 'completed';
        anim.danceStart = anim.frame;
      } else if (newState === 'spawning' && anim.state !== 'spawning') {
        anim.state = 'spawning';
        anim.spawnProgress = 0;
      }
    }

    layoutCrabs();
  }

  // --- Task Board UI ---
  function renderTaskBoard() {
    const board = document.getElementById('task-board');
    if (tasks.length === 0) {
      board.innerHTML = '<div style="color: var(--text-dim); font-size: 12px;">No tasks yet</div>';
      return;
    }

    board.innerHTML = tasks.map(t => {
      const statusClass =
        t.status === 'Completed' ? 'completed' :
        t.status === 'Claimed' || t.status === 'InProgress' ? 'claimed' :
        'open';
      const statusText =
        t.status === 'Completed' ? 'done' :
        t.status === 'Claimed' ? 'claimed' :
        t.status === 'InProgress' ? 'in progress' :
        t.status === 'Blocked' ? 'blocked' :
        'open';
      const agentLine = t.claimed_by ? `<div class="task-agent">${t.claimed_by}</div>` : '';
      const desc = t.description.length > 100 ? t.description.slice(0, 100) + '...' : t.description;

      return `<div class="task-card">
        <span class="task-id">${t.id}</span>
        <span class="task-status ${statusClass}">${statusText}</span>
        <div class="task-desc">${esc(desc)}</div>
        ${agentLine}
      </div>`;
    }).join('');
  }

  // --- Event Log ---
  function addLogEntry(event) {
    const entries = document.getElementById('log-entries');
    const div = document.createElement('div');
    div.className = 'log-entry';

    const time = event.timestamp
      ? new Date(event.timestamp).toLocaleTimeString()
      : '';

    const agentPart = event.agent_id
      ? `<span class="log-agent">[${esc(event.agent_id)}]</span>`
      : '';

    div.innerHTML =
      `<span class="log-time">${time}</span>` +
      `<span class="log-type">${esc(event.type || event.event_type || '?')}</span>` +
      agentPart +
      `<span>${esc(event.message || '')}</span>`;

    entries.appendChild(div);

    // Auto-scroll to bottom
    entries.scrollTop = entries.scrollHeight;

    // Limit entries
    while (entries.children.length > 200) {
      entries.removeChild(entries.firstChild);
    }
  }

  // --- WebSocket ---
  function connectWS() {
    setConnectionStatus('connecting');

    try {
      ws = new WebSocket(WS_URL);
    } catch (e) {
      setConnectionStatus('disconnected');
      scheduleReconnect();
      return;
    }

    ws.onopen = () => {
      setConnectionStatus('connected');
      if (reconnectTimer) {
        clearTimeout(reconnectTimer);
        reconnectTimer = null;
      }
    };

    ws.onmessage = (evt) => {
      let data;
      try {
        data = JSON.parse(evt.data);
      } catch {
        return;
      }
      handleEvent(data);
    };

    ws.onclose = () => {
      setConnectionStatus('disconnected');
      scheduleReconnect();
    };

    ws.onerror = () => {
      setConnectionStatus('disconnected');
    };
  }

  function scheduleReconnect() {
    if (reconnectTimer) return;
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      connectWS();
    }, 3000);
  }

  function setConnectionStatus(state) {
    const el = document.getElementById('connection-status');
    el.textContent = state;
    el.className = state;
  }

  // --- Event Handling ---
  function handleEvent(event) {
    switch (event.type) {
      case 'snapshot':
        // Full state dump on connect
        if (event.agents) {
          agents = event.agents;
        }
        if (event.tasks) {
          tasks = event.tasks;
        }
        if (event.config) {
          config = event.config;
          document.getElementById('task-label').textContent = config.task || '';
        }
        syncCrabs();
        renderTaskBoard();
        break;

      case 'agent:status':
        if (event.agent_id && event.data) {
          agents[event.agent_id] = {
            ...agents[event.agent_id],
            ...event.data,
          };
          syncCrabs();
        }
        addLogEntry(event);
        break;

      case 'task:claimed':
        if (event.task_id) {
          const task = tasks.find(t => t.id === event.task_id);
          if (task) {
            task.status = 'Claimed';
            task.claimed_by = event.agent_id || null;
          }
          renderTaskBoard();
        }
        addLogEntry(event);
        break;

      case 'task:completed':
        if (event.task_id) {
          const task = tasks.find(t => t.id === event.task_id);
          if (task) {
            task.status = 'Completed';
            task.completed_at = event.timestamp;
          }
          renderTaskBoard();
        }
        addLogEntry(event);
        break;

      case 'task:updated':
        // Full task board update from backend file watcher
        if (event.tasks) {
          tasks = event.tasks;
          renderTaskBoard();
        }
        addLogEntry(event);
        break;

      case 'team:spawned':
        addLogEntry(event);
        break;

      case 'agent:summary':
        addLogEntry(event);
        break;

      case 'event:raw':
        // Also check if this raw event carries task/agent info
        if (event.data && event.data.event_type) {
          const rawType = event.data.event_type;
          if (rawType === 'task_claimed' && event.agent_id) {
            const agent = agents[event.agent_id];
            if (agent) {
              agent.state = 'working';
              agent.current_task = event.task_id;
              syncCrabs();
            }
          } else if (rawType === 'task_completed' && event.agent_id) {
            const agent = agents[event.agent_id];
            if (agent) {
              agent.state = 'idle';
              agent.current_task = null;
              syncCrabs();
            }
          }
        }
        addLogEntry(event);
        break;

      default:
        addLogEntry(event);
        break;
    }
  }

  // --- Utility ---
  function esc(str) {
    const d = document.createElement('div');
    d.textContent = str;
    return d.innerHTML;
  }

  // --- Window resize ---
  function onResize() {
    drawBackground();
    layoutCrabs();
  }

  window.addEventListener('resize', () => {
    // pixi resizes automatically via resizeTo, we just redraw bg
    requestAnimationFrame(onResize);
  });

  // --- Init ---
  drawBackground();
  connectWS();

})();
