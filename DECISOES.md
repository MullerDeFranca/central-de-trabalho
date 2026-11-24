# Decisões técnicas — Central de Trabalho

Registro das decisões relevantes para manutenção futura (inclusive por outra IA). §11 da especificação.

## Stack

- **Tauri 2 + Rust (toolchain GNU)** em Windows. O alvo de build fica fora do workspace
  (`C:\tools\bancada-target`, via `CARGO_TARGET_DIR`) porque o espaço em `Muller PC`
  quebra `ld`/`windres` do MinGW.
- **Interface em JS vanilla + Vite** (não React): os artefatos originais eram HTML/JS
  puros e a especificação manda reaproveitar telas em JS (§1.1).
- **Rust só onde ganha desempenho**: encaixe (retangular/contorno), análise de moldes,
  operações de pixel do editor, vetorização, I/O de arquivos. Interface, drag-and-drop
  e prévias ficam em JS (§1.1).

## Motores (§9)

| Motor | Como entra | Fork | Base registrada |
|---|---|---|---|
| Ampliação Real-ESRGAN/ncnn | **Sidecar** em `bin/realesrgan` (trocável sem recompilar) | `MullerDeFranca/upscayl-ncnn` | `0beb390` (2026-04-11) |
| App/modelos Upscayl | Referência de modelos e pipeline | `MullerDeFranca/upscaylae` | `a00d55f` (2026-03-27) |
| Vetorização VTracer | **Biblioteca compilada** no núcleo (crate `vtracer 0.6`) | `MullerDeFranca/vtracer` | `82fedd1` (2026-11-23) |

Cada fork: `main`/`master` = espelho do upstream; alterações próprias na branch
**`Melhorias-Muller`**; cada fork tem `ALTERACOES-MULLER.md` catalogando alterações.

- Upscale IA chama `realesrgan-ncnn-vulkan.exe` como subprocesso (`-n realesrgan-x4plus -s 4`),
  com `CREATE_NO_WINDOW`. Vulkan na GPU AMD; fallback de CPU é do próprio motor.
- Vetorização: `vtracer::convert` com cores chapadas + `resvg` para rasterizar o SVG
  de volta em alta resolução (máx. 4096 px no maior lado).

## Dados (§3)

- Pasta de dados **configurável**, padrão `Documentos\CentralDTF`. A âncora fixa é
  `config.json` na pasta de dados do app (`%APPDATA%\com.bancada.app`), que só guarda
  o caminho configurado.
- **SQLite** (`bancada.sqlite`): clientes, logos, folhas salvas (dados + textos OCR + hash dHash).
- Arquivos soltos (folhas, configs de mockups) ficam em JSON ao lado dos dados ou do catálogo.
- Backup completo = cópia da pasta de dados (exportar) / restauração com salvaguarda (importar).

## Qualidade de impressão (§4)

- PNG de exportação grava **chunk pHYs** com o DPI configurado (`png::Encoder::set_pixel_dims`);
  a importação **lê** o pHYs para preservar o tamanho em cm.
- PDFs dos moldes são **vetoriais** (pdf-lib embutindo a página original com clip),
  não rasterizados.
- Medidas em cm/mm convertidas para px só na borda (impressão), nunca no modelo interno.

## Bibliotecas de interface

- **pdf.js-dist 6.x** (leitura de PDF) — API v6: `getDocument({data, isEvalSupported:false})`.
- **pdf-lib** (escrita de PDF vetorial).
- **tesseract.js 7** (OCR offline) — v7 exige `worker.recognize(img, {}, {text:true, blocks:true})`
  e percorrer `blocks[].paragraphs[].lines[].words[]`; assets locais em `ui/public/`.
- **dHash 64 bits** para busca de folhas por imagem.

## Fora de escopo (combinado com o Murilo)

Multi-logo e editor de texto nos mockups, modo lote, sync celular/cloud.
