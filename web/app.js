(() => {
  const canvas = document.querySelector('#paper');
  const ctx = canvas.getContext('2d', { alpha: true, desynchronized: true });
  const hint = document.querySelector('#hint');
  const blot = document.querySelector('#blot');
  const fs = document.querySelector('#fullscreen');
  const setup = document.querySelector('#setup');
  const setupForm = document.querySelector('#setup-form');
  const apiKey = document.querySelector('#api-key');
  const setupError = document.querySelector('#setup-error');
  let strokes = [], current = null, timer = 0, busy = false, dpr = 1;
  let replyCursorY = null, replyResting = false, replyFadeTimer = 0;
  let last = null;

  function size() {
    const old = document.createElement('canvas');
    old.width = canvas.width; old.height = canvas.height;
    if (canvas.width) old.getContext('2d').drawImage(canvas, 0, 0);
    dpr = Math.min(devicePixelRatio || 1, 2);
    canvas.width = Math.round(innerWidth * dpr);
    canvas.height = Math.round(innerHeight * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.lineCap = 'round'; ctx.lineJoin = 'round';
    if (old.width) ctx.drawImage(old, 0, 0, old.width, old.height, 0, 0, innerWidth, innerHeight);
  }
  addEventListener('resize', size); size();

  async function configure(force = false) {
    if (!force) {
      try {
        const status = await fetch('/api/config').then(r => r.json());
        if (status.ready) return;
      } catch (_) {}
    }
    setup.hidden = false;
    setTimeout(() => apiKey.focus(), 80);
  }
  configure();
  resumePending();
  async function resumePending() {
    const job = sessionStorage.getItem('riddlePendingJob');
    if (!job) return;
    let handedOff = false;
    busy = true; replyCursorY = null; hint.style.opacity = '0'; blot.classList.add('thinking');
    try {
      await pollJob(job);
      sessionStorage.removeItem('riddlePendingJob');
      blot.classList.remove('thinking');
      settleReply(5000); handedOff = true;
    } catch (err) {
      sessionStorage.removeItem('riddlePendingJob');
      console.error(err);
    } finally {
      if (!handedOff) resetPage();
    }
  }
  setupForm.addEventListener('submit', async e => {
    e.preventDefault(); setupError.textContent = '';
    const button = document.querySelector('#save-key'); button.disabled = true;
    try {
      const res = await fetch('/api/config', {
        method:'POST', headers:{'Content-Type':'application/octet-stream','X-Riddle-Setup':'1'}, body:apiKey.value.trim()
      });
      if (!res.ok) throw new Error('The key does not look right.');
      setup.hidden = true; apiKey.value = ''; hint.style.opacity = '1';
    } catch (err) { setupError.textContent = err.message || 'The secret could not be kept.'; }
    finally { button.disabled = false; }
  });

  const point = e => ({ x: e.clientX, y: e.clientY, p: e.pressure || .38 });
  canvas.addEventListener('pointerdown', e => {
    if (busy || (e.pointerType === 'mouse' && e.button !== 0)) return;
    e.preventDefault(); canvas.setPointerCapture(e.pointerId);
    if (replyResting) clearRestingReply();
    clearTimeout(timer); hint.style.opacity = '0';
    current = []; strokes.push(current); last = point(e); current.push(last);
    ctx.fillStyle = '#25231d'; ctx.beginPath(); ctx.arc(last.x, last.y, 1.4 + last.p * 2.8, 0, Math.PI * 2); ctx.fill();
  });
  canvas.addEventListener('pointermove', e => {
    if (!current || busy) return;
    e.preventDefault();
    const events = e.getCoalescedEvents ? e.getCoalescedEvents() : [e];
    for (const ev of events) {
      const p = point(ev), width = 2.1 + p.p * 4.4;
      ctx.strokeStyle = '#25231d'; ctx.lineWidth = width;
      ctx.beginPath(); ctx.moveTo(last.x, last.y); ctx.lineTo(p.x, p.y); ctx.stroke();
      current.push(p); last = p;
    }
  });
  const up = e => {
    if (!current) return;
    e.preventDefault(); current = null; last = null;
    timer = setTimeout(commit, 2800);
  };
  canvas.addEventListener('pointerup', up); canvas.addEventListener('pointercancel', up);

  async function commit() {
    if (busy || !strokes.length) return;
    busy = true; replyCursorY = null;
    let handedOff = false;
    const png = await makePageBlob();
    const drinking = drinkInk();
    try {
      // Upload quickly, then poll short-lived requests. HarmonyOS browsers may
      // kill one long streaming connection while the model is thinking.
      const request = fetch('/api/ask', { method: 'POST', headers: {'Content-Type':'image/png'}, body: png });
      await drinking;
      const res = await request;
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const job = (await res.text()).trim();
      sessionStorage.setItem('riddlePendingJob', job);
      await pollJob(job);
      sessionStorage.removeItem('riddlePendingJob');
      blot.classList.remove('thinking');
      settleReply(Math.min(18000, 4500 + strokes.length * 500));
      handedOff = true;
    } catch (e) {
      blot.classList.remove('thinking');
      await writeReply('The ink will not answer tonight.');
      settleReply(5000); handedOff = true;
      console.error(e);
    } finally {
      if (!handedOff) resetPage();
    }
  }

  async function makePageBlob() {
    const points = strokes.flat();
    const pad = 32;
    let bx0 = innerWidth, by0 = innerHeight, bx1 = 0, by1 = 0;
    for (const p of points) {
      bx0 = Math.min(bx0, p.x); by0 = Math.min(by0, p.y);
      bx1 = Math.max(bx1, p.x); by1 = Math.max(by1, p.y);
    }
    const minX = Math.max(0, bx0 - pad), minY = Math.max(0, by0 - pad);
    const maxX = Math.min(innerWidth, bx1 + pad), maxY = Math.min(innerHeight, by1 + pad);
    const cropW = Math.max(1, maxX - minX), cropH = Math.max(1, maxY - minY);
    const scale = Math.min(1, 1200 / Math.max(cropW, cropH));
    const page = document.createElement('canvas');
    page.width = Math.max(1, Math.round(cropW * scale));
    page.height = Math.max(1, Math.round(cropH * scale));
    const pageCtx = page.getContext('2d');
    pageCtx.fillStyle = '#ffffff';
    pageCtx.fillRect(0, 0, page.width, page.height);
    pageCtx.drawImage(
      canvas,
      minX * dpr, minY * dpr, cropW * dpr, cropH * dpr,
      0, 0, page.width, page.height
    );
    return new Promise(resolve => page.toBlob(resolve, 'image/png'));
  }

  async function pollJob(job) {
    let seen = 0, finished = false, waited = 0;
    const queue = [];
    let writing = false;
    const writeNext = async () => {
      if (writing || !queue.length) return;
      writing = true;
      while (queue.length) await writeReply(queue.shift());
      writing = false;
    };
    while (!finished && waited < 240000) {
      const res = await fetch(`/api/job/${encodeURIComponent(job)}?t=${Date.now()}`, {cache:'no-store'});
      if (res.status === 404) throw new Error('The remembered answer has faded.');
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const events = (await res.text()).split('\n').filter(Boolean).map(line => JSON.parse(line));
      for (const ev of events.slice(seen)) {
        if (ev.type === 'ink' || ev.type === 'error') { queue.push(ev.text); writeNext(); }
        if (ev.type === 'done') finished = true;
      }
      seen = events.length;
      if (!finished) { await delay(650); waited += 650; }
    }
    if (!finished) throw new Error('The diary took too long to answer.');
    while (writing || queue.length) { writeNext(); await delay(80); }
  }

  function drinkInk() {
    return canvas.animate([{filter:'blur(0)', opacity:1}, {filter:'blur(1.8px)',opacity:.38}, {filter:'blur(3px)',opacity:0}],
      {duration:1100, fill:'forwards', easing:'ease-in'}).finished.then(() => {
        ctx.clearRect(0, 0, innerWidth, innerHeight); canvas.style.opacity = '1';
        canvas.getAnimations().forEach(a => a.cancel()); blot.classList.add('thinking');
      });
  }

  async function writeReply(text) {
    blot.classList.remove('thinking');
    const hasHan = /[\u3400-\u9fff]/.test(text);
    const family = hasHan ? 'RiddleCJK' : 'Riddle';
    const fontSize = Math.max(34, Math.min(hasHan ? 52 : 58, innerWidth / (hasHan ? 20 : 18)));
    await document.fonts.load(`${fontSize}px ${family}`);
    const max = Math.min(innerWidth * .78, 980);
    ctx.font = `${fontSize}px ${family}`; ctx.fillStyle = '#29271f'; ctx.strokeStyle = '#29271f';
    ctx.lineWidth = 1.15; ctx.lineCap = 'round';
    const lines = wrap(text, max), lineH = fontSize * (hasHan ? 1.38 : 1.22);
    let y = replyCursorY ?? Math.max(90, (innerHeight - lines.length * lineH) * .32);
    for (const line of lines) {
      const width = ctx.measureText(line).width, x = (innerWidth - width) / 2;
      // Fine-grained clipping makes each connected word appear as if a nib is crossing it.
      const steps = Math.max(20, Math.ceil(width / 5));
      for (let i = 1; i <= steps; i++) {
        ctx.save(); ctx.beginPath(); ctx.rect(x - 3, y - fontSize, width * i / steps + 5, lineH + 10); ctx.clip();
        ctx.clearRect(x - 4, y - fontSize - 3, width + 8, lineH + 14);
        ctx.fillText(line, x, y); ctx.strokeText(line, x, y); ctx.restore();
        await delay(12);
      }
      y += lineH;
    }
    replyCursorY = y;
  }

  function settleReply(ms) {
    busy = false; strokes = []; current = null; replyResting = true;
    clearTimeout(replyFadeTimer);
    replyFadeTimer = setTimeout(async () => {
      if (!replyResting) return;
      replyResting = false;
      await fadeCanvas(1200);
      resetPage();
    }, ms);
  }

  function clearRestingReply() {
    clearTimeout(replyFadeTimer); replyResting = false; replyCursorY = null;
    canvas.getAnimations().forEach(a => a.cancel());
    canvas.style.opacity = '1'; ctx.clearRect(0, 0, innerWidth, innerHeight);
  }

  function resetPage() {
    clearTimeout(replyFadeTimer); replyResting = false; replyCursorY = null;
    blot.classList.remove('thinking'); canvas.getAnimations().forEach(a => a.cancel());
    ctx.clearRect(0, 0, innerWidth, innerHeight); canvas.style.opacity = '1';
    strokes = []; current = null; busy = false; hint.style.opacity = '1';
  }

  function wrap(text, max) {
    const out = [];
    for (const para of String(text).split(/\n+/)) {
      let line = '';
      // Keeps Chinese readable while still grouping Latin words.
      const tokens = /[\u3400-\u9fff]|[^\s\u3400-\u9fff]+|\s+/g;
      for (const token of para.match(tokens) || []) {
        const next = line + token;
        if (line.trim() && ctx.measureText(next).width > max) { out.push(line.trim()); line = token.trimStart(); }
        else line = next;
      }
      if (line.trim()) out.push(line.trim());
    }
    return out.length ? out : ['…'];
  }
  const delay = ms => new Promise(r => setTimeout(r, ms));
  async function fadeCanvas(ms) {
    const a = canvas.animate([{opacity:1, filter:'blur(0)'},{opacity:0,filter:'blur(2px)'}], {duration:ms,fill:'forwards'});
    await a.finished;
  }
  let hold = 0, held = false;
  fs.addEventListener('pointerdown', () => { held = false; hold = setTimeout(() => { held = true; configure(true); }, 1200); });
  fs.addEventListener('pointerup', () => clearTimeout(hold));
  fs.addEventListener('pointercancel', () => clearTimeout(hold));
  fs.addEventListener('click', async () => {
    if (held) return;
    try { if (!document.fullscreenElement) await document.documentElement.requestFullscreen(); else await document.exitFullscreen(); } catch (_) {}
  });
})();
