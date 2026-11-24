/* Bancada — interface. O encaixe e a biblioteca rodam no núcleo Rust via IPC
   (window.__TAURI__). No navegador, a aba Montar DTF só desenha a prévia. */

import Tesseract from 'tesseract.js';
import * as pdfjs from 'pdfjs-dist';
import pdfWorkerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url';
import * as PDFLib from 'pdf-lib';

pdfjs.GlobalWorkerOptions.workerSrc = pdfWorkerUrl;

const TAURI = window.__TAURI__ || null;
const invoke = TAURI ? TAURI.core.invoke : null;

if (!TAURI) document.querySelector('#aviso-navegador').hidden = false;

const $ = (sel) => document.querySelector(sel);

// Janela nativa do Windows para escolher pasta (§3 da especificação).
// No navegador (sem o app de desktop) retorna null.
async function escolherPasta(titulo) {
  if (!TAURI) return null;
  return await invoke('escolher_pasta', { titulo });
}

/* ------------------------------ Abas ------------------------------ */

document.querySelectorAll('#abas button').forEach((b) => {
  b.addEventListener('click', () => {
    document.querySelectorAll('#abas button').forEach((x) => x.classList.toggle('ativa', x === b));
    document.querySelectorAll('.aba').forEach((s) => s.classList.remove('ativa'));
    $(`#aba-${b.dataset.aba}`).classList.add('ativa');
  });
});

/* --------------------------- Montar DTF --------------------------- */

const artes = []; // { id, name, file, img, wCm, hCm, qty, rotate, fill, ratio }
let resultado = null;

function lerSettings() {
  return {
    widthCm: +$('#s-width').value || 58,
    marginCm: +$('#s-margin').value || 0,
    gapCm: +$('#s-gap').value || 0,
    maxLenCm: +$('#s-maxlen').value || 3000,
    dpi: +$('#s-dpi').value || 300,
    contour: $('#s-contour').checked,
    grouped: $('#s-grouped').checked,
    mirror: $('#s-mirror').checked,
    angleStep: +$('#s-angle').value || 90,
    costM: 0,
    priceM: 0,
    forceRotate: false,
  };
}

function arquivoParaB64(file) {
  return new Promise((res, rej) => {
    const r = new FileReader();
    r.onerror = rej;
    r.onload = () => res(r.result.split(',')[1]);
    r.readAsDataURL(file);
  });
}

function carregarImagem(file) {
  return new Promise((res, rej) => {
    const url = URL.createObjectURL(file);
    const img = new Image();
    img.onload = () => res(img);
    img.onerror = rej;
    img.src = url;
  });
}

$('#add-artes').addEventListener('change', async (ev) => {
  const files = [...ev.target.files];
  ev.target.value = '';
  if (!files.length) return;
  if (!TAURI) {
    // Sem o núcleo: só lista para a prévia visual, medindo no navegador.
    for (const f of files) {
      const img = await carregarImagem(f);
      const a = {
        id: Math.random(),
        name: f.name,
        file: f,
        img,
        wCm: +(img.width / 300 * 2.54).toFixed(2),
        hCm: +(img.height / 300 * 2.54).toFixed(2),
        qty: 1,
        rotate: true,
        fill: false,
        ratio: 1,
      };
      artes.push(a);
      registrarArte(a);
    }
    renderArtes();
    return;
  }
  const uploads = await Promise.all(files.map(async (f) => ({ name: f.name, b64: await arquivoParaB64(f) })));
  $('#progresso').textContent = 'Importando artes…';
  try {
    const infos = await invoke('importar_artes', { artes: uploads });
    for (const info of infos) {
      const f = files.find((x) => x.name === info.name);
      const img = await carregarImagem(f);
      const a = {
        id: info.id,
        name: info.name,
        file: f,
        img,
        wCm: +info.cmW.toFixed(2),
        hCm: +info.cmH.toFixed(2),
        qty: 1,
        rotate: true,
        fill: false,
        ratio: 1,
      };
      artes.push(a);
      registrarArte(a);
    }
  } catch (e) {
    $('#aviso').textContent = String(e);
  }
  $('#progresso').textContent = '';
  renderArtes();
});

function renderArtes() {
  const ul = $('#artes-lista');
  ul.innerHTML = '';
  artes.forEach((a, i) => {
    const li = document.createElement('li');
    li.innerHTML = `
      <div class="topo">
        <span class="nome" title="${a.name}">${a.name}</span>
        <button class="remover" title="Remover">✕</button>
      </div>
      <div class="grade">
        <label>Larg. cm<input type="number" step="0.1" min="0.1" value="${a.wCm}" data-k="wCm"></label>
        <label>Alt. cm<input type="number" step="0.1" min="0.1" value="${a.hCm}" data-k="hCm"></label>
        <label>Qtd.<input type="number" step="1" min="0" value="${a.qty}" data-k="qty"></label>
        <label>Repetir<input type="checkbox" ${a.fill ? 'checked' : ''} data-k="fill"></label>
        <label class="check"><input type="checkbox" ${a.rotate ? 'checked' : ''} data-k="rotate"> Girar</label>
      </div>
      <input class="ocr" type="text" data-k="ocr" spellcheck="false"
        value="${a.ocr ?? ''}"
        placeholder="${a.ocrLendo ? 'lendo textos da logo…' : 'textos da logo (para a busca)'}"
        title="Textos reconhecidos na logo. Servem para achar a folha na busca. Clique para corrigir.">`;
    li.querySelector('.remover').addEventListener('click', () => {
      artes.splice(i, 1);
      renderArtes();
      agendarAutosave();
    });
    li.querySelectorAll('input').forEach((inp) => {
      inp.addEventListener('change', () => {
        const k = inp.dataset.k;
        if (k === 'ocr') {
          a.ocr = inp.value.trim();
          agendarAutosave();
          return;
        }
        a[k] = inp.type === 'checkbox' ? inp.checked : +inp.value;
        agendarAutosave();
      });
    });
    ul.appendChild(li);
  });
}

$('#btn-encaixar').addEventListener('click', async () => {
  if (!artes.length) return;
  if (!TAURI) {
    $('#aviso').textContent = 'O encaixe roda no app de desktop (núcleo Rust).';
    return;
  }
  $('#aviso').textContent = '';
  $('#progresso').textContent = 'Otimizando encaixe…';
  try {
    const res = await invoke('encaixar', {
      settings: lerSettings(),
      itens: artes.map((a) => ({
        id: a.id,
        name: a.name,
        wCm: a.wCm,
        hCm: a.hCm,
        qty: a.qty,
        rotate: a.rotate,
        fill: a.fill,
        ratio: a.ratio,
      })),
    });
    resultado = res;
    // Repetição máxima pode ter ajustado as quantidades.
    const total = res.sheets.reduce((t, s) => t + s.placements.length, 0);
    for (const a of artes) {
      if (a.fill) {
        const n = res.sheets.reduce(
          (t, s) => t + s.placements.filter((p) => artes[p.item].id === a.id).length,
          0
        );
        if (n) a.qty = n;
      }
    }
    renderArtes();
    desenharFolhas(res);
    if (res.tooBig.length) {
      $('#aviso').textContent =
        'Não cabe no filme: ' + res.tooBig.map((i) => artes[i].name).join(', ');
    }
    $('#btn-exportar').disabled = !res.sheets.length;
    $('#btn-exportar-pdf').disabled = !res.sheets.length;
    const s = lerSettings();
    const len = res.sheets[0] ? (res.sheets[0].lengthPx / s.dpi) * 2.54 : 0;
    $('#stats').textContent = res.sheets.length
      ? `${res.sheets.length} folha(s) · ${total} peças · ${(len / 100).toFixed(2)} m de filme`
      : '';
    agendarAutosave();
  } catch (e) {
    $('#aviso').textContent = String(e);
  }
  $('#progresso').textContent = '';
});

$('#btn-exportar').addEventListener('click', async () => {
  if (!TAURI || !resultado) return;
  const pasta = await escolherPasta('Pasta de destino para os PNGs');
  if (!pasta) return;
  try {
    const arquivos = await invoke('exportar_png', { pasta });
    $('#progresso').textContent = 'Exportado: ' + arquivos.join(' · ');
  } catch (e) {
    $('#aviso').textContent = String(e);
  }
});

$('#btn-exportar-pdf').addEventListener('click', async () => {
  if (!TAURI || !resultado) return;
  const pasta = await escolherPasta('Pasta de destino para o PDF');
  if (!pasta) return;
  try {
    const arquivos = await invoke('exportar_pdf', { pasta });
    $('#progresso').textContent = 'Exportado: ' + arquivos.join(' · ');
  } catch (e) {
    $('#aviso').textContent = String(e);
  }
});

/* ---------------------- Desenho da prévia (canvas) ---------------------- */

const RULER_X = 40;
const RULER_Y = 22;

/** Sincroniza a folha ajustada manualmente com o núcleo (para a exportação). */
async function sincronizarFolha(idx) {
  if (!TAURI || !resultado) return;
  try {
    await invoke('ajustar_layout', { folha: idx, placements: resultado.sheets[idx].placements });
    agendarAutosave();
  } catch (e) {
    $('#aviso').textContent = String(e);
  }
}

function desenharFolhas(res) {
  const host = $('#folhas');
  host.innerHTML = '';
  const s = lerSettings();
  const cm = (px) => (px / s.dpi) * 2.54;
  const escala = Math.min(1, 900 / res.sheets[0]?.widthPx || 1);

  res.sheets.forEach((sheet, i) => {
    const card = document.createElement('div');
    card.className = 'folha-card';
    const titulo = document.createElement('div');
    titulo.className = 'titulo';
    card.appendChild(titulo);

    const toolbar = document.createElement('div');
    toolbar.className = 'folha-toolbar';
    toolbar.innerHTML = `
      <span class="dica">Clique numa peça para selecionar; arraste para mover.</span>
      <button data-a="girar" disabled>Girar 90°</button>
      <button data-a="excluir" disabled>Excluir</button>`;
    card.appendChild(toolbar);

    const cv = document.createElement('canvas');
    card.appendChild(cv);
    host.appendChild(card);

    let sel = -1;
    const btnGirar = toolbar.querySelector('[data-a="girar"]');
    const btnExcluir = toolbar.querySelector('[data-a="excluir"]');

    const atualizarTitulo = () => {
      titulo.textContent = `Folha ${i + 1} — ${cm(sheet.widthPx).toFixed(1)} × ${cm(sheet.lengthPx).toFixed(1)} cm · ${sheet.placements.length} peças`;
    };

    const desenhar = () => {
      atualizarTitulo();
      cv.width = Math.round(sheet.widthPx * escala) + RULER_X;
      cv.height = Math.round(sheet.lengthPx * escala) + RULER_Y;
      const ctx = cv.getContext('2d');
      ctx.translate(RULER_X, RULER_Y);
      ctx.save();
      ctx.beginPath();
      ctx.rect(0, 0, sheet.widthPx * escala, sheet.lengthPx * escala);
      ctx.clip();

      sheet.placements.forEach((p, pi) => {
        const arte = artes[p.item];
        ctx.save();
        const x = p.x * escala;
        const y = p.y * escala;
        ctx.translate(x + p.cx * escala, y + p.cy * escala);
        ctx.rotate((p.angle * Math.PI) / 180);
        if (s.mirror) ctx.scale(-1, 1);
        const w = p.iw * escala;
        const h = p.ih * escala;
        if (arte?.img) ctx.drawImage(arte.img, -w / 2, -h / 2, w, h);
        ctx.restore();
        if (pi === sel) {
          ctx.strokeStyle = '#00789e';
          ctx.lineWidth = 2;
          ctx.setLineDash([5, 4]);
          ctx.strokeRect(x, y, p.w * escala, p.h * escala);
          ctx.setLineDash([]);
        }
      });
      ctx.restore();

      // Régua em cm.
      ctx.fillStyle = '#5a6b74';
      ctx.font = '10px "IBM Plex Mono", monospace';
      const passoPx = s.dpi / 2.54; // 1 cm
      for (let x = 0; x <= sheet.widthPx; x += passoPx * 5) {
        const sx = x * escala;
        ctx.fillRect(sx, -8, 1, 8);
        ctx.fillText(String(Math.round(cm(x))), sx + 3, -10);
      }
      for (let y = 0; y <= sheet.lengthPx; y += passoPx * 5) {
        const sy = y * escala;
        ctx.fillRect(-8, sy, 8, 1);
        ctx.fillText(String(Math.round(cm(y))), -34, sy + 3);
      }
    };
    desenhar();

    const pecaEm = (ev) => {
      const r = cv.getBoundingClientRect();
      const mx = (ev.clientX - r.left) * (cv.width / r.width) - RULER_X;
      const my = (ev.clientY - r.top) * (cv.height / r.height) - RULER_Y;
      for (let pi = sheet.placements.length - 1; pi >= 0; pi--) {
        const p = sheet.placements[pi];
        if (mx >= p.x * escala && mx <= (p.x + p.w) * escala && my >= p.y * escala && my <= (p.y + p.h) * escala) {
          return pi;
        }
      }
      return -1;
    };

    let drag = null;
    cv.addEventListener('mousedown', (ev) => {
      const pi = pecaEm(ev);
      sel = pi;
      btnGirar.disabled = btnExcluir.disabled = sel < 0;
      if (pi >= 0) {
        const r = cv.getBoundingClientRect();
        drag = {
          pi,
          dx: (ev.clientX - r.left) * (cv.width / r.width) - RULER_X - sheet.placements[pi].x * escala,
          dy: (ev.clientY - r.top) * (cv.height / r.height) - RULER_Y - sheet.placements[pi].y * escala,
        };
      }
      desenhar();
    });
    cv.addEventListener('mousemove', (ev) => {
      if (!drag) return;
      const r = cv.getBoundingClientRect();
      const mx = (ev.clientX - r.left) * (cv.width / r.width) - RULER_X;
      const my = (ev.clientY - r.top) * (cv.height / r.height) - RULER_Y;
      const p = sheet.placements[drag.pi];
      p.x = Math.max(0, Math.round(mx / escala - drag.dx / escala));
      p.y = Math.max(0, Math.round(my / escala - drag.dy / escala));
      desenhar();
    });
    const soltar = () => {
      if (drag) {
        drag = null;
        sincronizarFolha(i);
      }
    };
    cv.addEventListener('mouseup', soltar);
    cv.addEventListener('mouseleave', soltar);

    btnGirar.addEventListener('click', () => {
      if (sel < 0) return;
      const p = sheet.placements[sel];
      // Gira 90° mantendo o centro da peça parado.
      const centroX = p.x + p.cx;
      const centroY = p.y + p.cy;
      const [nw, nh] = [p.h, p.w];
      const [ncx, ncy] = [p.h - p.cy, p.cx];
      p.angle = (p.angle + 90) % 360;
      p.w = nw;
      p.h = nh;
      p.cx = ncx;
      p.cy = ncy;
      p.x = Math.round(centroX - ncx);
      p.y = Math.round(centroY - ncy);
      // Recalcula o comprimento da folha.
      sheet.lengthPx = Math.max(...sheet.placements.map((q) => q.y + q.h)) + Math.round((s.marginCm / 2.54) * s.dpi);
      desenhar();
      sincronizarFolha(i);
    });

    btnExcluir.addEventListener('click', () => {
      if (sel < 0) return;
      sheet.placements.splice(sel, 1);
      sel = -1;
      btnGirar.disabled = btnExcluir.disabled = true;
      desenhar();
      sincronizarFolha(i);
    });
  });
}

/* ------------------------ Tratar Imagem (editor) ------------------------ */

const ed = {
  aberto: false,
  picking: false,
  picked: null, // "#rrggbb"
  img: new Image(),
};

function edMostrar(view) {
  ed.img.src = view.png;
  ed.img.onload = () => {
    const cv = $('#ed-canvas');
    cv.width = view.w;
    cv.height = view.h;
    cv.hidden = false;
    $('#ed-vazio').hidden = true;
    const ctx = cv.getContext('2d');
    ctx.clearRect(0, 0, cv.width, cv.height);
    ctx.drawImage(ed.img, 0, 0);
  };
  $('#ed-info').textContent = `${view.nome} — ${view.w} × ${view.h} px`;
  $('#ed-desfazer').disabled = !view.undo;
  $('#ed-salvar').disabled = false;
  $('#ed-descartar').disabled = false;
  $('#ed-svg').hidden = !view.temSvg;
  ed.aberto = true;
}

$('#ed-arquivo').addEventListener('change', async (ev) => {
  const f = ev.target.files[0];
  ev.target.value = '';
  if (!f) return;
  if (!TAURI) {
    $('#ed-aviso').textContent = 'O editor roda no app de desktop (núcleo Rust).';
    return;
  }
  $('#ed-aviso').textContent = '';
  $('#ed-progresso').textContent = 'Abrindo…';
  try {
    const [info] = await invoke('importar_artes', {
      artes: [{ name: f.name, b64: await arquivoParaB64(f) }],
    });
    const view = await invoke('editor_abrir', { id: info.id });
    edMostrar(view);
  } catch (e) {
    $('#ed-aviso').textContent = String(e);
  }
  $('#ed-progresso').textContent = '';
});

$('#ed-pick').addEventListener('click', () => {
  ed.picking = true;
  $('#ed-pick').textContent = 'Clique na imagem…';
});

$('#ed-canvas').addEventListener('click', (ev) => {
  if (!ed.picking) return;
  const cv = ev.currentTarget;
  const r = cv.getBoundingClientRect();
  const x = Math.floor(((ev.clientX - r.left) / r.width) * cv.width);
  const y = Math.floor(((ev.clientY - r.top) / r.height) * cv.height);
  const p = cv.getContext('2d').getImageData(x, y, 1, 1).data;
  ed.picked = '#' + [p[0], p[1], p[2]].map((v) => v.toString(16).padStart(2, '0')).join('');
  $('#ed-swatch').style.background = ed.picked;
  $('#ed-swatch').title = ed.picked;
  ed.picking = false;
  $('#ed-pick').textContent = 'Conta-gotas';
});

document.querySelectorAll('#editor-painel [data-op]').forEach((b) => {
  b.addEventListener('click', async () => {
    if (!ed.aberto || !TAURI) return;
    const op = b.dataset.op;
    if ((op === 'removerCor' || op === 'trocarCor') && !ed.picked) {
      $('#ed-aviso').textContent = 'Use o conta-gotas para escolher a cor primeiro.';
      return;
    }
    $('#ed-aviso').textContent = '';
    $('#ed-progresso').textContent = 'Processando…';
    try {
      const view = await invoke('editor_op', {
        op,
        tolPercent: +$('#ed-tol').value,
        cor: ed.picked,
        corNova: $('#ed-cor-nova').value,
        alphaT: +$('#ed-alpha').value,
      });
      edMostrar(view);
    } catch (e) {
      $('#ed-aviso').textContent = String(e);
    }
    $('#ed-progresso').textContent = '';
  });
});

$('#ed-desfazer').addEventListener('click', async () => {
  if (!TAURI) return;
  const view = await invoke('editor_desfazer');
  if (view) edMostrar(view);
});

$('#ed-vetorizar').addEventListener('click', async () => {
  if (!ed.aberto || !TAURI) return;
  $('#ed-aviso').textContent = '';
  $('#ed-progresso').textContent = 'Vetorizando…';
  try {
    const view = await invoke('editor_vetorizar', { precisaoCor: +$('#ed-precisao').value });
    edMostrar(view);
  } catch (e) {
    $('#ed-aviso').textContent = String(e);
  }
  $('#ed-progresso').textContent = '';
});

$('#ed-upscale-ia').addEventListener('click', async () => {
  if (!ed.aberto || !TAURI) return;
  $('#ed-aviso').textContent = '';
  $('#ed-progresso').textContent = 'Ampliando com IA (Real-ESRGAN)…';
  try {
    const view = await invoke('editor_upscale_ia');
    edMostrar(view);
  } catch (e) {
    $('#ed-aviso').textContent = String(e);
  }
  $('#ed-progresso').textContent = '';
});

$('#ed-svg').addEventListener('click', async (e) => {
  e.preventDefault();
  if (!TAURI) return;
  try {
    const svg = await invoke('editor_svg');
    if (!svg) return;
    const a = document.createElement('a');
    a.href = URL.createObjectURL(new Blob([svg], { type: 'image/svg+xml' }));
    a.download = 'arte-vetorizada.svg';
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 5000);
  } catch (e2) {
    $('#ed-aviso').textContent = String(e2);
  }
});

$('#ed-salvar').addEventListener('click', async () => {
  if (!ed.aberto || !TAURI) return;
  try {
    const dpi = +$('#s-dpi').value || 300;
    const info = await invoke('editor_salvar', { dpi });
    const img = new Image();
    img.src = ed.img.src;
    await img.decode();
    const a = {
      id: info.id,
      name: info.name,
      file: null,
      img,
      wCm: +info.cmW.toFixed(2),
      hCm: +info.cmH.toFixed(2),
      qty: 1,
      rotate: true,
      fill: false,
      ratio: 1,
    };
    artes.push(a);
    registrarArte(a);
    renderArtes();
    $('#ed-canvas').hidden = true;
    $('#ed-vazio').hidden = false;
    $('#ed-salvar').disabled = true;
    $('#ed-descartar').disabled = true;
    ed.aberto = false;
    document.querySelector('#abas button[data-aba="dtf"]').click();
  } catch (e) {
    $('#ed-aviso').textContent = String(e);
  }
});

$('#ed-descartar').addEventListener('click', async () => {
  if (TAURI) await invoke('editor_descartar');
  $('#ed-canvas').hidden = true;
  $('#ed-vazio').hidden = false;
  $('#ed-info').textContent = '';
  $('#ed-salvar').disabled = true;
  $('#ed-descartar').disabled = true;
  ed.aberto = false;
});

/* ---------------------------- Biblioteca ---------------------------- */

async function atualizarBiblioteca() {
  if (!TAURI) return;
  try {
    const clientes = await invoke('clientes_list');
    const ul = $('#clientes-lista');
    ul.innerHTML = '';
    clientes.forEach((c) => {
      const li = document.createElement('li');
      li.innerHTML = `<div class="topo"><span class="nome">${c.nome}</span></div>`;
      ul.appendChild(li);
    });
    const logos = await invoke('logos_list', { clienteId: null });
    const lu = $('#logos-lista');
    lu.innerHTML = '';
    logos.forEach((l) => {
      const li = document.createElement('li');
      li.innerHTML = `<div class="topo"><span class="nome">${l.nome}</span>
        <button class="remover" title="Remover">✕</button></div>`;
      li.querySelector('.remover').addEventListener('click', async () => {
        await invoke('logos_remover', { id: l.id });
        atualizarBiblioteca();
      });
      lu.appendChild(li);
    });
  } catch (e) {
    console.error(e);
  }
}

$('#cliente-add').addEventListener('click', async () => {
  const nome = $('#cliente-nome').value.trim();
  if (!nome || !TAURI) return;
  await invoke('clientes_add', { nome });
  $('#cliente-nome').value = '';
  atualizarBiblioteca();
});

$('#logo-add').addEventListener('click', async () => {
  const nome = $('#logo-nome').value.trim();
  const file = $('#logo-arquivo').files[0];
  if (!nome || !file || !TAURI) return;
  const b64 = await arquivoParaB64(file);
  await invoke('logos_add', { clienteId: null, nome, b64 });
  $('#logo-nome').value = '';
  $('#logo-arquivo').value = '';
  atualizarBiblioteca();
});

/* ---------------------- Dados e backup (§3) ---------------------- */

async function cfgMostrar() {
  if (!TAURI) return;
  try {
    const cfg = await invoke('config_obter');
    $('#cfg-pasta').textContent = `Pasta de dados: ${cfg.pastaDados}`;
    $('#cfg-pasta').title = `Padrão: ${cfg.pastaPadrao}`;
  } catch (e) {
    $('#cfg-pasta').textContent = String(e);
  }
}

$('#cfg-mudar-pasta').addEventListener('click', async () => {
  if (!TAURI) return;
  const pasta = await escolherPasta('Escolha a pasta de dados da Central');
  if (!pasta) return;
  $('#cfg-aviso').textContent = 'Migrando dados…';
  try {
    await invoke('config_definir_pasta', { nova: pasta });
    $('#cfg-aviso').textContent = 'Pasta de dados alterada. Os dados foram migrados (ou a pasta existente foi adotada).';
    cfgMostrar();
    atualizarBiblioteca();
    atualizarFolhas();
  } catch (e) {
    $('#cfg-aviso').textContent = String(e);
  }
});

$('#backup-exportar').addEventListener('click', async () => {
  if (!TAURI) return;
  const destino = await escolherPasta('Onde guardar o backup completo?');
  if (!destino) return;
  $('#cfg-aviso').textContent = 'Exportando backup…';
  try {
    $('#cfg-aviso').textContent = await invoke('backup_exportar', { destino });
  } catch (e) {
    $('#cfg-aviso').textContent = String(e);
  }
});

$('#backup-importar').addEventListener('click', async () => {
  if (!TAURI) return;
  const origem = await escolherPasta('Escolha a pasta do backup a restaurar');
  if (!origem) return;
  $('#cfg-aviso').textContent = 'Restaurando backup…';
  try {
    $('#cfg-aviso').textContent = await invoke('backup_importar', { origem });
    atualizarBiblioteca();
    atualizarFolhas();
  } catch (e) {
    $('#cfg-aviso').textContent = String(e);
  }
});

cfgMostrar();

atualizarBiblioteca();
atualizarFolhas();

/* ------------------- OCR das logos (tesseract.js offline) ------------------- */

const PUB = new URL('.', document.baseURI).href;
let _ocrWorker = null;
let ocrOff = false;
let ocrChain = Promise.resolve();

async function ocrWorker() {
  if (_ocrWorker) return _ocrWorker;
  _ocrWorker = await Tesseract.createWorker('por', 1, {
    workerPath: PUB + 'tesseract-core/worker.min.js',
    corePath: PUB + 'tesseract-core',
    langPath: PUB + 'tessdata',
    gzip: false,
    cacheMethod: 'none',
    logger: () => {},
  });
  await _ocrWorker.setParameters({ tessedit_pageseg_mode: '11' }); // texto esparso
  return _ocrWorker;
}

/** Medidas naturais da imagem (img pode ser HTMLImageElement). */
function dimDaImg(img) {
  return [img.naturalWidth || img.width || 1, img.naturalHeight || img.height || 1];
}

/** Canvas preparado pro OCR: escala p/ até 1400px, margem 6% e fundo com
    contraste; `flip` força a segunda tentativa invertida. */
function ocrCanvas(img, flip) {
  const [w0, h0] = dimDaImg(img);
  const esc = Math.min(1400 / Math.max(w0, h0), 3);
  const w = Math.max(1, Math.round(w0 * esc));
  const h = Math.max(1, Math.round(h0 * esc));
  const pad = Math.round(Math.max(w, h) * 0.06);
  const cv = document.createElement('canvas');
  cv.width = w + pad * 2;
  cv.height = h + pad * 2;
  const ctx = cv.getContext('2d');
  // Mede a luminosidade sobre fundo branco para decidir o contraste.
  ctx.fillStyle = '#fff';
  ctx.fillRect(0, 0, cv.width, cv.height);
  ctx.drawImage(img, pad, pad, w, h);
  const d = ctx.getImageData(0, 0, cv.width, cv.height).data;
  let soma = 0;
  let n = 0;
  for (let i = 0; i < d.length; i += 16) {
    soma += 0.3 * d[i] + 0.59 * d[i + 1] + 0.11 * d[i + 2];
    n++;
  }
  const light = soma / n > 127;
  ctx.fillStyle = light ? '#000' : '#fff';
  ctx.fillRect(0, 0, cv.width, cv.height);
  ctx.drawImage(img, pad, pad, w, h);
  if (light !== !!flip) {
    const im = ctx.getImageData(0, 0, cv.width, cv.height);
    const p = im.data;
    for (let i = 0; i < p.length; i += 4) {
      p[i] = 255 - p[i];
      p[i + 1] = 255 - p[i + 1];
      p[i + 2] = 255 - p[i + 2];
    }
    ctx.putImageData(im, 0, 0);
  }
  return cv;
}

/** Máscara: vira preto o que casa com a cor, branco o resto (isola um texto colorido). */
function ocrMask(img, rgb) {
  const [w0, h0] = dimDaImg(img);
  const esc = Math.min(1400 / Math.max(w0, h0), 3);
  const w = Math.max(1, Math.round(w0 * esc));
  const h = Math.max(1, Math.round(h0 * esc));
  const pad = Math.round(Math.max(w, h) * 0.06);
  const cv = document.createElement('canvas');
  cv.width = w + pad * 2;
  cv.height = h + pad * 2;
  const ctx = cv.getContext('2d');
  ctx.fillStyle = '#fff';
  ctx.fillRect(0, 0, cv.width, cv.height);
  ctx.drawImage(img, pad, pad, w, h);
  const im = ctx.getImageData(0, 0, cv.width, cv.height);
  const p = im.data;
  for (let i = 0; i < p.length; i += 4) {
    const casa =
      p[i + 3] > 128 &&
      Math.abs(p[i] - rgb[0]) + Math.abs(p[i + 1] - rgb[1]) + Math.abs(p[i + 2] - rgb[2]) < 90;
    const v = casa ? 0 : 255;
    p[i] = p[i + 1] = p[i + 2] = v;
    p[i + 3] = 255;
  }
  ctx.putImageData(im, 0, 0);
  return cv;
}

/** Até `n` cores principais da imagem (baldes de 5 bits/canal numa amostra 80×80). */
function mainColors(img, n = 4) {
  const cv = document.createElement('canvas');
  cv.width = cv.height = 80;
  const ctx = cv.getContext('2d');
  ctx.drawImage(img, 0, 0, 80, 80);
  const d = ctx.getImageData(0, 0, 80, 80).data;
  const buckets = new Map();
  let total = 0;
  for (let i = 0; i < d.length; i += 4) {
    if (d[i + 3] < 128) continue;
    total++;
    const key = `${d[i] >> 3},${d[i + 1] >> 3},${d[i + 2] >> 3}`;
    buckets.set(key, (buckets.get(key) || 0) + 1);
  }
  return [...buckets.entries()]
    .filter(([, c]) => c > total * 0.03)
    .sort((a, b) => b[1] - a[1])
    .slice(0, n)
    .map(([k]) => k.split(',').map((v) => Math.round((+v + 0.5) * 8)));
}

async function reconhecerArte(a) {
  if (ocrOff || !a.img) {
    a.ocrLendo = false;
    return;
  }
  try {
    const worker = await ocrWorker();
    const tentativas = [ocrCanvas(a.img, 0), ocrCanvas(a.img, 1)];
    for (const rgb of mainColors(a.img, 4)) tentativas.push(ocrMask(a.img, rgb));
    const achados = new Map();
    for (const t of tentativas) {
      const { data } = await worker.recognize(t, {}, { text: true, blocks: true });
      for (const b of data.blocks || []) {
        for (const p of b.paragraphs || []) {
          for (const l of p.lines || []) {
            for (const w of l.words || []) {
              const txt = (w.text || '').trim();
              if ((w.confidence ?? 0) < 60 || txt.length < 2) continue;
              const letras = (txt.match(/[A-Za-zÀ-ÿ]/g) || []).length;
              const digitos = (txt.match(/\d/g) || []).length;
              if (letras < 2 && digitos < 2) continue;
              const key = txt.toLowerCase();
              if (!achados.has(key) || achados.get(key) < w.confidence) achados.set(key, w.confidence);
            }
          }
        }
      }
    }
    a.ocr = [...achados.keys()].join(' ');
  } catch (e) {
    console.warn('OCR desligado:', e);
    ocrOff = true;
  }
  a.ocrLendo = false;
  // Atualiza o campo do card sem re-renderizar (para não atrapalhar a digitação).
  const li = $('#artes-lista').children[artes.indexOf(a)];
  const inp = li?.querySelector('input.ocr');
  if (inp && document.activeElement !== inp) {
    inp.value = a.ocr || '';
    inp.placeholder = ocrOff ? 'escreva os textos da logo para a busca' : 'textos da logo (para a busca)';
  }
  if (a.ocr) agendarAutosave(); // o texto lido entra no backup da folha
}

function queueOcr(a) {
  ocrChain = ocrChain.then(() => reconhecerArte(a));
}

/* ------------------------- dHash (busca por imagem) ------------------------- */

/** Hash perceptual de 64 bits: grade 9×8 em branco, cinza, bit = vizinho mais claro. */
function dhash(img) {
  const cv = document.createElement('canvas');
  cv.width = 9;
  cv.height = 8;
  const ctx = cv.getContext('2d');
  ctx.fillStyle = '#fff';
  ctx.fillRect(0, 0, 9, 8);
  ctx.drawImage(img, 0, 0, 9, 8);
  const d = ctx.getImageData(0, 0, 9, 8).data;
  const g = [];
  for (let i = 0; i < 9 * 8; i++) {
    g.push(0.3 * d[i * 4] + 0.59 * d[i * 4 + 1] + 0.11 * d[i * 4 + 2]);
  }
  let bits = '';
  for (let y = 0; y < 8; y++) {
    for (let x = 0; x < 8; x++) {
      bits += g[y * 9 + x] > g[y * 9 + x + 1] ? '1' : '0';
    }
  }
  return bits;
}

function hamming(a, b) {
  let d = Math.abs(a.length - b.length);
  for (let i = 0; i < Math.min(a.length, b.length); i++) if (a[i] !== b[i]) d++;
  return d;
}

/** Chamado ao importar uma arte: guarda o hash e agenda a leitura dos textos. */
function registrarArte(a) {
  try {
    a.dh = dhash(a.img);
  } catch (e) {
    console.warn('dHash falhou:', e);
  }
  a.ocrLendo = true;
  queueOcr(a);
}

/* ------------------- Salvar / abrir folhas (backup fiel) ------------------- */

let folhaAtualId = null;
const PIXEL =
  'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==';

function arquivoParaDataURL(file) {
  return new Promise((res, rej) => {
    const r = new FileReader();
    r.onerror = rej;
    r.onload = () => res(r.result);
    r.readAsDataURL(file);
  });
}

async function blobDaArte(a) {
  if (a.file) return arquivoParaDataURL(a.file);
  if (a.img?.src?.startsWith('data:')) return a.img.src;
  return null;
}

function previewDaFolha() {
  const src = document.querySelector('#folhas canvas');
  if (!src) return null;
  const esc = Math.min(1, 360 / src.width);
  const cv = document.createElement('canvas');
  cv.width = Math.max(1, Math.round(src.width * esc));
  cv.height = Math.max(1, Math.round(src.height * esc));
  const ctx = cv.getContext('2d');
  ctx.fillStyle = '#fff';
  ctx.fillRect(0, 0, cv.width, cv.height);
  ctx.drawImage(src, 0, 0, cv.width, cv.height);
  return cv.toDataURL('image/png').split(',')[1];
}

async function salvarFolha(silencioso = false) {
  const s = lerSettings();
  const nome = $('#s-sheetname').value.trim() || `folha ${new Date().toLocaleString('pt-BR')}`;
  const cliente = $('#s-cliente').value.trim() || null;
  const items = [];
  for (const a of artes) {
    items.push({
      name: a.name,
      blob: await blobDaArte(a),
      wCm: a.wCm,
      hCm: a.hCm,
      qty: a.qty,
      rotate: a.rotate,
      libId: null,
      fill: a.fill,
      ratio: a.ratio,
      text: '',
      textEm: 0,
      ocr: a.ocr || '',
      dh: a.dh || null,
      bgOk: false,
      nameList: [],
    });
  }
  const backup = {
    app: 'montador-dtf',
    version: 2,
    name: nome,
    date: Date.now(),
    client: cliente,
    frame: { widthCm: s.widthCm, marginCm: s.marginCm, maxLenCm: s.maxLenCm, dpi: s.dpi },
    settings: s,
    items,
    layout: resultado.sheets.map((sh) => sh.placements.map((p) => [p.item, p.x, p.y, p.angle])),
    layoutMode: {
      contour: s.contour,
      angleStep: s.angleStep,
      cell: resultado.cell,
      gapCm: s.gapCm,
      forceRotate: false,
    },
  };
  const textos = items
    .map((i) => `${i.name} ${i.ocr}`)
    .join(' ')
    .replace(/\s+/g, ' ')
    .trim();
  const id = await invoke('folhas_salvar', {
    id: folhaAtualId,
    nome,
    cliente,
    dados: JSON.stringify(backup),
    textos,
    previewB64: previewDaFolha(),
  });
  folhaAtualId = id;
  $('#folha-atual').textContent = silencioso
    ? `Folha #${id} — ${nome} · salva automaticamente às ${new Date().toLocaleTimeString('pt-BR')}`
    : `Folha salva na biblioteca (#${id}) — ${nome}`;
  if (!silencioso) atualizarFolhas($('#folhas-busca').value.trim());
}

/* ------------------------------ Autosave ------------------------------ */

let autosaveTimer = null;
let salvandoFolha = false;

/** Agenda o salvamento automático da folha em edição (debounce de 1,5 s).
    Dispara em: encaixe, arrastar/girar/excluir peça, edição de arte, nome/cliente. */
function agendarAutosave() {
  if (!TAURI || !resultado || !resultado.sheets.length) return;
  clearTimeout(autosaveTimer);
  autosaveTimer = setTimeout(async () => {
    if (salvandoFolha) {
      agendarAutosave();
      return;
    }
    salvandoFolha = true;
    try {
      await salvarFolha(true);
    } catch (e) {
      console.error('autosave:', e);
    }
    salvandoFolha = false;
  }, 1500);
}

$('#btn-nova-folha').addEventListener('click', () => {
  folhaAtualId = null;
  $('#s-sheetname').value = '';
  $('#s-cliente').value = '';
  $('#folha-atual').textContent = 'Nova folha: a montagem atual será salva como um registro novo.';
});

$('#s-sheetname').addEventListener('input', agendarAutosave);
$('#s-cliente').addEventListener('input', agendarAutosave);

$('#btn-salvar-folha').addEventListener('click', async () => {
  if (!TAURI) {
    $('#aviso').textContent = 'Salvar folhas roda no app de desktop (núcleo Rust).';
    return;
  }
  if (!resultado || !resultado.sheets.length) {
    $('#aviso').textContent = 'Encaixe as artes primeiro para salvar a folha.';
    return;
  }
  $('#aviso').textContent = '';
  $('#progresso').textContent = 'Salvando folha…';
  try {
    await salvarFolha();
  } catch (e) {
    $('#aviso').textContent = String(e);
  }
  $('#progresso').textContent = '';
});

/** Mesma caixa do núcleo Rust (variant_box): múltiplo de 90° troca eixos;
    senão, caixa do retângulo girado + 2 px de folga. */
function variantBox(iw, ih, angle) {
  const a = ((angle % 360) + 360) % 360;
  if (a % 90 === 0) {
    const straight = a % 180 === 0;
    const w = straight ? iw : ih;
    const h = straight ? ih : iw;
    return { angle: a, iw, ih, w, h, cx: w / 2, cy: h / 2 };
  }
  const rad = (a * Math.PI) / 180;
  const w = Math.ceil(iw * Math.abs(Math.cos(rad)) + ih * Math.abs(Math.sin(rad))) + 2;
  const h = Math.ceil(iw * Math.abs(Math.sin(rad)) + ih * Math.abs(Math.cos(rad))) + 2;
  return { angle: a, iw, ih, w, h, cx: w / 2, cy: h / 2 };
}

async function abrirFolha(id) {
  if (!TAURI) return;
  $('#aviso').textContent = '';
  $('#progresso').textContent = 'Abrindo folha…';
  try {
    const dados = JSON.parse(await invoke('folhas_abrir', { id }));
    const st = dados.settings || {};
    $('#s-width').value = st.widthCm ?? 58;
    $('#s-margin').value = st.marginCm ?? 0;
    $('#s-gap').value = st.gapCm ?? 0;
    $('#s-maxlen').value = st.maxLenCm ?? 3000;
    $('#s-dpi').value = st.dpi ?? 300;
    $('#s-contour').checked = !!st.contour;
    $('#s-grouped').checked = !!st.grouped;
    $('#s-mirror').checked = !!st.mirror;
    $('#s-angle').value = st.angleStep ?? 90;
    $('#s-sheetname').value = dados.name || '';
    $('#s-cliente').value = dados.client || '';
    const s = lerSettings();

    // Importa as artes do backup (placeholder de 1 px mantém os índices se faltar blob).
    const itemsIn = dados.items || [];
    const uploads = itemsIn.map((it) => ({
      name: it.name || 'arte',
      b64: ((it.blob || PIXEL).split(',')[1]) || '',
    }));
    const infos = uploads.length ? await invoke('importar_artes', { artes: uploads }) : [];
    artes.length = 0;
    for (let i = 0; i < itemsIn.length; i++) {
      const it = itemsIn[i];
      const img = new Image();
      img.src = it.blob || PIXEL;
      await img.decode();
      const a = {
        id: infos[i].id,
        name: it.name || 'arte',
        file: null,
        img,
        wCm: it.wCm,
        hCm: it.hCm,
        qty: it.qty ?? 1,
        rotate: it.rotate !== false,
        fill: !!it.fill,
        ratio: it.ratio ?? 1,
        ocr: it.ocr || '',
        dh: it.dh || null,
      };
      if (!a.dh && it.blob) {
        try {
          a.dh = dhash(img);
        } catch {}
      }
      artes.push(a);
    }

    // Reconstrói o layout exato do backup.
    const norm = (q) => (Array.isArray(q) ? { i: q[0], x: q[1], y: q[2], angle: q[3] || 0 } : q);
    const folhas = (dados.layout || []).map((folha) =>
      (folha || []).map((q0) => {
        const q = norm(q0);
        const it = itemsIn[q.i] || {};
        const iw = Math.round(((it.wCm || 1) / 2.54) * s.dpi);
        const ih = Math.round(((it.hCm || 1) / 2.54) * s.dpi);
        const v = variantBox(iw, ih, q.angle || 0);
        return {
          item: q.i,
          x: Math.round(q.x),
          y: Math.round(q.y),
          w: v.w,
          h: v.h,
          angle: v.angle,
          iw: v.iw,
          ih: v.ih,
          cx: v.cx,
          cy: v.cy,
        };
      })
    );

    await invoke('abrir_montagem', {
      settings: s,
      itens: artes.map((a) => ({
        id: a.id,
        name: a.name,
        wCm: a.wCm,
        hCm: a.hCm,
        qty: a.qty,
        rotate: a.rotate,
        fill: a.fill,
        ratio: a.ratio,
      })),
      folhas,
    });

    const widthPx = Math.round((s.widthCm / 2.54) * s.dpi);
    const marginPx = Math.round((s.marginCm / 2.54) * s.dpi);
    resultado = {
      sheets: folhas.map((placements) => ({
        widthPx,
        lengthPx: placements.reduce((m, p) => Math.max(m, p.y + p.h), 0) + marginPx,
        placements,
      })),
      tooBig: [],
      cell: dados.layoutMode?.cell || 0,
    };

    folhaAtualId = id;
    $('#folha-atual').textContent = `Editando a folha #${id} — ${dados.name || ''} (salvar atualiza)`;
    $('#btn-exportar').disabled = !resultado.sheets.length;
    $('#btn-exportar-pdf').disabled = !resultado.sheets.length;
    document.querySelector('#abas button[data-aba="dtf"]').click();
    renderArtes();
    desenharFolhas(resultado);
    const total = resultado.sheets.reduce((t, sh) => t + sh.placements.length, 0);
    const len = resultado.sheets[0] ? (resultado.sheets[0].lengthPx / s.dpi) * 2.54 : 0;
    $('#stats').textContent = resultado.sheets.length
      ? `${resultado.sheets.length} folha(s) · ${total} peças · ${(len / 100).toFixed(2)} m de filme`
      : '';
  } catch (e) {
    $('#aviso').textContent = String(e);
  }
  $('#progresso').textContent = '';
}

/* --------------------- Biblioteca de folhas (busca) --------------------- */

function esc(t) {
  return String(t ?? '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

function desenharCardsFolhas(cards, distPorId) {
  const grade = $('#folhas-grade');
  grade.innerHTML = '';
  if (!cards.length) {
    grade.innerHTML = `<p class="vazio">${
      distPorId ? 'Nenhuma folha com arte parecida.' : 'Nenhuma folha salva ainda.'
    }</p>`;
    return;
  }
  for (const c of cards) {
    const el = document.createElement('div');
    el.className = 'folha-card-bib';
    const data = new Date(c.atualizadoEm).toLocaleString('pt-BR');
    const dist = distPorId?.get(c.id);
    el.innerHTML = `
      ${c.preview ? `<img src="${c.preview}" alt="">` : '<div class="sem-preview">sem prévia</div>'}
      <div class="corpo">
        <div class="nome">${esc(c.nome)}</div>
        <div class="meta">${c.cliente ? `${esc(c.cliente)} · ` : ''}${data}</div>
        ${c.textos ? `<div class="textos" title="${esc(c.textos)}">${esc(c.textos)}</div>` : ''}
        ${
          dist != null
            ? `<div class="meta dist ${dist <= 14 ? 'forte' : ''}">semelhança ${Math.max(0, 64 - dist)}/64</div>`
            : ''
        }
        <div class="acoes-card">
          <button data-a="abrir">Abrir</button>
          <button data-a="excluir">Excluir</button>
        </div>
      </div>`;
    el.querySelector('[data-a="abrir"]').addEventListener('click', () => abrirFolha(c.id));
    el.querySelector('[data-a="excluir"]').addEventListener('click', async () => {
      if (!confirm(`Excluir a folha "${c.nome}"?`)) return;
      await invoke('folhas_excluir', { id: c.id });
      if (folhaAtualId === c.id) {
        folhaAtualId = null;
        $('#folha-atual').textContent = '';
      }
      atualizarFolhas($('#folhas-busca').value.trim());
    });
    grade.appendChild(el);
  }
}

async function atualizarFolhas(busca = '') {
  if (!TAURI) return;
  try {
    const cards = await invoke('folhas_list', { busca: busca || null });
    desenharCardsFolhas(cards, null);
  } catch (e) {
    console.error(e);
  }
}

let buscaTimer = null;
$('#folhas-busca').addEventListener('input', () => {
  clearTimeout(buscaTimer);
  buscaTimer = setTimeout(() => atualizarFolhas($('#folhas-busca').value.trim()), 250);
});

$('#folhas-busca-img').addEventListener('change', async (ev) => {
  const f = ev.target.files[0];
  ev.target.value = '';
  if (!f || !TAURI) return;
  try {
    const img = await carregarImagem(f);
    const dh = dhash(img);
    const cards = await invoke('folhas_list', { busca: null });
    const dist = new Map();
    for (const c of cards) {
      if (!c.hashes?.length) continue;
      dist.set(c.id, Math.min(...c.hashes.map((h) => hamming(dh, h))));
    }
    const ordenadas = cards
      .filter((c) => dist.has(c.id))
      .sort((a, b) => dist.get(a.id) - dist.get(b.id));
    desenharCardsFolhas(ordenadas, dist);
  } catch (e) {
    console.error(e);
  }
});


/* ========================== Encaixar Moldes ==========================
   Sublimação: moldes em PDF por pedido, encaixe no rolo (núcleo Rust),
   saída em PDF vetorial (pdf-lib embute a página original recortada). */

const PT_MM = 25.4 / 72;
const MM_PT = 72 / 25.4;
const CORES_PED = ['#d9480f', '#1971c2', '#2f9e44', '#9c36b5', '#e8590c', '#0c8599', '#c2255c', '#5c940d', '#f08c00', '#3b5bdb'];
const PPM_MOLDE = 2; // pixels por mm na análise

const mol = { peds: [], result: null };
// pedido: { nome, cor, giro, moldes: [ref] }
// molde:  { id, nome, bytes, pageIndex, view, ppm, qtd, partes: ParteInfo[] }

function mcfg() {
  const lv = $('#m-larg').value;
  const wMm = lv === 'custom' ? Math.max(100, (+$('#m-larg-custom').value || 0) * 10) : +lv;
  return {
    wMm,
    gapMm: Math.max(0, +$('#m-gap').value || 0),
    maxLenMm: Math.max(200, (+$('#m-maxlen').value || 200) * 10),
    gridMm: +$('#m-grid').value,
    split: $('#m-split').checked,
    white: $('#m-white').checked,
    together: $('#m-together').checked,
  };
}

$('#m-larg').addEventListener('change', () => {
  $('#m-larg-custom-wrap').hidden = $('#m-larg').value !== 'custom';
});

function fmtM(mm) {
  return (mm / 1000).toLocaleString('pt-BR', { minimumFractionDigits: 2, maximumFractionDigits: 2 }) + ' m';
}
function hojeStr() {
  const d = new Date();
  const p = (n) => String(n).padStart(2, '0');
  return p(d.getDate()) + p(d.getMonth() + 1) + String(d.getFullYear()).slice(-2);
}

/* ---------- leitura dos PDFs (pdf.js) ---------- */

async function lerPdfMoldes(file) {
  const bytes = new Uint8Array(await file.arrayBuffer());
  const doc = await pdfjs.getDocument({ data: bytes.slice(0), isEvalSupported: false }).promise;
  const base = file.name.replace(/\.pdf$/i, '');
  const paginas = [];
  for (let p = 1; p <= doc.numPages; p++) {
    const page = await doc.getPage(p);
    const view = page.view; // [x0,y0,x1,y1] em pt
    const wmm = (view[2] - view[0]) * PT_MM;
    const hmm = (view[3] - view[1]) * PT_MM;
    let ppm = PPM_MOLDE;
    const maxDim = 9000;
    if (Math.max(wmm, hmm) * ppm > maxDim) ppm = maxDim / Math.max(wmm, hmm);
    const vp = page.getViewport({ scale: ppm * PT_MM });
    const cv = document.createElement('canvas');
    cv.width = Math.ceil(vp.width);
    cv.height = Math.ceil(vp.height);
    const ctx = cv.getContext('2d', { willReadFrequently: true });
    ctx.clearRect(0, 0, cv.width, cv.height);
    await page.render({ canvasContext: ctx, viewport: vp, background: 'rgba(0,0,0,0)' }).promise;
    const rgba = ctx.getImageData(0, 0, cv.width, cv.height).data;
    // RGBA -> base64 em blocos (sem estourar a pilha)
    let bin = '';
    const PASSO = 32768;
    for (let i = 0; i < rgba.length; i += PASSO) {
      bin += String.fromCharCode(...rgba.subarray(i, Math.min(i + PASSO, rgba.length)));
    }
    paginas.push({
      nome: doc.numPages > 1 ? `${base} (pág. ${p})` : base,
      rgba: btoa(bin),
      w: cv.width,
      h: cv.height,
      ppm,
      view,
      bytes,
      pageIndex: p - 1,
    });
    page.cleanup();
  }
  doc.destroy();
  return paginas;
}

async function analisarMoldes() {
  if (!TAURI) return;
  const c = mcfg();
  const lista = await invoke('moldes_analisar', { gridMm: c.gridMm, split: c.split, white: c.white });
  const porId = new Map(lista.map((m) => [m.id, m.partes]));
  for (const p of mol.peds) {
    for (const m of p.moldes) {
      m.partes = porId.get(m.id) || [];
    }
  }
}

async function addMoldes(ped, files) {
  const pdfs = files.filter((f) => /\.pdf$/i.test(f.name) || f.type === 'application/pdf');
  if (!pdfs.length) {
    $('#m-aviso').textContent = 'Escolha arquivos PDF.';
    return;
  }
  if (!TAURI) {
    $('#m-aviso').textContent = 'O encaixe de moldes roda no app de desktop (núcleo Rust).';
    return;
  }
  $('#m-aviso').textContent = '';
  for (const f of pdfs) {
    $('#m-progresso').textContent = `Lendo ${f.name}…`;
    try {
      const paginas = await lerPdfMoldes(f);
      const infos = await invoke('moldes_importar', {
        paginas: paginas.map((p) => ({ nome: p.nome, rgba: p.rgba, w: p.w, h: p.h, ppm: p.ppm, view: p.view })),
      });
      paginas.forEach((p, i) => {
        ped.moldes.push({
          id: infos[i].id,
          nome: p.nome,
          bytes: p.bytes,
          pageIndex: p.pageIndex,
          view: p.view,
          ppm: p.ppm,
          qtd: 0,
          partes: [],
        });
      });
    } catch (e) {
      console.error(e);
      $('#m-aviso').textContent = `Não consegui ler "${f.name}": ${e}`;
    }
  }
  $('#m-progresso').textContent = 'Analisando partes…';
  await analisarMoldes();
  $('#m-progresso').textContent = '';
  renderPeds();
}

/* ---------- interface dos pedidos ---------- */

function novoPedido() {
  const p = { nome: '', cor: CORES_PED[mol.peds.length % CORES_PED.length], giro: 'none', moldes: [] };
  mol.peds.push(p);
  return p;
}

function updSoma() {
  let n = 0;
  for (const p of mol.peds) for (const m of p.moldes) n += (m.qtd || 0) * (m.partes?.length || 1);
  $('#m-soma').textContent = n ? `${n} peças no total` : '';
}

function renderPeds() {
  const box = $('#m-pedidos');
  box.innerHTML = '';
  mol.peds.forEach((p, pi) => {
    const el = document.createElement('div');
    el.className = 'ped';
    el.style.setProperty('--c', p.cor);
    el.innerHTML = `
      <div class="ped-h">
        <input type="text" placeholder="Nome do cliente / pedido" value="${esc(p.nome)}" data-k="nome">
        <label class="mini">Giro
          <select data-k="giro">
            <option value="none"${p.giro === 'none' ? ' selected' : ''}>Não girar</option>
            <option value="180"${p.giro === '180' ? ' selected' : ''}>Pode girar 180°</option>
            <option value="all"${p.giro === 'all' ? ' selected' : ''}>Pode girar 90° e 180°</option>
          </select>
        </label>
        <button data-a="dup" title="Novo pedido com os mesmos moldes">Duplicar</button>
        <button data-a="del">Excluir</button>
      </div>
      <div class="tbl"></div>
      <div class="drop" data-a="add">+ Adicionar moldes (PDF) — clique ou arraste os arquivos aqui</div>
      <input type="file" accept="application/pdf,.pdf" multiple hidden>`;
    const tbl = el.querySelector('.tbl');
    if (p.moldes.length) {
      const t = document.createElement('table');
      t.innerHTML = `<thead><tr><th>Molde</th><th></th><th>Partes</th><th>Qtd.</th><th></th></tr></thead><tbody></tbody>`;
      const tb = t.querySelector('tbody');
      p.moldes.forEach((m, mi) => {
        const partes = m.partes || [];
        const tr = document.createElement('tr');
        tr.innerHTML = `
          <td class="th">${partes.slice(0, 4).map((q) => `<img src="${q.thumb}" alt="">`).join('')}</td>
          <td>${esc(m.nome)}</td>
          <td class="mini">${partes.length} ${partes.length === 1 ? 'parte' : 'partes'}<br>${partes
            .map((q) => Math.round(q.wmm / 10) + '×' + Math.round(q.hmm / 10))
            .join(', ')} cm</td>
          <td class="q"><input type="number" min="0" step="1" value="${m.qtd || ''}" placeholder="0"></td>
          <td><button title="Remover">✕</button></td>`;
        const inp = tr.querySelector('input');
        inp.addEventListener('input', () => {
          m.qtd = +inp.value || 0;
          updSoma();
        });
        tr.querySelector('button').addEventListener('click', async () => {
          if (TAURI) await invoke('moldes_remover', { id: m.id });
          p.moldes.splice(mi, 1);
          renderPeds();
        });
        tb.appendChild(tr);
      });
      tbl.appendChild(t);
    }
    el.querySelector('[data-k="nome"]').addEventListener('input', (e) => (p.nome = e.target.value));
    el.querySelector('[data-k="giro"]').addEventListener('change', (e) => (p.giro = e.target.value));
    const fileInp = el.querySelector('input[type="file"]');
    el.querySelector('[data-a="add"]').addEventListener('click', () => fileInp.click());
    fileInp.addEventListener('change', async (e) => {
      const fs = [...e.target.files];
      e.target.value = '';
      await addMoldes(p, fs);
    });
    el.querySelector('[data-a="del"]').addEventListener('click', () => {
      mol.peds.splice(pi, 1);
      if (!mol.peds.length) novoPedido();
      renderPeds();
    });
    el.querySelector('[data-a="dup"]').addEventListener('click', () => {
      const q = novoPedido();
      q.giro = p.giro;
      q.moldes = p.moldes.map((m) => ({ ...m, qtd: 0 }));
      renderPeds();
    });
    el.addEventListener('dragover', (e) => {
      e.preventDefault();
      el.classList.add('drag');
    });
    el.addEventListener('dragleave', () => el.classList.remove('drag'));
    el.addEventListener('drop', async (e) => {
      e.preventDefault();
      el.classList.remove('drag');
      await addMoldes(p, [...e.dataTransfer.files]);
    });
    box.appendChild(el);
  });
  updSoma();
}

$('#m-add-pedido').addEventListener('click', () => {
  novoPedido();
  renderPeds();
});
['m-split', 'm-white', 'm-grid'].forEach((id) =>
  $('#' + id).addEventListener('change', async () => {
    if (!mol.peds.some((p) => p.moldes.length)) return;
    $('#m-progresso').textContent = 'Reanalisando partes…';
    await analisarMoldes();
    $('#m-progresso').textContent = '';
    renderPeds();
  })
);

/* ---------- encaixe ---------- */

$('#m-encaixar').addEventListener('click', async () => {
  if (!TAURI) {
    $('#m-aviso').textContent = 'O encaixe de moldes roda no app de desktop (núcleo Rust).';
    return;
  }
  const c = mcfg();
  $('#m-aviso').textContent = '';
  $('#m-progresso').textContent = 'Encaixando…';
  try {
    await analisarMoldes();
    const r = await invoke('moldes_encaixar', {
      cfg: {
        wMm: c.wMm,
        gapMm: c.gapMm,
        maxLenMm: c.maxLenMm,
        gridMm: c.gridMm,
        together: c.together,
        split: c.split,
        white: c.white,
      },
      pedidos: mol.peds.map((p) => ({
        giro: p.giro,
        moldes: p.moldes.map((m) => ({ molde: m.id, qtd: m.qtd || 0 })),
      })),
    });
    mol.result = { ...r, cfg: c };
    renderResultadoMoldes();
    $('#m-progresso').textContent = '';
  } catch (e) {
    console.error(e);
    $('#m-aviso').textContent = String(e);
    $('#m-progresso').textContent = '';
  }
});

/* ---------- prévia ---------- */

const thumbImgCache = new Map();
function thumbImg(moldeId, partIx, src) {
  const key = moldeId + ':' + partIx;
  if (!thumbImgCache.has(key)) {
    const img = new Image();
    img.src = src;
    thumbImgCache.set(key, img);
  }
  return thumbImgCache.get(key);
}

function parteInfo(moldeId, partIx) {
  for (const p of mol.peds) {
    const m = p.moldes.find((x) => x.id === moldeId);
    if (m) return m.partes[partIx];
  }
  return null;
}

function desenharParte(bin, cfg, maxW) {
  const sc = maxW / cfg.wMm;
  const cv = document.createElement('canvas');
  const dpr = window.devicePixelRatio || 1;
  const hCss = Math.max(20, bin.lenMm * sc);
  cv.style.width = maxW + 'px';
  cv.style.height = hCss + 'px';
  cv.width = Math.round(maxW * dpr);
  cv.height = Math.round(hCss * dpr);
  const ctx = cv.getContext('2d');
  ctx.scale(dpr, dpr);
  for (const it of bin.items) {
    const pt = parteInfo(it.molde, it.parte);
    if (!pt) continue;
    const x = (it.x * cfg.gridMm + it.offX) * sc;
    const y = (it.y * cfg.gridMm + it.offY) * sc;
    const w = it.rw * sc;
    const h = it.rh * sc;
    const img = thumbImg(it.molde, it.parte, pt.thumb);
    ctx.save();
    ctx.translate(x + w / 2, y + h / 2);
    ctx.rotate((-it.deg * Math.PI) / 180);
    const tw = pt.wmm * sc;
    const th = pt.hmm * sc;
    if (img.complete) ctx.drawImage(img, -tw / 2, -th / 2, tw, th);
    ctx.restore();
    ctx.strokeStyle = mol.peds[it.pedido]?.cor || '#333';
    ctx.lineWidth = 1.2;
    ctx.strokeRect(x + 0.5, y + 0.5, w - 1, h - 1);
  }
  return cv;
}

function renderResultadoMoldes() {
  const r = mol.result;
  const c = r.cfg;
  $('#m-resultado').hidden = false;
  const totLen = r.bins.reduce((s, b) => s + b.lenMm, 0);
  const totArea = r.bins.reduce((s, b) => s + b.areaMm2, 0);
  const aprove = totLen ? (totArea / (c.wMm * totLen)) * 100 : 0;
  const placed = r.bins.reduce((s, b) => s + b.items.length, 0);
  $('#m-stats').innerHTML = `
    <div class="stat"><b>${fmtM(totLen)}</b><span>de papel (${c.wMm / 10} cm)</span></div>
    <div class="stat"><b>${aprove.toLocaleString('pt-BR', { maximumFractionDigits: 1 })}%</b><span>aproveitamento</span></div>
    <div class="stat"><b>${r.bins.length}</b><span>${r.bins.length === 1 ? 'arquivo' : 'arquivos'} de até ${c.maxLenMm / 10} cm</span></div>
    <div class="stat"><b>${placed}${placed < r.total ? ' / ' + r.total : ''}</b><span>peças encaixadas</span></div>`;
  $('#m-legenda').innerHTML = mol.peds
    .filter((p) => p.moldes.some((m) => m.qtd > 0))
    .map((p) => `<span><i style="background:${p.cor}"></i>${esc(p.nome || 'Pedido')}</span>`)
    .join('');
  $('#m-erros').innerHTML = r.erros.map(esc).join('<br>');
  const box = $('#m-partes');
  box.innerHTML = '';
  const maxW = Math.min(300, (box.clientWidth || 900) - 24);
  r.bins.forEach((b, i) => {
    const d = document.createElement('div');
    d.className = 'parte';
    d.innerHTML = `<div class="mini"><b>Parte ${i + 1} de ${r.bins.length}</b> · ${fmtM(b.lenMm)} · ${b.items.length} peças</div>`;
    d.appendChild(desenharParte(b, c, maxW));
    box.appendChild(d);
  });
  $('#m-exportar').disabled = !r.bins.length;
}

/* ---------- exportação PDF vetorial (pdf-lib) ---------- */

const srcDocCache = new Map();
async function srcDocMolde(m) {
  if (!srcDocCache.has(m.id)) {
    srcDocCache.set(m.id, await PDFLib.PDFDocument.load(m.bytes.slice(0), { ignoreEncryption: true }));
  }
  return srcDocCache.get(m.id);
}

function moldePorId(id) {
  for (const p of mol.peds) {
    const m = p.moldes.find((x) => x.id === id);
    if (m) return m;
  }
  return null;
}

async function buildPdfMolde(bin, cfg) {
  const { PDFDocument } = PDFLib;
  const P = PDFLib;
  const out = await PDFDocument.create();
  out.setTitle('Encaixe de moldes');
  out.setCreator('Bancada — Encaixe de Moldes');
  const Wpt = cfg.wMm * MM_PT;
  const Hpt = bin.lenMm * MM_PT;
  const page = out.addPage([Wpt, Hpt]);
  const emb = new Map();
  for (const it of bin.items) {
    const m = moldePorId(it.molde);
    const pt = m?.partes?.[it.parte];
    if (!m || !pt) continue;
    const key = it.molde + ':' + it.parte;
    if (!emb.has(key)) {
      const src = await srcDocMolde(m);
      const sp = src.getPage(m.pageIndex);
      const [left, right, top, bottom] = pt.crop;
      const ep = await out.embedPage(sp, { left, right, top, bottom });
      emb.set(key, { e: ep, name: page.node.newXObject('Peca', ep.ref) });
    }
    const { e, name } = emb.get(key);
    const ew = pt.crop[1] - pt.crop[0];
    const eh = pt.crop[2] - pt.crop[3];
    const Xmm = it.x * cfg.gridMm + it.offX;
    const Ytop = it.y * cfg.gridMm + it.offY;
    const X = Xmm * MM_PT;
    const Y = Hpt - (Ytop + it.rh) * MM_PT;
    let x = X;
    let y = Y;
    if (it.deg === 90) x = X + eh;
    else if (it.deg === 180) { x = X + ew; y = Y + eh; }
    else if (it.deg === 270) y = Y + ew;
    const ops = [P.pushGraphicsState(), P.concatTransformationMatrix(1, 0, 0, 1, x, y), P.rotateDegrees(it.deg)];
    if (pt.clip && pt.clip.length) {
      for (const r of pt.clip) ops.push(P.rectangle(r[0], r[1], r[2], r[3]));
      ops.push(P.clip(), P.endPath());
    }
    ops.push(P.drawObject(name), P.popGraphicsState());
    page.pushOperators(...ops);
  }
  return await out.save();
}

function bytesParaB64(bytes) {
  let bin = '';
  const PASSO = 32768;
  for (let i = 0; i < bytes.length; i += PASSO) {
    bin += String.fromCharCode(...bytes.subarray(i, Math.min(i + PASSO, bytes.length)));
  }
  return btoa(bin);
}

$('#m-exportar').addEventListener('click', async () => {
  if (!TAURI || !mol.result) return;
  const r = mol.result;
  const pasta = await escolherPasta('Pasta de destino para os PDFs');
  if (!pasta) return;
  const bt = $('#m-exportar');
  bt.disabled = true;
  try {
    const arquivos = [];
    for (let i = 0; i < r.bins.length; i++) {
      $('#m-progresso').textContent = `Gerando parte ${i + 1} de ${r.bins.length}…`;
      const bytes = await buildPdfMolde(r.bins[i], r.cfg);
      arquivos.push({
        nome: `SUB ${hojeStr()} - Parte ${i + 1} de ${r.bins.length} - ${r.cfg.wMm / 10}cm.pdf`,
        b64: bytesParaB64(bytes),
      });
    }
    const salvos = await invoke('salvar_arquivos', { pasta, arquivos });
    $('#m-progresso').textContent = 'Salvo: ' + salvos.join(' · ');
  } catch (e) {
    console.error(e);
    $('#m-aviso').textContent = 'Erro ao gerar o PDF: ' + e;
  }
  bt.disabled = false;
});

novoPedido();
renderPeds();

/* ============================= Mockups =============================
   Aplica a logo em fotos de produtos (catálogo = pasta de imagens).
   Motor portado do mockup-studio: remoção de fundo, variantes de cor,
   acabamentos realistas (silk/bordado/laser/relevo/metálico) e régua em cm.
   Simplificações: uma logo por vez; posições e ajustes salvos na pasta. */

const mkCanvas = (w, h) => {
  const c = document.createElement('canvas');
  c.width = Math.max(1, Math.round(w));
  c.height = Math.max(1, Math.round(h));
  return c;
};
const ctx2d = (c) => c.getContext('2d', { willReadFrequently: true });
const clampN = (v, a, b) => (v < a ? a : v > b ? b : v);

const mkx = {
  pasta: null,
  files: [], // nomes de arquivo
  cfg: { palette: [], products: {}, colors: {} },
  prods: [], // [{ key, nome, colors: [{file, cor}] }]
  logo: { orig: null, proc: null, nome: '', palette: [], map: {}, vc: {}, dark: null },
  vars: ['original'], // variantes ativas
  unsel: new Set(),
  cfgSujo: false,
};

/* ---------------- logo: fundo, paleta, variantes ---------------- */

function borderColorMk(d, W, H) {
  const b = new Map();
  const add = (x, y) => {
    const o = (y * W + x) * 4;
    if (d[o + 3] < 200) return;
    const k = ((d[o] >> 4) << 8) | ((d[o + 1] >> 4) << 4) | (d[o + 2] >> 4);
    const e = b.get(k) || [0, 0, 0, 0];
    e[0] += d[o]; e[1] += d[o + 1]; e[2] += d[o + 2]; e[3]++;
    b.set(k, e);
  };
  for (let x = 0; x < W; x++) { add(x, 0); add(x, H - 1); }
  for (let y = 0; y < H; y++) { add(0, y); add(W - 1, y); }
  let best = [255, 255, 255], bn = -1;
  for (const e of b.values()) if (e[3] > bn) { bn = e[3]; best = [e[0] / e[3], e[1] / e[3], e[2] / e[3]]; }
  return best;
}

function removeBgMk(src, o) {
  const W = src.width, H = src.height, c = mkCanvas(W, H), x = ctx2d(c);
  x.drawImage(src, 0, 0);
  const id = x.getImageData(0, 0, W, H), d = id.data;
  const key = borderColorMk(d, W, H);
  const t = o.tol * 2.2, s = Math.max(1, o.soft * 2);
  const N = W * H, f = new Float32Array(N);
  for (let i = 0; i < N; i++) {
    const p = i * 4;
    const dd = Math.sqrt((d[p] - key[0]) ** 2 + (d[p + 1] - key[1]) ** 2 + (d[p + 2] - key[2]) ** 2);
    f[i] = dd <= t ? 0 : dd >= t + s ? 1 : (dd - t) / s;
  }
  let region = null;
  if (o.bgMode === 'outer') {
    region = new Uint8Array(N);
    const st = new Int32Array(N);
    let sp = 0;
    const push = (i) => { if (!region[i] && (f[i] < 1 || d[i * 4 + 3] < 16)) { region[i] = 1; st[sp++] = i; } };
    for (let xx = 0; xx < W; xx++) { push(xx); push((H - 1) * W + xx); }
    for (let y = 0; y < H; y++) { push(y * W); push(y * W + W - 1); }
    while (sp) {
      const i = st[--sp];
      if (f[i] >= 0.999 && d[i * 4 + 3] >= 16) continue;
      const xx = i % W;
      if (xx > 0) push(i - 1);
      if (xx < W - 1) push(i + 1);
      if (i >= W) push(i - W);
      if (i < N - W) push(i + W);
    }
  }
  for (let i = 0; i < N; i++) {
    if (region && !region[i]) continue;
    const a = f[i];
    if (a >= 1) continue;
    const p = i * 4;
    if (a <= 0.001) { d[p + 3] = 0; continue; }
    for (let k = 0; k < 3; k++) d[p + k] = clampN((d[p + k] - key[k] * (1 - a)) / a, 0, 255);
    d[p + 3] = d[p + 3] * a;
  }
  x.putImageData(id, 0, 0);
  return c;
}

function trimMk(c) {
  const d = ctx2d(c).getImageData(0, 0, c.width, c.height).data, W = c.width, H = c.height;
  let x0 = W, y0 = H, x1 = -1, y1 = -1;
  for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) {
    if (d[(y * W + x) * 4 + 3] > 10) {
      if (x < x0) x0 = x; if (x > x1) x1 = x;
      if (y < y0) y0 = y; if (y > y1) y1 = y;
    }
  }
  if (x1 < 0) return c;
  const p = 2;
  x0 = Math.max(0, x0 - p); y0 = Math.max(0, y0 - p);
  x1 = Math.min(W - 1, x1 + p); y1 = Math.min(H - 1, y1 + p);
  const o = mkCanvas(x1 - x0 + 1, y1 - y0 + 1);
  o.getContext('2d').drawImage(c, -x0, -y0);
  return o;
}

const dist2c = (a, b) => (a[0] - b[0]) ** 2 + (a[1] - b[1]) ** 2 + (a[2] - b[2]) ** 2;
const hexToRgbMk = (x) => {
  x = String(x || '').replace('#', '').trim();
  if (x.length === 3) x = x.split('').map((c) => c + c).join('');
  if (!/^[0-9a-f]{6}$/i.test(x)) return null;
  const n = parseInt(x, 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
};
const rgbToHexMk = (r, g, b) => '#' + [r, g, b].map((v) => clampN(Math.round(v), 0, 255).toString(16).padStart(2, '0')).join('');

function computePaletteMk(c, maxN = 6) {
  const d = ctx2d(c).getImageData(0, 0, c.width, c.height).data, n = d.length / 4;
  const step = Math.max(1, Math.floor(n / 25000));
  const px = [];
  for (let i = 0; i < n; i += step) {
    const o = i * 4;
    if (d[o + 3] > 200) px.push([d[o], d[o + 1], d[o + 2]]);
  }
  if (!px.length) return [];
  const cs = [px[0].slice()];
  while (cs.length < maxN) {
    let best = null, bd = -1;
    for (let i = 0; i < px.length; i += 5) {
      let m = 1e9;
      for (const q of cs) { const dd = dist2c(px[i], q); if (dd < m) m = dd; }
      if (m > bd) { bd = m; best = px[i]; }
    }
    if (bd < 900) break;
    cs.push(best.slice());
  }
  let cnt = [];
  for (let it = 0; it < 10; it++) {
    const sum = cs.map(() => [0, 0, 0]);
    cnt = cs.map(() => 0);
    for (const p of px) {
      let j = 0, m = 1e9;
      for (let k = 0; k < cs.length; k++) { const dd = dist2c(p, cs[k]); if (dd < m) { m = dd; j = k; } }
      sum[j][0] += p[0]; sum[j][1] += p[1]; sum[j][2] += p[2]; cnt[j]++;
    }
    cs.forEach((q, j) => { if (cnt[j]) { q[0] = sum[j][0] / cnt[j]; q[1] = sum[j][1] / cnt[j]; q[2] = sum[j][2] / cnt[j]; } });
  }
  const res = cs.map((q, j) => ({ rgb: q.map(Math.round), count: cnt[j] }))
    .filter((r) => r.count / px.length > 0.012)
    .sort((a, b) => b.count - a.count);
  const out = [];
  for (const r of res) if (!out.some((o) => dist2c(o.rgb, r.rgb) < 1600)) out.push(r);
  return out.map((r) => ({ rgb: r.rgb, hex: rgbToHexMk(...r.rgb), pct: r.count / px.length }));
}

function recolorMk(c, mode) {
  if (mode === 'original') return c;
  const W = c.width, H = c.height, o = mkCanvas(W, H), x = ctx2d(o);
  x.drawImage(c, 0, 0);
  const id = x.getImageData(0, 0, W, H), d = id.data;
  if (Array.isArray(mode)) {
    for (let p = 0; p < d.length; p += 4) { d[p] = mode[0]; d[p + 1] = mode[1]; d[p + 2] = mode[2]; }
  } else if (mode === 'remap') {
    const pal = mkx.logo.palette;
    const tg = pal.map((p) => hexToRgbMk(mkx.logo.map[p.hex] || '') || null);
    if (tg.some(Boolean)) {
      for (let p = 0; p < d.length; p += 4) {
        if (!d[p + 3]) continue;
        const pxc = [d[p], d[p + 1], d[p + 2]];
        let j = 0, m = 1e9;
        for (let k = 0; k < pal.length; k++) { const dd = dist2c(pxc, pal[k].rgb); if (dd < m) { m = dd; j = k; } }
        const t = tg[j];
        if (!t) continue;
        for (let k = 0; k < 3; k++) d[p + k] = clampN(pxc[k] + t[k] - pal[j].rgb[k], 0, 255);
      }
    }
  }
  x.putImageData(id, 0, 0);
  return o;
}

function processLogoMk() {
  const L = mkx.logo;
  L.vc = {};
  L.dark = null;
  if (!L.orig) { L.proc = null; return; }
  let c = $('#mk-rmbg').checked
    ? removeBgMk(L.orig, { tol: +$('#mk-tol').value || 30, soft: +$('#mk-soft').value || 14, bgMode: $('#mk-bgmode').value })
    : L.orig;
  c = trimMk(c);
  L.proc = c;
  L.palette = computePaletteMk(c);
  // Mantém remaps de cores próximas
  const old = L.map || {};
  const nm = {};
  for (const p of L.palette) {
    const k = Object.keys(old).find((h0) => { const r = hexToRgbMk(h0); return r && dist2c(r, p.rgb) < 900; });
    if (k) nm[p.hex] = old[k];
  }
  L.map = nm;
  buildVariantesMk();
}

function todasVariantes() {
  const v = [{ id: 'original' }];
  for (const hx of mkx.cfg.palette || []) v.push({ id: 'hex:' + hx, hex: hx });
  v.push({ id: 'remap' });
  return v;
}

function variantesAtivas() {
  const a = todasVariantes().filter((v) => mkx.vars.includes(v.id));
  return a.length ? a : [{ id: 'original' }];
}

function buildVariantesMk() {
  const L = mkx.logo;
  L.vc = {};
  L.dark = null;
  if (!L.proc) return;
  for (const v of todasVariantes()) {
    L.vc[v.id] = v.id === 'original' ? L.proc : v.id === 'remap' ? recolorMk(L.proc, 'remap') : recolorMk(L.proc, hexToRgbMk(v.hex));
  }
  if ($('#mk-darkalt').checked) L.dark = recolorMk(L.proc, [255, 255, 255]);
  const cv = $('#mk-logo-canvas');
  cv.width = L.proc.width;
  cv.height = L.proc.height;
  cv.getContext('2d').drawImage(L.proc, 0, 0);
  $('#mk-logo-prev').hidden = false;
  $('#mk-logo-meta').textContent = `${L.nome} · ${L.orig.width}×${L.orig.height}px`;
  renderChipsVariantes();
  mkx.thumbCache?.clear();
  renderGrade();
}

function renderChipsVariantes() {
  const box = $('#mk-variantes');
  box.innerHTML = '';
  const dot = (v) =>
    v.id === 'original'
      ? '<span class="dot" style="background:conic-gradient(#e53935,#fdd835,#43a047,#1e88e5,#8e24aa,#e53935)"></span>'
      : v.id === 'remap'
        ? '<span style="font-size:11px">⇄</span>'
        : `<span class="dot" style="background:${v.hex}"></span>`;
  const nome = (v) => (v.id === 'original' ? 'Original' : v.id === 'remap' ? 'Cores trocadas' : v.hex);
  for (const v of todasVariantes()) {
    if (v.id === 'remap' && !mkx.logo.palette.length) continue;
    const el = document.createElement('span');
    el.className = 'chip' + (mkx.vars.includes(v.id) ? ' on' : '');
    el.innerHTML = dot(v) + nome(v);
    el.addEventListener('click', () => {
      const i = mkx.vars.indexOf(v.id);
      if (i >= 0) mkx.vars.splice(i, 1);
      else mkx.vars.push(v.id);
      if (!mkx.vars.length) mkx.vars = ['original'];
      renderChipsVariantes();
      mkx.thumbCache?.clear();
      renderGrade();
      renderRemapUI();
    });
    box.appendChild(el);
  }
  renderRemapUI();
}

let remapBox = null;
function renderRemapUI() {
  const host = $('#mk-variantes');
  if (!remapBox) {
    remapBox = document.createElement('div');
    remapBox.id = 'mk-remap';
  }
  remapBox.innerHTML = '';
  if (mkx.vars.includes('remap') && mkx.logo.palette.length) {
    for (const p of mkx.logo.palette) {
      const row = document.createElement('div');
      row.className = 'linha-mk';
      const de = document.createElement('span');
      de.className = 'dot';
      de.style.cssText = `width:14px;height:14px;border-radius:4px;background:${p.hex};border:1px solid rgba(0,0,0,.2)`;
      const inp = document.createElement('input');
      inp.type = 'color';
      inp.value = mkx.logo.map[p.hex] || p.hex;
      inp.title = `Trocar ${p.hex}`;
      inp.addEventListener('change', () => {
        if (inp.value.toLowerCase() === p.hex) delete mkx.logo.map[p.hex];
        else mkx.logo.map[p.hex] = inp.value;
        buildVariantesMk();
      });
      row.append(de, inp);
      remapBox.appendChild(row);
    }
  }
  host.appendChild(remapBox);
}

$('#mk-logo').addEventListener('change', async (ev) => {
  const f = ev.target.files[0];
  ev.target.value = '';
  if (!f) return;
  const img = await carregarImagem(f);
  const c = mkCanvas(img.width, img.height);
  c.getContext('2d').drawImage(img, 0, 0);
  mkx.logo.orig = c;
  mkx.logo.nome = f.name;
  mkx.logo.map = {};
  processLogoMk();
  $('#mk-salvar').disabled = !(mkx.prods.length && mkx.logo.proc);
});
['mk-tol', 'mk-soft'].forEach((id) => $('#' + id).addEventListener('change', processLogoMk));
$('#mk-bgmode').addEventListener('change', processLogoMk);
$('#mk-rmbg').addEventListener('change', () => {
  $('#mk-rmbg-ops').hidden = !$('#mk-rmbg').checked;
  processLogoMk();
});
$('#mk-darkalt').addEventListener('change', buildVariantesMk);
$('#mk-add-cor').addEventListener('click', () => {
  const hx = $('#mk-cor-nova').value;
  if (!mkx.cfg.palette.includes(hx)) mkx.cfg.palette.push(hx);
  if (!mkx.vars.includes('hex:' + hx)) mkx.vars.push('hex:' + hx);
  mkx.cfgSujo = true;
  buildVariantesMk();
});
['mk-finish', 'mk-shade', 'mk-sizetext'].forEach((id) =>
  $('#' + id).addEventListener('change', () => {
    mkx.thumbCache?.clear();
    renderGrade();
  })
);

/* ---------------- catálogo (pasta de fotos) ---------------- */

function parseNomeArquivo(fn) {
  const base = fn.replace(/\.[^.]+$/, '').trim();
  const m = base.match(/^(.*?)(\d+)(?:_(\d+))?$/);
  if (m && m[1].trim()) {
    const idx = m[3] ? parseInt(m[3], 10) + 1 : 1;
    return { grupo: m[1].trim(), cor: 'Cor ' + idx, ordem: idx };
  }
  const parts = base.split(/\s+/);
  if (parts.length === 1) return { grupo: base, cor: 'Única', ordem: 0 };
  const cor = parts.pop();
  return { grupo: parts.join(' '), cor: cor.charAt(0).toUpperCase() + cor.slice(1).toLowerCase(), ordem: 0 };
}

function normMk(s) {
  return String(s).normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase().trim();
}

function buildProds() {
  const map = new Map();
  for (const f of mkx.files) {
    const p = parseNomeArquivo(f);
    const key = normMk(p.grupo);
    if (!map.has(key)) map.set(key, { key, nome: p.grupo, colors: [] });
    map.get(key).colors.push({ file: f, cor: p.cor, ordem: p.ordem });
  }
  mkx.prods = [...map.values()].map((p) => {
    p.colors.sort((a, b) => a.ordem - b.ordem || a.file.localeCompare(b.file, 'pt'));
    return p;
  }).sort((a, b) => a.nome.localeCompare(b.nome, 'pt'));
}

function pcfgMk(key) {
  let po = mkx.cfg.products[key];
  if (!po) po = mkx.cfg.products[key] = {};
  if (!po.positions || !po.positions.length) {
    po.positions = [{ id: 'centro', name: 'Centro', x: 0.38, y: 0.42, w: 0.24, h: 0.18, rot: 0, s: 0.85, dx: 0, dy: 0, cm: 0 }];
  }
  return po;
}

$('#mk-procurar-pasta').addEventListener('click', async () => {
  const pasta = await escolherPasta('Pasta com as fotos dos produtos');
  if (pasta) $('#mk-pasta').value = pasta;
});

$('#mk-abrir-pasta').addEventListener('click', async () => {
  if (!TAURI) {
    $('#mk-aviso').textContent = 'O catálogo roda no app de desktop (núcleo Rust).';
    return;
  }
  const pasta = $('#mk-pasta').value.trim();
  if (!pasta) {
    $('#mk-aviso').textContent = 'Informe a pasta com as fotos dos produtos.';
    return;
  }
  $('#mk-aviso').textContent = '';
  $('#mk-progresso').textContent = 'Lendo catálogo…';
  try {
    const r = await invoke('catalogo_ler', { pasta });
    mkx.pasta = pasta;
    mkx.files = r.imagens;
    mkx.cfg = r.config ? JSON.parse(r.config) : { palette: [], products: {}, colors: {} };
    if (!mkx.cfg.palette) mkx.cfg.palette = [];
    if (!mkx.cfg.products) mkx.cfg.products = {};
    if (!mkx.cfg.colors) mkx.cfg.colors = {};
    mkx.imgCache = new Map();
    mkx.thumbCache = new Map();
    buildProds();
    $('#mk-cat-info').textContent = `${mkx.files.length} foto(s) · ${mkx.prods.length} produto(s)`;
    $('#mk-salvar-cfg').disabled = false;
    $('#mk-salvar').disabled = !(mkx.prods.length && mkx.logo.proc);
    renderGrade();
  } catch (e) {
    $('#mk-aviso').textContent = String(e);
  }
  $('#mk-progresso').textContent = '';
});

$('#mk-salvar-cfg').addEventListener('click', async () => {
  if (!TAURI || !mkx.pasta) return;
  try {
    await invoke('escrever_arquivo_texto', {
      pasta: mkx.pasta,
      nome: '_bancada-mockups.json',
      texto: JSON.stringify(mkx.cfg, null, 2),
    });
    mkx.cfgSujo = false;
    $('#mk-progresso').textContent = 'Configurações salvas na pasta do catálogo.';
    setTimeout(() => ($('#mk-progresso').textContent = ''), 2500);
  } catch (e) {
    $('#mk-aviso').textContent = String(e);
  }
});

async function imgDoArquivo(file) {
  if (!mkx.imgCache) mkx.imgCache = new Map();
  if (mkx.imgCache.has(file)) return mkx.imgCache.get(file);
  const b64 = await invoke('ler_arquivo_b64', { pasta: mkx.pasta, nome: file });
  const img = new Image();
  img.src = 'data:image/*;base64,' + b64;
  await img.decode();
  mkx.imgCache.set(file, img);
  return img;
}

/* ---------------- render do mockup ---------------- */

const lumMk = (b, i) => (0.2126 * b[i] + 0.7152 * b[i + 1] + 0.0722 * b[i + 2]) / 255;

function regionLumMk(ctx, x, y, w, hh) {
  x = Math.max(0, Math.floor(x));
  y = Math.max(0, Math.floor(y));
  w = Math.max(1, Math.min(ctx.canvas.width - x, Math.floor(w)));
  hh = Math.max(1, Math.min(ctx.canvas.height - y, Math.floor(hh)));
  const d = ctx.getImageData(x, y, w, hh).data;
  let s = 0, n = 0;
  const st = Math.max(4, Math.floor(d.length / 4 / 4000) * 4);
  for (let i = 0; i < d.length; i += st) { s += lumMk(d, i); n++; }
  return n ? s / n : 0.5;
}

function isDarkMk(file, ctx, box) {
  const tone = (mkx.cfg.colors[file] || {}).tone || 'auto';
  if (tone === 'clara') return false;
  if (tone === 'escura') return true;
  return regionLumMk(ctx, box[0], box[1], box[2], box[3]) < 0.42;
}

function ppcMk(key, W) {
  const po = mkx.cfg.products[key] || {};
  const r = po.ruler;
  if (!r || !(r.cm > 0)) return 0;
  return (Math.abs(r.x2 - r.x1) * W) / r.cm;
}

function logoRectMk(pos, W, H, logo, key) {
  const bx = pos.x * W, by = pos.y * H, bw = pos.w * W, bh = pos.h * H;
  const lr = logo.width / logo.height;
  const s = pos.s ?? 0.85;
  let lw = bw * s, lh = lw / lr;
  if (lh > bh * s) { lh = bh * s; lw = lh * lr; }
  if (pos.cm > 0 && key) {
    const k = ppcMk(key, W);
    if (k) { lw = pos.cm * k; lh = lw / lr; }
  }
  return {
    bx, by, bw, bh, lw, lh,
    cx: bx + bw / 2 + (pos.dx || 0) * bw,
    cy: by + bh / 2 + (pos.dy || 0) * bh,
    rot: ((pos.rot || 0) * Math.PI) / 180,
  };
}

const PHR_MK = [['costa', 'das costas'], ['frente', 'da frente'], ['peito', 'do peito'], ['manga', 'da manga'], ['centro', 'do centro'], ['lateral', 'da lateral'], ['bolso', 'do bolso'], ['nuca', 'da nuca'], ['gola', 'da gola'], ['tampa', 'da tampa'], ['em cima', 'em cima'], ['embaixo', 'embaixo'], ['topo', 'do topo']];
function frasePos(ps) {
  const n = normMk(ps.name || '');
  const m = PHR_MK.find(([k]) => n.startsWith(k));
  return m ? m[1] : 'em ' + (ps.name || '').toLowerCase();
}
const fmtCmMk = (v) => v.toLocaleString('pt-BR', { maximumFractionDigits: 1 });

function drawSizeTextMk(ctx, W, H, key, positions) {
  if (!$('#mk-sizetext').checked) return;
  const k = ppcMk(key, W);
  if (!k) return;
  const lines = [];
  for (const pos of positions) {
    const r = logoRectMk(pos, W, H, mkx.logo.proc, key);
    const cw = r.lw / k, ch = r.lh / k;
    lines.push(`Aplicação da logo ${frasePos(pos)} aproximadamente: ${fmtCmMk(Math.max(cw, ch))} cm de ${cw >= ch ? 'largura' : 'altura'}`);
  }
  if (!lines.length) return;
  const fs = Math.max(11, Math.round(W / 52)), pad = Math.round(fs * 0.55), lh = Math.round(fs * 1.35);
  ctx.save();
  ctx.font = `600 ${fs}px "Segoe UI", system-ui, sans-serif`;
  ctx.textBaseline = 'top';
  const tw = Math.max(...lines.map((l) => ctx.measureText(l).width));
  const x = Math.round(W * 0.02), y = Math.round(W * 0.02);
  const bw = tw + pad * 2, bh = lines.length * lh + pad * 2 - (lh - fs);
  ctx.fillStyle = 'rgba(255,255,255,.88)';
  const rr = fs * 0.4;
  ctx.beginPath();
  ctx.moveTo(x + rr, y);
  ctx.arcTo(x + bw, y, x + bw, y + bh, rr);
  ctx.arcTo(x + bw, y + bh, x, y + bh, rr);
  ctx.arcTo(x, y + bh, x, y, rr);
  ctx.arcTo(x, y, x + bw, y, rr);
  ctx.fill();
  ctx.fillStyle = '#1d1c1a';
  lines.forEach((l, i) => ctx.fillText(l, x + pad, y + pad + i * lh));
  ctx.restore();
}

function finishPixelsMk(l, b, w, hh, dark, u, ox, oy) {
  const f = $('#mk-finish').value;
  if (f === 'nenhum') return;
  const sh = (+$('#mk-shade').value || 0) / 100;
  let sum = 0, n = 0;
  for (let i = 0; i < l.length; i += 4) {
    const a = l[i + 3];
    if (a > 20) { sum += lumMk(b, i) * a; n += a; }
  }
  const lm = n ? sum / n : 0.5;
  const k = Math.max(1, Math.round(1.3 * u));
  const pal = f === 'dourado' ? [[138, 103, 34], [246, 226, 150]] : [[120, 125, 132], [244, 245, 247]];
  for (let y = 0; y < hh; y++) for (let x = 0; x < w; x++) {
    const i = (y * w + x) * 4, a = l[i + 3];
    if (!a) continue;
    const bl = lumMk(b, i);
    let fac = 1 + sh * ((bl + 0.05) / (lm + 0.05) - 1);
    fac = fac < 0.35 ? 0.35 : fac > 1.65 ? 1.65 : fac;
    let r = l[i], g = l[i + 1], bb = l[i + 2], na = a;
    if (f === 'silk') { r *= fac; g *= fac; bb *= fac; na = a * 0.96; }
    else if (f === 'bordado') { const s2 = fac * (0.84 + 0.24 * (0.5 + 0.5 * Math.sin(((x + ox) + (y + oy)) * 1.9 / u))); r *= s2; g *= s2; bb *= s2; }
    else if (f === 'laser') { const m = dark ? 1.65 : 0.55, ad = dark ? 16 : 0; r = b[i] * m + ad; g = b[i + 1] * m + ad; bb = b[i + 2] * m + ad; na = a * 0.93; }
    else if (f === 'relevo') {
      const ia = y >= k && x >= k ? l[((y - k) * w + (x - k)) * 4 + 3] : 0;
      const ib = y + k < hh && x + k < w ? l[((y + k) * w + (x + k)) * 4 + 3] : 0;
      const m = 0.96 + (0.55 * (ib - ia)) / 255;
      r = b[i] * m; g = b[i + 1] * m; bb = b[i + 2] * m;
    } else { // dourado / prateado
      const t = 0.5 + 0.5 * Math.sin(((x + ox) * 0.55 + (y + oy)) / (16 * u));
      const ff = Math.pow(fac, 1.4);
      r = (pal[0][0] + (pal[1][0] - pal[0][0]) * t) * ff;
      g = (pal[0][1] + (pal[1][1] - pal[0][1]) * t) * ff;
      bb = (pal[0][2] + (pal[1][2] - pal[0][2]) * t) * ff;
    }
    l[i] = r > 255 ? 255 : r;
    l[i + 1] = g > 255 ? 255 : g;
    l[i + 2] = bb > 255 ? 255 : bb;
    l[i + 3] = na;
  }
}

function applyAtMk(ctx, W, H, pos, file, u, vid, key) {
  const LG = mkx.logo;
  if (!LG.proc) return;
  const r0 = logoRectMk(pos, W, H, LG.proc, key);
  const dark = isDarkMk(file, ctx, [r0.bx, r0.by, r0.bw, r0.bh]);
  const logo = dark && $('#mk-darkalt').checked && LG.dark && !vid.startsWith('hex:') ? LG.dark : LG.vc[vid] || LG.proc;
  const { lw, lh, cx, cy, rot } = r0;
  const diag = Math.ceil(Math.hypot(lw, lh)) + 4;
  const x0 = Math.max(0, Math.floor(cx - diag / 2)), y0 = Math.max(0, Math.floor(cy - diag / 2));
  const x1 = Math.min(W, Math.ceil(cx + diag / 2)), y1 = Math.min(H, Math.ceil(cy + diag / 2));
  if (x1 - x0 < 1 || y1 - y0 < 1) return;
  const L = mkCanvas(x1 - x0, y1 - y0), lc = ctx2d(L);
  lc.imageSmoothingQuality = 'high';
  lc.translate(cx - x0, cy - y0);
  lc.rotate(rot);
  lc.drawImage(logo, -lw / 2, -lh / 2, lw, lh);
  lc.setTransform(1, 0, 0, 1, 0, 0);
  const ld = lc.getImageData(0, 0, L.width, L.height), bd = ctx.getImageData(x0, y0, L.width, L.height);
  finishPixelsMk(ld.data, bd.data, L.width, L.height, dark, u, x0, y0);
  lc.putImageData(ld, 0, 0);
  ctx.save();
  const f = $('#mk-finish').value;
  if (f === 'bordado') { ctx.shadowColor = 'rgba(0,0,0,.38)'; ctx.shadowBlur = 1.8 * u; ctx.shadowOffsetY = 1 * u; ctx.shadowOffsetX = 0.4 * u; }
  else if (f === 'dourado' || f === 'prateado') { ctx.shadowColor = 'rgba(0,0,0,.28)'; ctx.shadowBlur = 1.2 * u; ctx.shadowOffsetY = 0.7 * u; }
  ctx.drawImage(L, x0, y0);
  ctx.restore();
}

async function renderMockup(prod, file, vid, alvoW) {
  const base = await imgDoArquivo(file);
  const esc = alvoW ? alvoW / base.width : 1;
  const W = Math.round(base.width * esc), H = Math.round(base.height * esc);
  const out = mkCanvas(W, H), ctx = ctx2d(out);
  ctx.imageSmoothingQuality = 'high';
  ctx.drawImage(base, 0, 0, W, H);
  if (!mkx.logo.proc) return out;
  const po = pcfgMk(prod.key);
  const u = W / 1200;
  for (const pos of po.positions) applyAtMk(ctx, W, H, pos, file, u, vid, prod.key);
  drawSizeTextMk(ctx, W, H, prod.key, po.positions);
  return out;
}

/* ---------------- grade de resultados ---------------- */

let gradeObserver = null;

function cardsMockup() {
  const it = [];
  if (!mkx.logo.proc) return it;
  for (const p of mkx.prods) {
    for (const c of p.colors) {
      for (const v of variantesAtivas()) {
        it.push({ prod: p, file: c.file, cor: c.cor, v, k: p.key + '|' + c.file + '|' + v.id });
      }
    }
  }
  return it;
}

function nomeVariante(v) {
  return v.id === 'original' ? 'original' : v.id === 'remap' ? 'cores trocadas' : v.hex.toUpperCase();
}

function renderGrade() {
  const host = $('#mk-grade');
  host.innerHTML = '';
  gradeObserver?.disconnect();
  const itens = cardsMockup();
  $('#mk-vazio').hidden = itens.length > 0;
  if (!itens.length) {
    $('#mk-salvar').disabled = !(mkx.prods.length && mkx.logo.proc);
    return;
  }
  $('#mk-salvar').disabled = false;
  if (!mkx.thumbCache) mkx.thumbCache = new Map();
  gradeObserver = new IntersectionObserver((entries) => {
    for (const e of entries) {
      if (!e.isIntersecting) continue;
      gradeObserver.unobserve(e.target);
      const cv = e.target.querySelector('canvas');
      const it = cv._item;
      const cached = mkx.thumbCache.get(it.k);
      if (cached) {
        cv.getContext('2d').drawImage(cached, 0, 0);
        continue;
      }
      renderMockup(it.prod, it.file, it.v.id, 480).then((m) => {
        cv.width = m.width;
        cv.height = m.height;
        cv.getContext('2d').drawImage(m, 0, 0);
        mkx.thumbCache.set(it.k, m);
      });
    }
  }, { root: $('#mockups-main'), rootMargin: '200px' });

  for (const it of itens) {
    const card = document.createElement('div');
    card.className = 'mk-card' + (mkx.unsel.has(it.k) ? ' off' : '');
    const cv = document.createElement('canvas');
    cv._item = it;
    card.innerHTML = `
      <div class="ph"></div>
      <label class="pick"><input type="checkbox" ${mkx.unsel.has(it.k) ? '' : 'checked'} title="Incluir ao salvar"></label>
      <div class="cap"><b>${esc(it.prod.nome)}</b><span>${esc(it.cor)} · ${esc(nomeVariante(it.v))}</span></div>
      <div class="acoes-card"><button data-a="pos">Posições</button></div>`;
    card.querySelector('.ph').appendChild(cv);
    card.querySelector('input').addEventListener('change', (e2) => {
      if (e2.target.checked) mkx.unsel.delete(it.k);
      else mkx.unsel.add(it.k);
      card.classList.toggle('off', !e2.target.checked);
    });
    card.querySelector('[data-a="pos"]').addEventListener('click', () => abrirEditorPos(it.prod));
    host.appendChild(card);
    gradeObserver.observe(card);
  }
}

/* ---------------- editor de posições ---------------- */

const edMk = { prod: null, img: null, sel: null, drag: null };

async function abrirEditorPos(prod) {
  if (!TAURI) return;
  edMk.prod = prod;
  edMk.sel = null;
  edMk.img = await imgDoArquivo(prod.colors[0].file);
  $('#mk-ed-titulo').textContent = `Posições — ${prod.nome}`;
  const img = $('#mk-ed-img');
  img.src = edMk.img.src;
  await img.decode();
  $('#mk-editor').hidden = false;
  const po = pcfgMk(prod.key);
  $('#mk-ed-ruler-on').checked = !!po.ruler;
  $('#mk-ed-ruler-cm').value = po.ruler?.cm || '';
  renderEdTudo();
}

function edEscala() {
  const img = $('#mk-ed-img');
  return { w: img.clientWidth, h: img.clientHeight };
}

function renderEdTudo() {
  renderEdBoxes();
  renderEdPosList();
  renderEdRuler();
}

function renderEdBoxes() {
  const host = $('#mk-ed-boxes');
  host.innerHTML = '';
  const { w, h } = edEscala();
  const po = pcfgMk(edMk.prod.key);
  for (const pos of po.positions) {
    const el = document.createElement('div');
    el.className = 'mk-box' + (edMk.sel === pos.id ? ' sel' : '');
    el.style.left = pos.x * w + 'px';
    el.style.top = pos.y * h + 'px';
    el.style.width = pos.w * w + 'px';
    el.style.height = pos.h * h + 'px';
    el.style.transform = `rotate(${pos.rot || 0}deg)`;
    el.innerHTML = `<span class="bl">${esc(pos.name)}</span><span class="hd"></span>`;
    el.addEventListener('mousedown', (e) => {
      if (e.target.classList.contains('hd')) return;
      edMk.sel = pos.id;
      edMk.drag = { tipo: 'move', pos, x0: e.clientX, y0: e.clientY, ox: pos.x, oy: pos.y };
      renderEdTudo();
      e.preventDefault();
    });
    el.querySelector('.hd').addEventListener('mousedown', (e) => {
      edMk.sel = pos.id;
      edMk.drag = { tipo: 'resize', pos, x0: e.clientX, y0: e.clientY, ow: pos.w, oh: pos.h };
      renderEdTudo();
      e.preventDefault();
    });
    host.appendChild(el);
  }
}

window.addEventListener('mousemove', (e) => {
  const d = edMk.drag;
  if (!d) return;
  const { w, h } = edEscala();
  if (d.tipo === 'move') {
    d.pos.x = clampN(d.ox + (e.clientX - d.x0) / w, 0, 1 - d.pos.w);
    d.pos.y = clampN(d.oy + (e.clientY - d.y0) / h, 0, 1 - d.pos.h);
  } else if (d.tipo === 'resize') {
    d.pos.w = clampN(d.ow + (e.clientX - d.x0) / w, 0.03, 1 - d.pos.x);
    d.pos.h = clampN(d.oh + (e.clientY - d.y0) / h, 0.03, 1 - d.pos.y);
  } else if (d.tipo === 'ruler') {
    const po = pcfgMk(edMk.prod.key);
    const r = po.ruler;
    const v = clampN(d.ox + (e.clientX - d.x0) / w, 0.01, 0.99);
    r[d.qual] = v;
    if (r.x1 > r.x2) [r.x1, r.x2] = [r.x2, r.x1];
    renderEdRuler();
    return;
  }
  mkx.cfgSujo = true;
  renderEdBoxes();
});
window.addEventListener('mouseup', () => {
  if (edMk.drag) {
    edMk.drag = null;
    renderEdTudo();
    syncEdOps();
  }
});

function renderEdPosList() {
  const host = $('#mk-ed-poslist');
  host.innerHTML = '';
  const po = pcfgMk(edMk.prod.key);
  for (const pos of po.positions) {
    const el = document.createElement('div');
    el.className = 'pos' + (edMk.sel === pos.id ? ' sel' : '');
    el.innerHTML = `<b>${esc(pos.name)}</b><span class="fraco">${Math.round(pos.w * 100)}×${Math.round(pos.h * 100)}%</span><button title="Remover">✕</button>`;
    el.addEventListener('click', (e) => {
      if (e.target.tagName === 'BUTTON') return;
      edMk.sel = pos.id;
      renderEdTudo();
      syncEdOps();
    });
    el.querySelector('button').addEventListener('click', () => {
      po.positions = po.positions.filter((q) => q.id !== pos.id);
      if (edMk.sel === pos.id) edMk.sel = null;
      mkx.cfgSujo = true;
      renderEdTudo();
      syncEdOps();
    });
    host.appendChild(el);
  }
}

function posSelEd() {
  const po = pcfgMk(edMk.prod.key);
  return po.positions.find((q) => q.id === edMk.sel) || null;
}

function syncEdOps() {
  const pos = posSelEd();
  $('#mk-ed-pos-ops').hidden = !pos;
  if (!pos) return;
  $('#mk-ed-nome').value = pos.name;
  $('#mk-ed-s').value = Math.round((pos.s ?? 0.85) * 100);
  $('#mk-ed-cm').value = pos.cm || '';
  $('#mk-ed-rot').value = pos.rot || 0;
}

$('#mk-ed-add').addEventListener('click', () => {
  const po = pcfgMk(edMk.prod.key);
  const id = 'pos' + Date.now().toString(36);
  po.positions.push({ id, name: 'Nova posição', x: 0.3, y: 0.3, w: 0.3, h: 0.25, rot: 0, s: 0.85, dx: 0, dy: 0, cm: 0 });
  edMk.sel = id;
  mkx.cfgSujo = true;
  renderEdTudo();
  syncEdOps();
});
$('#mk-ed-nome').addEventListener('input', () => { const p = posSelEd(); if (p) { p.name = $('#mk-ed-nome').value; mkx.cfgSujo = true; renderEdBoxes(); renderEdPosList(); } });
$('#mk-ed-s').addEventListener('input', () => { const p = posSelEd(); if (p) { p.s = (+$('#mk-ed-s').value || 85) / 100; p.cm = 0; mkx.cfgSujo = true; } });
$('#mk-ed-cm').addEventListener('change', () => { const p = posSelEd(); if (p) { p.cm = +$('#mk-ed-cm').value || 0; mkx.cfgSujo = true; } });
$('#mk-ed-rot').addEventListener('change', () => { const p = posSelEd(); if (p) { p.rot = +$('#mk-ed-rot').value || 0; mkx.cfgSujo = true; renderEdBoxes(); } });

function renderEdRuler() {
  const po = pcfgMk(edMk.prod.key);
  const rEl = $('#mk-ed-ruler');
  if (!po.ruler || !$('#mk-ed-ruler-on').checked) { rEl.hidden = true; return; }
  const { w, h } = edEscala();
  const r = po.ruler;
  rEl.hidden = false;
  rEl.style.left = r.x1 * w + 'px';
  rEl.style.top = r.y * h + 'px';
  rEl.style.width = (r.x2 - r.x1) * w + 'px';
  $('#mk-ed-ruler-label').textContent = r.cm ? `${fmtCmMk(r.cm)} cm` : 'defina os cm →';
  $('#mk-ed-rh1').style.left = '-7px';
  $('#mk-ed-rh2').style.left = (r.x2 - r.x1) * w - 7 + 'px';
}

$('#mk-ed-ruler-on').addEventListener('change', () => {
  const po = pcfgMk(edMk.prod.key);
  if ($('#mk-ed-ruler-on').checked && !po.ruler) po.ruler = { x1: 0.15, x2: 0.85, y: 0.9, cm: 0 };
  if (!$('#mk-ed-ruler-on').checked) delete po.ruler;
  mkx.cfgSujo = true;
  renderEdRuler();
});
$('#mk-ed-ruler-cm').addEventListener('change', () => {
  const po = pcfgMk(edMk.prod.key);
  if (!po.ruler) return;
  po.ruler.cm = +$('#mk-ed-ruler-cm').value || 0;
  mkx.cfgSujo = true;
  renderEdRuler();
});
$('#mk-ed-rh1').addEventListener('mousedown', (e) => {
  const r = pcfgMk(edMk.prod.key).ruler;
  edMk.drag = { tipo: 'ruler', qual: 'x1', x0: e.clientX, ox: r.x1 };
  e.preventDefault();
});
$('#mk-ed-rh2').addEventListener('mousedown', (e) => {
  const r = pcfgMk(edMk.prod.key).ruler;
  edMk.drag = { tipo: 'ruler', qual: 'x2', x0: e.clientX, ox: r.x2 };
  e.preventDefault();
});

$('#mk-ed-fechar').addEventListener('click', () => {
  $('#mk-editor').hidden = true;
  edMk.prod = null;
  mkx.thumbCache?.clear();
  renderGrade();
  if (mkx.cfgSujo) $('#mk-progresso').textContent = 'Posições alteradas — clique em "Salvar configurações" para gravar.';
});

/* ---------------- exportação ---------------- */

$('#mk-salvar').addEventListener('click', async () => {
  if (!TAURI || !mkx.logo.proc) return;
  const itens = cardsMockup().filter((i) => !mkx.unsel.has(i.k));
  if (!itens.length) {
    $('#mk-aviso').textContent = 'Nenhuma imagem selecionada.';
    return;
  }
  const pasta = (mkx.pasta || 'C:\\Users\\Muller PC\\Desktop') + '\\mockups';
  const bt = $('#mk-salvar');
  bt.disabled = true;
  $('#mk-aviso').textContent = '';
  try {
    const arquivos = [];
    let n = 0;
    for (const it of itens) {
      n++;
      $('#mk-progresso').textContent = `Renderizando ${n} de ${itens.length}…`;
      const cv = await renderMockup(it.prod, it.file, it.v.id, 0);
      const b64 = cv.toDataURL('image/png').split(',')[1];
      const nm = `${it.prod.nome} - ${it.cor} - ${nomeVariante(it.v)}.png`.replace(/[<>:"/\\|?*]/g, '');
      arquivos.push({ nome: nm, b64 });
    }
    const salvos = await invoke('salvar_arquivos', { pasta, arquivos });
    $('#mk-progresso').textContent = `${salvos.length} mockup(s) salvos em ${pasta}`;
  } catch (e) {
    $('#mk-aviso').textContent = String(e);
  }
  bt.disabled = false;
});

/* ================= Atualizações dos motores + da Central (§8/§9) =================
   Checagem no máximo 1x/dia, sem erro visível se falhar; toda atualização
   depende de aprovação do Murilo. Estado em <dados>/motores.json. */

const MOTORES = [
  {
    id: 'sidecar',
    nome: 'Ampliação IA — motor ncnn (sidecar)',
    repo: 'upscayl/upscayl-ncnn',
    tipo: 'sidecar',
    baseData: '2022-04-24', // build embutido (pacote Real-ESRGAN v0.2.5.0)
    fork: 'MullerDeFranca/upscayl-ncnn',
    meusArquivos: [],
  },
  {
    id: 'upscaylapp',
    nome: 'App Upscayl — modelos próprios e pipeline',
    repo: 'upscayl/upscayl',
    tipo: 'fork',
    baseData: '2026-03-27', // commit a00d55f, base do fork upscaylae
    fork: 'MullerDeFranca/upscaylae',
    meusArquivos: [
      'electron/muller/*', 'common/muller-*.ts', 'common/models-list.ts',
      'resources/models/arte-chapada-*', 'resources/models/limpa-jpeg-4x.*',
      'GUIA-MULLER.md', 'electron/index.ts', 'electron/main-window.ts',
    ],
  },
  {
    id: 'vtracer',
    nome: 'Vetorização — VTracer (compilado na Central)',
    repo: 'visioncortex/vtracer',
    tipo: 'lib',
    baseData: '2026-11-23', // commit 82fedd1, base do fork
    fork: 'MullerDeFranca/vtracer',
    meusArquivos: [],
  },
];

const upd = { estado: null, releases: {} };

async function updLerEstado() {
  if (upd.estado) return upd.estado;
  upd.estado = { ultimaChecagem: 0, motores: {} };
  if (!TAURI) return upd.estado;
  try {
    const cfg = await invoke('config_obter');
    const b64 = await invoke('ler_arquivo_b64', { pasta: cfg.pastaDados, nome: 'motores.json' });
    if (b64) upd.estado = JSON.parse(new TextDecoder().decode(Uint8Array.from(atob(b64), (c) => c.charCodeAt(0))));
  } catch (e) { /* primeira execução ou arquivo ausente: começa zerado */ }
  if (!upd.estado.motores) upd.estado.motores = {};
  return upd.estado;
}

async function updGravarEstado() {
  if (!TAURI || !upd.estado) return;
  try {
    const cfg = await invoke('config_obter');
    await invoke('escrever_arquivo_texto', {
      pasta: cfg.pastaDados,
      nome: 'motores.json',
      texto: JSON.stringify(upd.estado, null, 2),
    });
  } catch (e) { console.warn('não gravei motores.json', e); }
}

/* Mini-renderizador de markdown para os patch notes (§9.4). */
function mdMini(src) {
  const escH = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  const inline = (s) => escH(s)
    .replace(/\[([^\]]+)\]\((https?:[^)]+)\)/g, '<a href="$2" target="_blank" rel="noopener">$1</a>')
    .replace(/\*\*([^*]+)\*\*/g, '<b>$1</b>')
    .replace(/`([^`]+)`/g, '<code>$1</code>');
  const linhas = String(src || '').split('\n');
  let html = '', emLista = false;
  for (const ln of linhas) {
    const t = ln.trimEnd();
    if (/^#{1,4}\s/.test(t)) {
      if (emLista) { html += '</ul>'; emLista = false; }
      html += `<h4>${inline(t.replace(/^#{1,4}\s*/, ''))}</h4>`;
    } else if (/^[-*]\s+/.test(t)) {
      if (!emLista) { html += '<ul>'; emLista = true; }
      html += `<li>${inline(t.replace(/^[-*]\s+/, ''))}</li>`;
    } else if (t === '') {
      if (emLista) { html += '</ul>'; emLista = false; }
    } else {
      if (emLista) { html += '</ul>'; emLista = false; }
      html += `<p>${inline(t)}</p>`;
    }
  }
  if (emLista) html += '</ul>';
  return html;
}

function updStatusDo(m) {
  const rel = upd.releases[m.id];
  const st = upd.estado.motores[m.id] || {};
  if (!rel) return { situacao: 'desconhecido' };
  const nova = rel.tag_name || rel.name || '';
  const dataRel = (rel.published_at || '').slice(0, 10);
  const temNova = dataRel > m.baseData;
  if (!temNova) return { situacao: 'atualizado', rel };
  if (st.ignorar === nova) return { situacao: 'ignorada', rel };
  return { situacao: 'ha-atualizacao', rel, nova, dataRel };
}

async function checarAtualizacoesMotores(forcar = false) {
  const st = await updLerEstado();
  const umDia = 24 * 60 * 60 * 1000;
  if (!forcar && Date.now() - (st.ultimaChecagem || 0) < umDia) { renderAtualizacoes(); return; }
  st.ultimaChecagem = Date.now();
  await updGravarEstado();
  for (const m of MOTORES) {
    try {
      const r = await fetch(`https://api.github.com/repos/${m.repo}/releases/latest`, {
        headers: { Accept: 'application/vnd.github+json' },
      });
      if (r.ok) upd.releases[m.id] = await r.json();
    } catch (e) { /* sem internet ou limite da API: silêncio, tenta depois (§9.3) */ }
  }
  renderAtualizacoes();
  const novas = MOTORES.filter((m) => updStatusDo(m).situacao === 'ha-atualizacao');
  if (novas.length) {
    $('#upd-toast-txt').textContent = novas.length === 1
      ? `Atualização disponível: ${novas[0].nome}`
      : `${novas.length} motores com atualização disponível`;
    $('#upd-toast').hidden = false;
  }
}

async function updAplicar(m, cardEl) {
  const info = updStatusDo(m);
  const rel = info.rel;
  const tag = rel.tag_name || rel.name || '';
  const st = (upd.estado.motores[m.id] = upd.estado.motores[m.id] || { historico: [] });
  if (m.tipo === 'sidecar') {
    const asset = (rel.assets || []).find((a) => /windows/i.test(a.name) && /\.zip$/i.test(a.name));
    if (!asset) { alert('Esta release não tem pacote para Windows.'); return; }
    cardEl.querySelector('.upd-acoes').innerHTML = '<span class="fraco">Baixando e trocando o motor… (pode levar alguns minutos)</span>';
    try {
      const msg = await invoke('motor_sidecar_atualizar', { url: asset.browser_download_url, tag });
      st.historico.unshift({ tag, data: new Date().toISOString().slice(0, 10), status: 'atualizado' });
      m.baseData = (rel.published_at || '').slice(0, 10);
      await updGravarEstado();
      alert(msg);
    } catch (e) {
      alert('Falha ao atualizar: ' + e);
    }
    renderAtualizacoes();
  } else {
    const passos = m.tipo === 'lib'
      ? `git fetch upstream\ngit switch master && git merge upstream/master\ngit switch Melhorias-Muller && git merge master\n# resolva conflitos se houver, depois atualize o crate na Central (Cargo.toml) e gere nova versão`
      : `git fetch upstream\ngit switch main && git merge upstream/main\ngit switch Melhorias-Muller && git merge main\n# resolva conflitos se houver`;
    const aviso = m.meusArquivos.length
      ? `\n\nATENÇÃO — você tem alterações próprias que podem conflitar (ver ALTERACOES-MULLER.md):\n` + m.meusArquivos.join(', ')
      : '';
    alert(`Passo a passo para atualizar ${m.nome} (fork ${m.fork}):\n\n${passos}${aviso}`);
  }
}

async function renderAtualizacoes() {
  const host = $('#upd-lista');
  if (!host) return;
  await updLerEstado();
  host.innerHTML = '';

  // Cartão da própria Central (updater do Tauri).
  const central = document.createElement('div');
  central.className = 'upd-card';
  central.innerHTML = `
    <div class="upd-topo"><b>Central de Trabalho</b><span class="fraco">este programa</span></div>
    <div class="upd-acoes"><button data-a="verificar-central">Verificar atualização</button></div>
    <div class="upd-status fraco"></div>`;
  central.querySelector('[data-a="verificar-central"]').addEventListener('click', async (ev) => {
    const status = central.querySelector('.upd-status');
    status.textContent = 'Verificando…';
    try {
      const r = await invoke('central_verificar_atualizacao');
      if (!r.disponivel) { status.textContent = 'A Central está na versão mais recente.'; return; }
      status.innerHTML = `Nova versão <b>${esc(r.versao)}</b> disponível.`;
      const bt = document.createElement('button');
      bt.textContent = 'Atualizar';
      bt.className = 'primario';
      bt.onclick = async () => {
        bt.disabled = true;
        status.textContent = 'Baixando e instalando…';
        try { status.textContent = await invoke('central_instalar_atualizacao'); }
        catch (e) { status.textContent = String(e); bt.disabled = false; }
      };
      status.append(' ', bt);
    } catch (e) {
      status.textContent = 'Não consegui verificar agora (sem internet ou updater não configurado).';
    }
  });
  host.appendChild(central);

  // Cartões dos motores.
  for (const m of MOTORES) {
    const st = upd.estado.motores[m.id] || {};
    const info = updStatusDo(m);
    const card = document.createElement('div');
    card.className = 'upd-card upd-' + info.situacao;
    const rotulos = { 'atualizado': 'em dia', 'ha-atualizacao': 'há atualização', 'ignorada': 'versão ignorada', 'desconhecido': 'não verificado' };
    let detalhes = '';
    if (info.rel && info.situacao !== 'atualizado' && info.situacao !== 'desconhecido') {
      const notas = mdMini(info.rel.body || '(sem notas)');
      detalhes = `
        <details class="upd-notas"><summary>Patch notes (${esc(info.nova || '')}, ${esc(info.dataRel || '')})</summary>
        <div class="upd-notas-corpo">${notas}</div>
        <div class="fraco"><a href="https://github.com/${m.repo}/releases/latest" target="_blank" rel="noopener">Ver no GitHub</a></div></details>`;
      if (m.meusArquivos.length) {
        detalhes += `<div class="upd-conflito">Você tem alterações próprias neste projeto — verifique conflitos em: ${m.meusArquivos.map(esc).join(', ')} (ver ALTERACOES-MULLER.md).</div>`;
      }
    }
    const historico = (st.historico || []).slice(0, 3)
      .map((h) => `${h.data}: ${esc(h.tag)} (${h.status})`).join('<br>');
    card.innerHTML = `
      <div class="upd-topo"><b>${esc(m.nome)}</b><span class="selo selo-${info.situacao}">${rotulos[info.situacao]}</span></div>
      <div class="fraco">base: ${esc(m.baseData)} · fork ${esc(m.fork)}</div>
      ${detalhes}
      <div class="upd-acoes"></div>
      ${historico ? `<div class="fraco upd-hist">${historico}</div>` : ''}`;
    const acoes = card.querySelector('.upd-acoes');
    if (info.situacao === 'ha-atualizacao') {
      const bAt = document.createElement('button');
      bAt.textContent = 'Atualizar';
      bAt.className = 'primario';
      bAt.onclick = () => updAplicar(m, card);
      const bIg = document.createElement('button');
      bIg.textContent = 'Ignorar esta versão';
      bIg.onclick = async () => {
        const s2 = (upd.estado.motores[m.id] = upd.estado.motores[m.id] || {});
        s2.ignorar = info.nova;
        s2.historico = s2.historico || [];
        s2.historico.unshift({ tag: info.nova, data: new Date().toISOString().slice(0, 10), status: 'ignorada' });
        await updGravarEstado();
        renderAtualizacoes();
      };
      const bDp = document.createElement('button');
      bDp.textContent = 'Lembrar depois';
      bDp.onclick = () => { $('#upd-toast').hidden = true; };
      acoes.append(bAt, bIg, bDp);
    }
    if (m.tipo === 'sidecar') {
      try {
        const ss = await invoke('motor_sidecar_status');
        if (ss.temAnterior) {
          const bRb = document.createElement('button');
          bRb.textContent = 'Voltar à versão anterior';
          bRb.onclick = async () => {
            try { alert(await invoke('motor_sidecar_rollback')); renderAtualizacoes(); }
            catch (e) { alert(String(e)); }
          };
          acoes.append(bRb);
        }
      } catch (e) { /* sem sidecar instalado */ }
    }
    host.appendChild(card);
  }

  const bRec = document.createElement('button');
  bRec.textContent = 'Verificar agora';
  bRec.className = 'upd-rechecar';
  bRec.onclick = () => checarAtualizacoesMotores(true);
  host.appendChild(bRec);
}

$('#upd-toast-ver').addEventListener('click', () => {
  $('#upd-toast').hidden = true;
  document.querySelector('#abas button[data-aba="biblioteca"]').click();
});
$('#upd-toast-depois').addEventListener('click', () => { $('#upd-toast').hidden = true; });

if (TAURI) {
  renderAtualizacoes();
  checarAtualizacoesMotores(false);
}
