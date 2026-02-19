// Grotto Agent Monitor — pixi.js frontend
// Connects to ws://host/ws/:session-id for real-time agent events

(async function () {
  'use strict';

  // --- Constants ---
  const SESSION_ID = location.pathname.replace(/^\//, '').replace(/\/$/, '');
  const WS_URL = `${location.protocol === 'https:' ? 'wss:' : 'ws:'}//${location.host}/ws/${SESSION_ID}`;
  const EVENTS_API_URL = `/api/sessions/${encodeURIComponent(SESSION_ID)}/events`;
  const SAND_Y_RATIO = 0.75;
  const CRAB_SCALE = 0.35; // scale down the DALL-E sprites

  // --- State ---
  let agents = {};
  let tasks = [];
  let config = {};
  let crabSprites = {};
  let ws = null;
  let reconnectTimer = null;
  let sessionCompleted = false;
  let historyMode = false;
  let wsEverConnected = false;
  let historyLoaded = false;

  // --- Load sprite textures ---
  const SPRITE_PATHS = {
    idle: '/sprites/crab_idle.png',
    working: '/sprites/crab_working.png',
    celebrating: '/sprites/crab_celebrating.png',
    spawning: '/sprites/crab_spawning.png',
  };

  const textures = {};
  for (const [state, path] of Object.entries(SPRITE_PATHS)) {
    try {
      textures[state] = await PIXI.Assets.load(path);
    } catch (e) {
      console.warn(`Failed to load sprite: ${path}`, e);
    }
  }

  // Hue shifts for different crab colors (base is red ~0°)
  const CRAB_COLORS = [
    { name: 'coral',  hueShift: 0 },
    { name: 'azure',  hueShift: 200 },
    { name: 'sunny',  hueShift: 45 },
    { name: 'kelp',   hueShift: 135 },
    { name: 'urchin', hueShift: 280 },
    { name: 'tang',   hueShift: 25 },
  ];

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

    const water = new PIXI.Graphics();
    water.rect(0, 0, w, sandY).fill(0x1a3a5c);
    bgLayer.addChild(water);

    for (let y = 20; y < sandY; y += 30) {
      const ripple = new PIXI.Graphics();
      ripple.moveTo(0, y);
      for (let x = 0; x < w; x += 20) {
        ripple.lineTo(x + 10, y + 3);
        ripple.lineTo(x + 20, y);
      }
      ripple.stroke({ width: 1, color: 0x2a5a8c, alpha: 0.3 });
      ripple.label = 'ripple';
      bgLayer.addChild(ripple);
    }

    const sand = new PIXI.Graphics();
    sand.rect(0, sandY, w, h - sandY).fill(0xd4a857);
    bgLayer.addChild(sand);

    const dots = new PIXI.Graphics();
    for (let i = 0; i < 80; i++) {
      dots.circle(Math.random() * w, sandY + Math.random() * (h - sandY), 1);
    }
    dots.fill({ color: 0xb08930, alpha: 0.5 });
    bgLayer.addChild(dots);

    drawCoral(w * 0.1, sandY - 10);
    drawCoral(w * 0.85, sandY - 8);
    drawCoral(w * 0.5, sandY - 12);
  }

  function drawCoral(x, y) {
    const coral = new PIXI.Graphics();
    const colors = [0xe06050, 0xc04838, 0xff7868];
    const c = colors[Math.floor(Math.random() * colors.length)];
    coral.rect(x - 2, y, 4, 14).fill(c);
    coral.rect(x - 8, y - 6, 4, 10).fill(c);
    coral.rect(x + 4, y - 4, 4, 8).fill(c);
    coral.circle(x, y - 2, 3);
    coral.circle(x - 6, y - 8, 2);
    coral.circle(x + 6, y - 6, 2);
    coral.fill(0xff9080);
    bgLayer.addChild(coral);
  }

  // --- Create Crab using DALL-E sprites ---
  function createCrab(agentId, colorIndex) {
    const wrapper = new PIXI.Container();
    wrapper.label = agentId;

    // Crab sprite
    const tex = textures.idle || PIXI.Texture.WHITE;
    const sprite = new PIXI.Sprite(tex);
    sprite.anchor.set(0.5, 0.85); // bottom-center anchor so it sits on sand
    sprite.scale.set(CRAB_SCALE);
    sprite.label = 'crabSprite';

    // Apply hue shift for color variety
    const colorDef = CRAB_COLORS[colorIndex % CRAB_COLORS.length];
    if (colorDef.hueShift !== 0) {
      try {
        const matrix = new PIXI.ColorMatrixFilter();
        matrix.hue(colorDef.hueShift - 10, false); // base crab is ~10° red
        sprite.filters = [matrix];
      } catch (e) {
        // ColorMatrixFilter may not be available in all builds
      }
    }

    wrapper.addChild(sprite);

    // Agent name label
    const label = new PIXI.Text({
      text: agentId,
      style: { fontFamily: 'Courier New', fontSize: 11, fill: 0xe0d8c8, align: 'center' },
    });
    label.anchor.set(0.5, 1);
    label.label = 'nameLabel';
    label.y = -tex.height * CRAB_SCALE * 0.2;
    wrapper.addChild(label);

    // Status label
    const statusLabel = new PIXI.Text({
      text: '',
      style: { fontFamily: 'Courier New', fontSize: 9, fill: 0x8899aa, align: 'center' },
    });
    statusLabel.anchor.set(0.5, 0);
    statusLabel.label = 'statusLabel';
    statusLabel.y = tex.height * CRAB_SCALE * 0.2;
    wrapper.addChild(statusLabel);

    // Animation state
    wrapper._anim = {
      state: 'idle',
      frame: 0,
      walkDir: 1,
      walkSpeed: 0.3 + Math.random() * 0.2,
      bobPhase: Math.random() * Math.PI * 2,
      targetX: 0,
      spawnProgress: 0,
      currentTexture: 'idle',
    };

    return wrapper;
  }

  // --- Switch crab sprite texture based on state ---
  function setCrabTexture(wrapper, stateName) {
    const sprite = wrapper.children.find(c => c.label === 'crabSprite');
    if (!sprite) return;

    const texName = stateName === 'completed' ? 'celebrating' : (stateName || 'idle');
    const tex = textures[texName] || textures.idle;
    if (tex && sprite.texture !== tex) {
      sprite.texture = tex;
      wrapper._anim.currentTexture = texName;
    }
  }

  // --- Animation Loop ---
  let frameCount = 0;

  app.ticker.add(() => {
    frameCount++;
    const dt = app.ticker.deltaTime;

    for (const [id, wrapper] of Object.entries(crabSprites)) {
      const anim = wrapper._anim;
      const sprite = wrapper.children.find(c => c.label === 'crabSprite');
      anim.frame += dt;

      if (anim.state === 'spawning') {
        setCrabTexture(wrapper, 'spawning');
        anim.spawnProgress = Math.min(1, anim.spawnProgress + 0.01 * dt);
        const sandY = app.screen.height * SAND_Y_RATIO;
        wrapper.y = sandY - 10 + (1 - anim.spawnProgress) * 40;
        wrapper.alpha = anim.spawnProgress;

        if (anim.spawnProgress >= 1) {
          anim.state = agents[id]?.state === 'working' ? 'working' : 'idle';
        }
      } else if (anim.state === 'idle') {
        setCrabTexture(wrapper, 'idle');
        // Gentle bob + slow walk
        const bob = Math.sin(anim.frame * 0.03 + anim.bobPhase) * 2;
        wrapper.y = app.screen.height * SAND_Y_RATIO - 10 + bob;

        if (Math.random() < 0.003) {
          anim.walkDir *= -1;
          if (sprite) sprite.scale.x = CRAB_SCALE * anim.walkDir;
        }

        wrapper.x += anim.walkDir * anim.walkSpeed * 0.3 * dt;

        const margin = 60;
        if (wrapper.x < margin) { anim.walkDir = 1; if (sprite) sprite.scale.x = CRAB_SCALE; }
        if (wrapper.x > app.screen.width - margin) { anim.walkDir = -1; if (sprite) sprite.scale.x = -CRAB_SCALE; }

      } else if (anim.state === 'working') {
        setCrabTexture(wrapper, 'working');
        // Busy hammering bob + slight rock
        const bob = Math.sin(anim.frame * 0.08) * 1;
        wrapper.y = app.screen.height * SAND_Y_RATIO - 10 + bob;

        if (sprite) sprite.rotation = Math.sin(anim.frame * 0.1) * 0.03;

      } else if (anim.state === 'completed') {
        setCrabTexture(wrapper, 'completed');
        // Victory bounce
        const bounce = Math.abs(Math.sin(anim.frame * 0.12)) * 15;
        wrapper.y = app.screen.height * SAND_Y_RATIO - 10 - bounce;

        if (sprite) sprite.rotation = Math.sin(anim.frame * 0.15) * 0.2;

        if (anim.frame > anim.danceStart + 200) {
          anim.state = 'idle';
          if (sprite) sprite.rotation = 0;
        }
      }

      // Update status label — prefer live phase over file-based state
      const statusLabel = wrapper.children.find(c => c.label === 'statusLabel');
      if (statusLabel && agents[id]) {
        const a = agents[id];
        const display = a.phase || a.state || '';
        statusLabel.text = display;
        statusLabel.style.fill =
          display === 'thinking' ? 0x60a0ff :
          display === 'editing' ? 0xe0c050 :
          display === 'running' ? 0xff9050 :
          display === 'working' ? 0xe0c050 :
          display === 'error' ? 0xff4040 :
          display === 'finished' ? 0x50c878 :
          display === 'idle' ? 0x8899aa :
          display === 'spawning' || display === 'starting' ? 0x50c878 :
          0x8899aa;
      }
    }

    // Animate water ripples
    for (const child of bgLayer.children) {
      if (child.label === 'ripple') {
        child.x = Math.sin(frameCount * 0.005 + child.y * 0.01) * 3;
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

    for (const [id, agent] of Object.entries(agents)) {
      const wrapper = crabSprites[id];
      if (!wrapper) continue;

      const anim = wrapper._anim;
      // Use live phase if available, fall back to file-based state
      const phase = agent.phase;
      const newState = agent.state || 'idle';

      // Phase-driven animation: thinking/editing/running all map to 'working' anim
      const isActive = phase === 'thinking' || phase === 'editing' || phase === 'running'
        || newState === 'working';
      const isFinished = phase === 'finished'
        || (phase === 'idle' && anim.state === 'working')
        || (newState === 'idle' && anim.state === 'working');
      const isError = phase === 'error';

      if (isActive && anim.state !== 'working' && anim.state !== 'spawning') {
        anim.state = 'working';
        anim.frame = 0;
      } else if (isFinished) {
        anim.state = 'completed';
        anim.danceStart = anim.frame;
      } else if (isError && anim.state !== 'spawning') {
        anim.state = 'idle'; // Show as idle but status label will show error color
      } else if (newState === 'spawning' && anim.state !== 'spawning') {
        anim.state = 'spawning';
        anim.spawnProgress = 0;
      }
    }

    layoutCrabs();
  }

  // --- Task Board UI ---
  function normalizeTaskStatus(status) {
    return String(status || '').toLowerCase();
  }

  function renderTaskBoard() {
    const board = document.getElementById('task-board');
    if (tasks.length === 0) {
      board.innerHTML = '<div style="color: var(--text-dim); font-size: 12px;">No tasks yet</div>';
      return;
    }

    board.innerHTML = tasks.map(t => {
      const normalized = normalizeTaskStatus(t.status);
      const statusClass =
        normalized === 'completed' ? 'completed' :
        normalized === 'claimed' || normalized === 'in_progress' ? 'claimed' : 'open';
      const statusText =
        normalized === 'completed' ? 'done' :
        normalized === 'claimed' ? 'claimed' :
        normalized === 'in_progress' ? 'in progress' :
        normalized === 'blocked' ? 'blocked' : 'open';
      const agentLine = t.claimed_by ? `<div class="task-agent">${esc(t.claimed_by)}</div>` : '';
      const desc = t.description.length > 100 ? t.description.slice(0, 100) + '...' : t.description;

      return `<div class="task-card">
        <span class="task-id">${esc(t.id)}</span>
        <span class="task-status ${statusClass}">${statusText}</span>
        <div class="task-desc">${esc(desc)}</div>
        ${agentLine}
      </div>`;
    }).join('');
  }

  // --- Event Log ---
  function getEventKind(event) {
    return event?.type || event?.event_type || '';
  }

  function safeJsonSnippet(value, maxLen) {
    const limit = maxLen || 160;
    try {
      const seen = new WeakSet();
      const json = JSON.stringify(value, (k, v) => {
        if (v && typeof v === 'object') {
          if (seen.has(v)) return '[Circular]';
          seen.add(v);
        }
        return v;
      });
      if (!json) return '';
      return json.length > limit ? `${json.slice(0, limit)}…` : json;
    } catch {
      return '';
    }
  }

  function compactDetailFromData(data) {
    if (!data || typeof data !== 'object') return '';

    const preferredKeys = ['event_type', 'type', 'phase', 'task_id', 'last_activity', 'reason', 'status', 'agent_id'];
    const details = [];
    for (const key of preferredKeys) {
      const value = data[key];
      if (value === undefined || value === null || value === '') continue;
      details.push(`${key}=${String(value)}`);
      if (details.length >= 4) break;
    }

    if (details.length > 0) return details.join(' · ');

    const snippet = safeJsonSnippet(data, 180);
    return snippet;
  }

  function hasMeaningfulMessage(event) {
    const message = String(event?.message || '').trim();
    if (!message) return false;

    const kind = String(getEventKind(event) || '').trim().toLowerCase();
    const normalized = message.toLowerCase();

    if (kind && (normalized === kind || normalized === kind.replace(/[:_]/g, ' '))) {
      return false;
    }

    const genericMessages = new Set([
      'ok',
      'updated',
      'status update',
      'event received',
      'done',
    ]);

    return !genericMessages.has(normalized);
  }

  function addLogEntry(event) {
    const entries = document.getElementById('log-entries');
    const div = document.createElement('div');
    div.className = 'log-entry';

    const time = event.timestamp ? new Date(event.timestamp).toLocaleTimeString() : '';
    const agentPart = event.agent_id
      ? `<span class="log-agent">[${esc(event.agent_id)}]</span>` : '';

    const message = hasMeaningfulMessage(event) ? String(event.message).trim() : '';
    const detail = compactDetailFromData(event?.data);
    const textPart = message ? `<span class="log-text">${esc(message)}</span>` : '';
    const detailPart = detail ? `<span class="log-detail">${esc(detail)}</span>` : '';

    div.innerHTML =
      `<span class="log-time">${time}</span>` +
      `<span class="log-type">${esc(getEventKind(event) || '?')}</span>` +
      agentPart +
      textPart +
      detailPart;

    entries.appendChild(div);
    entries.scrollTop = entries.scrollHeight;
    while (entries.children.length > 400) entries.removeChild(entries.firstChild);
  }

  function normalizeEvent(event) {
    if (!event || typeof event !== 'object') return event;
    const kind = getEventKind(event);
    if (kind && !event.type) {
      event.type = kind;
    }
    return event;
  }

  async function loadEventHistory() {
    if (historyLoaded || !SESSION_ID) return;
    historyLoaded = true;

    try {
      const res = await fetch(EVENTS_API_URL);
      if (!res.ok) return;
      const events = await res.json();
      if (!Array.isArray(events)) return;

      for (const raw of events) {
        handleEvent(normalizeEvent(raw), { fromHistory: true });
      }

      // History alone does not imply completion; completion is only set from
      // explicit signals (snapshot/session status or session:completed event).
    } catch {
      // no-op: historical data is best effort when WS is unavailable
    }
  }

  // --- WebSocket ---
  function clearReconnectTimer() {
    if (!reconnectTimer) return;
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }

  function enterHistoryMode(reason) {
    // Keep fallback behavior centralized so onclose/onerror stay in sync.
    loadEventHistory();
    setHistoryMode(reason || 'websocket unavailable; showing cached history (live status unknown)');
  }

  function handleSocketOpen() {
    wsEverConnected = true;
    historyMode = false;
    setConnectionStatus('connected');
    setBanner('');
    clearReconnectTimer();
  }

  function handleSocketClose() {
    if (sessionCompleted) {
      setConnectionStatus('completed');
      return;
    }

    if (!wsEverConnected) {
      enterHistoryMode('websocket unavailable; showing cached history (live status unknown)');
    } else {
      setConnectionStatus('disconnected');
    }

    scheduleReconnect();
  }

  function handleSocketError() {
    if (sessionCompleted) return;

    if (!wsEverConnected) {
      enterHistoryMode('websocket unavailable; showing cached history (live status unknown)');
    } else {
      setConnectionStatus('disconnected');
    }
  }

  function connectWS() {
    if (!SESSION_ID || sessionCompleted) return;
    setConnectionStatus('connecting');

    try {
      ws = new WebSocket(WS_URL);
    } catch (e) {
      setConnectionStatus('disconnected');
      enterHistoryMode('websocket unavailable; showing cached history (live status unknown)');
      scheduleReconnect();
      return;
    }

    ws.onopen = handleSocketOpen;

    ws.onmessage = (evt) => {
      let data;
      try { data = JSON.parse(evt.data); } catch { return; }
      handleEvent(normalizeEvent(data));
    };

    ws.onclose = handleSocketClose;
    ws.onerror = handleSocketError;
  }

  function scheduleReconnect() {
    if (sessionCompleted || reconnectTimer) return;
    reconnectTimer = setTimeout(() => { reconnectTimer = null; connectWS(); }, 3000);
  }

  function setConnectionStatus(state) {
    const el = document.getElementById('connection-status');
    el.textContent = state;
    el.className = state;
  }

  function setBanner(message) {
    const banner = document.getElementById('session-completed-banner');
    if (!banner) return;

    if (!message) {
      banner.classList.add('hidden');
      banner.classList.remove('history');
      return;
    }

    banner.classList.remove('hidden');
    banner.textContent = message;
  }

  function closeSocketIfOpen() {
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.close();
    }
  }

  function setHistoryMode(reason) {
    if (sessionCompleted) return;
    historyMode = true;
    setConnectionStatus('history');
    const banner = document.getElementById('session-completed-banner');
    if (banner) banner.classList.add('history');
    setBanner(`History mode — ${reason || 'showing cached timeline while live status is unknown'}`);
  }

  function setSessionCompleted(reason) {
    sessionCompleted = true;
    historyMode = false;
    clearReconnectTimer();
    closeSocketIfOpen();

    setConnectionStatus('completed');
    const banner = document.getElementById('session-completed-banner');
    if (banner) banner.classList.remove('history');
    setBanner(`Session completed — ${reason || 'session finished'}`);
  }

  // --- Event Handling ---
  function handleEvent(event, opts) {
    const options = opts || {};
    event = normalizeEvent(event);
    const eventKind = getEventKind(event);

    switch (eventKind) {
      case 'snapshot':
        if (event.agents) agents = event.agents;
        if (event.tasks) tasks = event.tasks;
        if (event.config) {
          config = event.config;
          document.getElementById('task-label').textContent = config.task || '';
        }
        if (event.session_active === false || event.session_status === 'completed') {
          setSessionCompleted('tmux session ended');
        }
        syncCrabs();
        renderTaskBoard();
        break;

      case 'agent:status':
        if (event.agent_id && event.data) {
          agents[event.agent_id] = { ...agents[event.agent_id], ...event.data };
          syncCrabs();
        }
        addLogEntry(event);
        break;

      case 'agent:phase':
        if (event.agent_id && event.data) {
          if (!agents[event.agent_id]) agents[event.agent_id] = {};
          agents[event.agent_id].phase = event.data.phase;
          agents[event.agent_id].last_activity = event.data.last_activity;
          syncCrabs();
        }
        addLogEntry(event);
        break;

      case 'task:claimed':
      case 'task_claimed':
        if (event.task_id) {
          const task = tasks.find(t => t.id === event.task_id);
          if (task) { task.status = 'claimed'; task.claimed_by = event.agent_id || null; }
          renderTaskBoard();
        }
        addLogEntry(event);
        break;

      case 'task:completed':
      case 'task_completed':
        if (event.task_id) {
          const task = tasks.find(t => t.id === event.task_id);
          if (task) { task.status = 'completed'; task.completed_at = event.timestamp; }
          renderTaskBoard();
        }
        addLogEntry(event);
        break;

      case 'task:updated':
        if (event.tasks) { tasks = event.tasks; renderTaskBoard(); }
        addLogEntry(event);
        break;

      case 'team:spawned':
      case 'team_spawned':
      case 'agent:summary':
      case 'agent_summary':
        addLogEntry(event);
        break;

      case 'session:completed':
        setSessionCompleted((event.data && event.data.reason) || 'session finished');
        addLogEntry(event);
        break;

      case 'event:raw':
        if (event.data) {
          const rawType = getEventKind(event.data);
          if (rawType === 'task_claimed' && event.agent_id) {
            const agent = agents[event.agent_id];
            if (agent) { agent.state = 'working'; agent.current_task = event.task_id; syncCrabs(); }
          } else if (rawType === 'task_completed' && event.agent_id) {
            const agent = agents[event.agent_id];
            if (agent) { agent.state = 'idle'; agent.current_task = null; syncCrabs(); }
          }
        }
        addLogEntry(event);
        break;

      default:
        addLogEntry(event);
        break;
    }

    // Apply some state on historical events too
    if (options.fromHistory && eventKind === 'agent:status' && event.agent_id && event.data) {
      agents[event.agent_id] = { ...agents[event.agent_id], ...event.data };
      syncCrabs();
    }
  }

  function esc(str) {
    const d = document.createElement('div');
    d.textContent = String(str || '');
    return d.innerHTML;
  }

  window.addEventListener('resize', () => {
    requestAnimationFrame(() => { drawBackground(); layoutCrabs(); });
  });

  // --- Init ---
  drawBackground();

  const sessionEl = document.getElementById('session-id');
  if (sessionEl && SESSION_ID) {
    sessionEl.textContent = SESSION_ID;
    document.title = `Grotto — ${SESSION_ID}`;
  }

  connectWS();

})();
