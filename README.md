# Central de Trabalho (Bancada)

Super sistema de produção DTF/sublimação em um único app desktop: **Tauri 2** (núcleo Rust + interface HTML/JS), Windows.

## Abas

| Aba | O que faz | Origem |
|---|---|---|
| **Biblioteca** | Clientes, logos, folhas salvas (busca por texto/OCR e por imagem), dados e backup | bancada-de-imagem + novo |
| **Tratar Imagem** | Remover fundo/cor, trocar cor, monocromia, recorte, **vetorizar (VTracer)**, **upscale IA (Real-ESRGAN)** | bancada-de-imagem + montador-dtf |
| **Montar DTF** | Encaixe retangular/contorno, prévia com ajuste manual, exportar PNG (300 DPI c/ pHYs) e PDF | montador-dtf |
| **Encaixar Moldes** | Moldes de sublimação em PDF: análise de partes, encaixe por pedido, PDF vetorial | encaixe-de-moldes |
| **Mockups** | Catálogo de fotos, logo com variantes e acabamentos realistas, editor de posições com régua em cm | mockup-studio |

## Desenvolvimento

```bash
# Pré-requisitos: Rust (GNU), Node 18+, e o motor de upscale:
scripts/instalar-motores.ps1        # baixa o Real-ESRGAN p/ bin/realesrgan

cd ui && npm install && npm run dev # interface em dev (porta 5173)
cargo build -p app                  # app de desktop (debug)
cargo test                          # testes do núcleo
```

- `nucleo/` — biblioteca Rust com a lógica pesada (encaixe, contorno, moldes, editor, vetorização)
- `app/` — app Tauri (comandos, banco SQLite, diálogos nativos)
- `ui/` — interface (Vite, JS vanilla)
- `repos/forks/` — forks dos motores (upscaylae, upscayl-ncnn, vtracer)

Documentos: [DECISOES.md](DECISOES.md) (decisões técnicas) ·
[MANUTENCAO.md](MANUTENCAO.md) (o que foi mantido/alterado/removido de cada ferramenta original + teste de validação)
