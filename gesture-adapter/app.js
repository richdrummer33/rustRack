'use strict';

const SVG_NS = 'http://www.w3.org/2000/svg';
const PARAM_R = 14;
const PORT_R  = 10;
const TAP_PX = 6;
const TAP_MS = 250;
const LONG_PRESS_MS = 600;
const ZOOM_EMIT_MS = 50;

const params = new URLSearchParams(location.search);
const MOCK = params.get('mock') === '1';
const HOST = params.get('host') || location.hostname || 'localhost';
const PORT = params.get('port') || '54323';

const sceneEl  = document.getElementById('scene');
const statusEl = document.getElementById('status');
const modeEl   = document.getElementById('mode');
const logEl    = document.getElementById('log');
const logPanel = document.getElementById('logPanel');
const sheetEl  = document.getElementById('actionSheet');
const sheetHeader = document.getElementById('actionHeader');

let scene = null;
let ws = null;
let seqCounter = 0;
const pointers = new Map();
let pinch = null;
let cablePreview = null;
const elIndex = new Map();
const panelAssets = new Map();   // "pluginSlug/modelSlug" -> SVG string

function el(tag, attrs = {}, text) {
  const e = document.createElementNS(SVG_NS, tag);
  for (const k in attrs) e.setAttribute(k, attrs[k]);
  if (text != null) e.textContent = text;
  return e;
}

function setStatus(klass, text) {
  statusEl.className = 'status ' + klass;
  statusEl.textContent = text;
}

function logLine(dir, payload) {
  const li = document.createElement('li');
  li.className = dir;
  li.textContent = (dir === 'out' ? '→ ' : dir === 'err' ? '! ' : '← ') +
    (typeof payload === 'string' ? payload : JSON.stringify(payload));
  logEl.insertBefore(li, logEl.firstChild);
  while (logEl.children.length > 200) logEl.removeChild(logEl.lastChild);
}

document.getElementById('toggleLog').addEventListener('click', () => {
  logPanel.hidden = !logPanel.hidden;
});

function connect() {
  if (MOCK) {
    modeEl.textContent = 'mock';
    setStatus('status-mock', 'mock');
    fetch('../protocol/mock-snapshot.json')
      .then(r => r.json())
      .then(s => applySnapshot(s))
      .catch(e => logLine('err', 'mock load failed: ' + e.message));
    return;
  }
  modeEl.textContent = `${HOST}:${PORT}`;
  setStatus('status-disconnected', 'connecting…');
  ws = new WebSocket(`ws://${HOST}:${PORT}/`);
  ws.onopen = () => {
    setStatus('status-connected', 'connected');
    ws.send(JSON.stringify({ op: 'hello', v: 1, client: 'web-prototype' }));
  };
  ws.onmessage = ev => {
    let msg; try { msg = JSON.parse(ev.data); } catch { return; }
    if (msg.op === 'snapshot') applySnapshot(msg);
    else if (msg.op === 'module-asset') applyAsset(msg);
    else if (msg.op === 'ack') {} // silent
    else if (msg.op === 'err') logLine('err', msg);
    else logLine('in', msg);
  };
  ws.onclose = () => {
    setStatus('status-disconnected', 'disconnected');
    setTimeout(connect, 1500);
  };
  ws.onerror = () => {};
}

function applyAsset(msg) {
  if (!msg.pluginSlug || !msg.modelSlug || msg.format !== 'svg') return;
  const key = `${msg.pluginSlug}/${msg.modelSlug}`;
  panelAssets.set(key, msg.data || '');
  if (scene) render();
}

function applySnapshot(s) {
  scene = s;
  render();
}

function panelKey(m) {
  if (!m.pluginSlug || !m.modelSlug) return null;
  return `${m.pluginSlug}/${m.modelSlug}`;
}

function parseSvgString(svgText) {
  const doc = new DOMParser().parseFromString(svgText, 'image/svg+xml');
  const root = doc.documentElement;
  if (!root || root.nodeName.toLowerCase() !== 'svg') return null;
  return document.importNode(root, true);
}

function render() {
  if (!scene) return;
  sceneEl.setAttribute('viewBox',
    `0 0 ${scene.window.w} ${scene.window.h}`);
  while (sceneEl.firstChild) sceneEl.removeChild(sceneEl.firstChild);
  elIndex.clear();

  for (const m of scene.modules) {
    const [x, y, w, h] = m.screenBox;
    const key = panelKey(m);
    const hasPanel = key && panelAssets.has(key);

    // 1. Rect underneath: visually outlined when no panel, fully
    //    transparent when we're overlaying real SVG artwork.
    const rectClasses = ['module'];
    if (hasPanel) rectClasses.push('has-panel');
    if (m.bypassed) rectClasses.push('bypassed');
    sceneEl.appendChild(el('rect',
      { x, y, width: w, height: h, rx: 4,
        class: rectClasses.join(' '),
        'data-id': `module:${m.id}` }));

    // 2. Panel SVG, scaled into the module's screenBox. Pointer events
    //    disabled — touch routing still works off snapshot coords.
    if (hasPanel) {
      const inner = parseSvgString(panelAssets.get(key));
      if (inner) {
        inner.setAttribute('x', x);
        inner.setAttribute('y', y);
        inner.setAttribute('width', w);
        inner.setAttribute('height', h);
        inner.setAttribute('preserveAspectRatio', 'none');
        inner.setAttribute('class', 'panel');
        sceneEl.appendChild(inner);
      }
    }

    // 3. Module name label when no panel is rendered (otherwise it
    //    would float on top of real artwork).
    const labelClass = 'mod-label' + (hasPanel ? ' has-panel' : '');
    sceneEl.appendChild(el('text',
      { x: x + w/2, y: y + 22, 'text-anchor': 'middle', class: labelClass },
      m.name));

    // 4. Knob and jack hit-zone markers, drawn on top.
    for (const p of m.params) {
      const id = `param:${m.id}:${p.id}`;
      const c = el('circle',
        { cx: p.x, cy: p.y, r: PARAM_R, class: 'param', 'data-id': id });
      sceneEl.appendChild(c);
      elIndex.set(id, c);
    }
    for (const port of m.inputs) {
      const id = `input:${m.id}:${port.id}`;
      const c = el('circle',
        { cx: port.x, cy: port.y, r: PORT_R, class: 'input', 'data-id': id });
      sceneEl.appendChild(c);
      elIndex.set(id, c);
    }
    for (const port of m.outputs) {
      const id = `output:${m.id}:${port.id}`;
      const c = el('circle',
        { cx: port.x, cy: port.y, r: PORT_R, class: 'output', 'data-id': id });
      sceneEl.appendChild(c);
      elIndex.set(id, c);
    }
  }
}

function svgPt(ev) {
  const pt = sceneEl.createSVGPoint();
  pt.x = ev.clientX;
  pt.y = ev.clientY;
  const ctm = sceneEl.getScreenCTM();
  if (!ctm) return { x: 0, y: 0 };
  return pt.matrixTransform(ctm.inverse());
}

function dist(ax, ay, bx, by) {
  return Math.hypot(ax - bx, ay - by);
}

function hitTest(x, y) {
  if (!scene) return null;
  for (const m of scene.modules) {
    for (const p of m.params)
      if (dist(x, y, p.x, p.y) <= PARAM_R * 1.4)
        return { kind: 'param', moduleId: m.id, paramId: p.id };
    for (const port of m.inputs)
      if (dist(x, y, port.x, port.y) <= PORT_R * 1.6)
        return { kind: 'input', moduleId: m.id, portId: port.id };
    for (const port of m.outputs)
      if (dist(x, y, port.x, port.y) <= PORT_R * 1.6)
        return { kind: 'output', moduleId: m.id, portId: port.id };
  }
  for (const m of scene.modules) {
    const [x0, y0, w, h] = m.screenBox;
    if (x >= x0 && x <= x0+w && y >= y0 && y <= y0+h)
      return { kind: 'module', moduleId: m.id, name: m.name };
  }
  return null;
}

function send(intent) {
  intent.op = 'intent';
  intent.seq = ++seqCounter;
  if (MOCK) {
    logLine('out', intent);
    return;
  }
  if (ws && ws.readyState === 1) {
    ws.send(JSON.stringify(intent));
    logLine('out', intent);
  } else {
    logLine('err', 'not connected, dropped: ' + JSON.stringify(intent));
  }
}

function highlight(hit, on) {
  if (!hit) return;
  const id = hit.kind === 'module'
    ? `module:${hit.moduleId}`
    : hit.kind === 'param'
    ? `param:${hit.moduleId}:${hit.paramId}`
    : `${hit.kind}:${hit.moduleId}:${hit.portId}`;
  const node = elIndex.get(id);
  if (node) node.classList.toggle('active', !!on);
}

function setCablePreview(x1, y1, x2, y2) {
  if (!cablePreview) {
    cablePreview = el('line', { class: 'preview-cable' });
    sceneEl.appendChild(cablePreview);
  }
  cablePreview.setAttribute('x1', x1);
  cablePreview.setAttribute('y1', y1);
  cablePreview.setAttribute('x2', x2);
  cablePreview.setAttribute('y2', y2);
}
function clearCablePreview() {
  if (cablePreview) {
    cablePreview.remove();
    cablePreview = null;
  }
}

function portRefFromHit(hit) {
  return { moduleId: hit.moduleId, port: hit.kind, portId: hit.portId };
}

function moduleById(id) {
  return scene ? scene.modules.find(m => m.id === id) : null;
}

function openActionSheet(moduleId) {
  const m = moduleById(moduleId);
  if (!m) return;
  sheetHeader.textContent = m.name ? `${m.name}` : `Module ${moduleId}`;
  // Reflect bypass state in label so the user knows what tapping it does.
  const bypassBtn = sheetEl.querySelector('button[data-action="bypass"]');
  if (bypassBtn) bypassBtn.textContent = m.bypassed ? 'Un-bypass' : 'Bypass';
  sheetEl.dataset.moduleId = String(moduleId);
  sheetEl.hidden = false;
}

function closeActionSheet() {
  sheetEl.hidden = true;
  delete sheetEl.dataset.moduleId;
}

sheetEl.addEventListener('click', ev => {
  const btn = ev.target.closest('button.action');
  if (!btn) {
    // Clicked the backdrop.
    if (ev.target.classList.contains('action-backdrop')) closeActionSheet();
    return;
  }
  const action = btn.dataset.action;
  const moduleId = Number(sheetEl.dataset.moduleId);
  closeActionSheet();
  if (action && !Number.isNaN(moduleId)) {
    send({ kind: 'module-action', action, moduleId });
  }
});

sceneEl.addEventListener('pointerdown', ev => {
  ev.preventDefault();
  sceneEl.setPointerCapture(ev.pointerId);

  const pt = svgPt(ev);
  const hit = hitTest(pt.x, pt.y);
  const s = {
    x0: pt.x, y0: pt.y, t0: performance.now(),
    x: pt.x, y: pt.y, hit, moved: false,
    consumed: false, lastPanX: pt.x, lastPanY: pt.y,
    longTimer: null,
  };
  pointers.set(ev.pointerId, s);
  highlight(hit, true);

  s.longTimer = setTimeout(() => {
    if (pointers.has(ev.pointerId) && !s.moved && !s.consumed && s.hit) {
      if (s.hit.kind === 'module') {
        openActionSheet(s.hit.moduleId);
      } else {
        send({ kind: 'context', target: s.hit });
      }
      s.consumed = true;
    }
  }, LONG_PRESS_MS);

  if (pointers.size === 2) initPinch();
});

sceneEl.addEventListener('pointermove', ev => {
  const s = pointers.get(ev.pointerId);
  if (!s) return;
  const pt = svgPt(ev);
  s.x = pt.x; s.y = pt.y;
  if (!s.moved && dist(s.x, s.y, s.x0, s.y0) > TAP_PX) s.moved = true;

  if (pinch) { updatePinch(); return; }
  if (s.consumed || !s.moved) return;

  if (s.hit && (s.hit.kind === 'input' || s.hit.kind === 'output')) {
    setCablePreview(s.x0, s.y0, s.x, s.y);
    return;
  }
  if (s.hit && s.hit.kind === 'param') {
    return;
  }
  const dx = s.x - s.lastPanX;
  const dy = s.y - s.lastPanY;
  s.lastPanX = s.x; s.lastPanY = s.y;
  if (Math.abs(dx) > 0.5 || Math.abs(dy) > 0.5) {
    send({ kind: 'pan', dx, dy });
  }
});

function finishPointer(ev) {
  const s = pointers.get(ev.pointerId);
  if (!s) return;
  clearTimeout(s.longTimer);
  highlight(s.hit, false);

  const dt = performance.now() - s.t0;

  if (pinch) {
    endPinch();
  } else if (!s.consumed) {
    if (!s.moved && dt < TAP_MS) {
      if (s.hit) send({ kind: 'tap', target: s.hit });
    } else if (s.hit && (s.hit.kind === 'input' || s.hit.kind === 'output')) {
      const release = hitTest(s.x, s.y);
      if (release && (release.kind === 'input' || release.kind === 'output') &&
          !(release.moduleId === s.hit.moduleId && release.portId === s.hit.portId &&
            release.kind === s.hit.kind)) {
        send({
          kind: 'cable',
          from: portRefFromHit(s.hit),
          to:   portRefFromHit(release),
        });
      }
      clearCablePreview();
    } else if (s.hit && s.hit.kind === 'param' && s.moved) {
      const deltaPx = -(s.y - s.y0);
      send({
        kind: 'knob-drag',
        moduleId: s.hit.moduleId,
        paramId: s.hit.paramId,
        deltaPx,
        fine: false,
      });
    }
  }

  pointers.delete(ev.pointerId);
  if (pointers.size < 2 && pinch) endPinch();
}

sceneEl.addEventListener('pointerup', finishPointer);
sceneEl.addEventListener('pointercancel', ev => {
  clearCablePreview();
  finishPointer(ev);
});
sceneEl.addEventListener('contextmenu', e => e.preventDefault());

function initPinch() {
  const ps = [...pointers.values()];
  if (ps.length !== 2) return;
  pinch = {
    d0: dist(ps[0].x, ps[0].y, ps[1].x, ps[1].y),
    cx: (ps[0].x + ps[1].x) / 2,
    cy: (ps[0].y + ps[1].y) / 2,
    lastD: dist(ps[0].x, ps[0].y, ps[1].x, ps[1].y),
    lastEmit: 0,
  };
  for (const s of pointers.values()) {
    s.consumed = true;
    clearTimeout(s.longTimer);
    highlight(s.hit, false);
  }
  clearCablePreview();
}

function updatePinch() {
  if (!pinch) return;
  const ps = [...pointers.values()];
  if (ps.length !== 2) return;
  const d = dist(ps[0].x, ps[0].y, ps[1].x, ps[1].y);
  const now = performance.now();
  if (now - pinch.lastEmit < ZOOM_EMIT_MS) return;
  const factor = d / pinch.lastD;
  if (Math.abs(factor - 1) < 0.005) return;
  pinch.lastD = d;
  pinch.lastEmit = now;
  send({
    kind: 'zoom',
    factor,
    anchor: { x: pinch.cx, y: pinch.cy },
  });
}

function endPinch() { pinch = null; }

connect();
