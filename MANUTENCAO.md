# Manutenção — o que veio de cada ferramenta original (§10)

Lista de controle contra regressão: o que foi **mantido**, **alterado** ou
**removido** ao unificar os artefatos na Central. Atualizar a cada mudança de escopo.

## montador-dtf → aba Montar DTF + parte da Tratar Imagem

- **Mantido:** encaixe retangular e por contorno (mesmas decisões do núcleo, portado
  fielmente para Rust), repetição máxima, prévia com ajuste manual, exportar PNG
  com DPI embutido (pHYs) e PDF, biblioteca de folhas com backup JSON, OCR offline
  (tesseract.js), busca de folhas por texto e por imagem (dHash), autosave.
- **Alterado:** interface virou aba dentro da Central; destino de exportação usa
  **janela nativa do Windows** (o `prompt()` antigo não funcionava no app de desktop);
  upscale **2×/4× suave foi substituído pelo upscale IA (Real-ESRGAN 4×)** —
  decisão do Murilo em 2026-11-23 ("Só Real-ESRGAN").
- **Substituído:** vetorização via ImageTracer (JS por CDN) → **VTracer nativo em Rust**,
  offline, com slider de detalhe de cor e download do SVG.
- **Removido (consciente):** dependência de internet para vetorizar.

## bancada-de-imagem → aba Tratar Imagem

- **Mantido:** remover fundo (flood fill pela borda), remover/trocar cor com conta-gotas,
  monocromia, limiar de alpha, recorte do conteúdo, desfazer (12 níveis).
- **Alterado:** operações de pixel rodam no núcleo Rust; upscale é IA (ver acima).
- **Adicionado:** vetorizar (VTracer) e upscale IA 4×; baixar SVG.

## encaixe-de-moldes → aba Encaixar Moldes

- **Mantido:** leitura de PDF (pdf.js), detecção de partes (componentes conectados,
  grupos split tiny/near), clip por linha, encaixe por pedido, prévia.
- **Alterado:** exportação gera **PDF vetorial** (pdf-lib, página original embutida
  com clip) salvo direto em pasta via janela nativa — sem ZIP.

## mockup-studio → aba Mockups

- **Mantido:** catálogo por pasta (agrupamento por nome/cor), remoção de fundo da logo,
  paleta e variantes de cor, acabamentos (silk/DTF, bordado, laser, relevo, dourado,
  prateado), editor de posições com arrastar/redimensionar e **régua em cm**,
  posições salvas em JSON na pasta do catálogo, grade com miniaturas sob demanda.
- **Removido (simplificação combinada com o Murilo):** multi-logo, editor de texto
  com fontes, modo lote, sync celular/cloud. Uma logo por vez.

## Vetorizae (Vector Magic empacotado)

- **Não incorporado:** software proprietário de terceiros. A função é coberta pelo
  **VTracer** nativo (aba Tratar Imagem).

## estudio-de-canais e briefing-identidade-visual

- **Não incorporados** nesta fase (fora do escopo combinado). Candidatos a futuras abas.

## Teste de validação principal (pendente, manual)

Exportar **a mesma folha DTF** pelo artefato antigo e pela Central e abrir as duas
no CorelDRAW: tamanho em cm, DPI, transparência e cores precisam ser idênticos.

## Teste pós-atualização de motor (§9.6)

Imagens de referência em `testes/` (a criar): uma ampliação 4× e uma vetorização.
Comparar com a execução anterior: tamanho, DPI, transparência, cores e qualidade.
