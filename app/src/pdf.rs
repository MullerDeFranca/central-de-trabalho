//! Gerador mínimo de PDF: uma página por folha, artes raster como XObjects
//! (RGB + máscara de alpha), posicionadas com matriz de rotação.
//! Medidas em pontos (1/72 pol), origem no canto inferior-esquerdo.

/// Uma imagem já rasterizada para o PDF (na resolução do filme).
pub struct PdfImagem {
    pub nome: String, // /Im0, /Im1…
    pub w: u32,
    pub h: u32,
    pub rgb: Vec<u8>,
    pub alpha: Vec<u8>,
}

/// Um desenho de imagem na página: centro (pt), tamanho (pt), ângulo (graus,
/// anti-horário, como no PDF) e espelhamento horizontal.
pub struct PdfDraw {
    pub img: usize,
    pub cx: f64,
    pub cy: f64,
    pub w: f64,
    pub h: f64,
    pub angle: f64,
    pub mirror: bool,
}

pub struct PdfPagina {
    pub w_pt: f64,
    pub h_pt: f64,
    pub draws: Vec<PdfDraw>,
}

fn escape_nome(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect()
}

fn stream_obj(header: String, dados: &[u8]) -> Vec<u8> {
    let mut v = header.into_bytes();
    v.extend_from_slice(dados);
    v.extend_from_slice(b"\nendstream");
    v
}

pub fn gerar(paginas: &[PdfPagina], imagens: &[PdfImagem]) -> Vec<u8> {
    let mut objetos: Vec<Vec<u8>> = Vec::new();

    // Imagens (RGB + SMask de alpha), uma vez cada.
    let mut img_ids: Vec<usize> = Vec::new();
    for im in imagens {
        let alpha_zip = miniz_oxide::deflate::compress_to_vec_zlib(&im.alpha, 6);
        let smask_body = stream_obj(
            format!(
                "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n",
                im.w, im.h, alpha_zip.len()
            ),
            &alpha_zip,
        );
        objetos.push(smask_body);
        let smask = objetos.len();

        let rgb_zip = miniz_oxide::deflate::compress_to_vec_zlib(&im.rgb, 6);
        objetos.push(stream_obj(
            format!(
                "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /SMask {} 0 R /Length {} >>\nstream\n",
                im.w, im.h, smask, rgb_zip.len()
            ),
            &rgb_zip,
        ));
        img_ids.push(objetos.len());
    }

    // Páginas e conteúdos (o /Parent é preenchido depois).
    let mut page_ids: Vec<usize> = Vec::new();
    for pg in paginas {
        let mut content = String::new();
        let mut usados: Vec<usize> = Vec::new();
        for d in &pg.draws {
            let rad = d.angle.to_radians();
            let (sin, cos) = rad.sin_cos();
            let (sx, sy) = if d.mirror { (-d.w, d.h) } else { (d.w, d.h) };
            // M = T(cx,cy) · R(θ) · S(±w,h) · T(-0.5,-0.5)
            let a = sx * cos;
            let b = sx * sin;
            let c = -sy * sin;
            let dd = sy * cos;
            let e = d.cx - 0.5 * (a + c);
            let f = d.cy - 0.5 * (b + dd);
            content.push_str(&format!(
                "q {:.4} {:.4} {:.4} {:.4} {:.4} {:.4} cm /{} Do Q\n",
                a, b, c, dd, e, f,
                escape_nome(&imagens[d.img].nome)
            ));
            if !usados.contains(&d.img) {
                usados.push(d.img);
            }
        }
        objetos.push(content.into_bytes());
        let cid = objetos.len();
        let xobjs: String = usados
            .iter()
            .map(|&i| format!("/{} {} 0 R", escape_nome(&imagens[i].nome), img_ids[i]))
            .collect::<Vec<_>>()
            .join(" ");
        objetos.push(
            format!(
                "<< /Type /Page /MediaBox [0 0 {:.2} {:.2}] /Resources << /XObject << {} >> >> /Contents {} 0 R /Parent PAGES 0 R >>",
                pg.w_pt, pg.h_pt, xobjs, cid
            )
            .into_bytes(),
        );
        page_ids.push(objetos.len());
    }

    let kids: String = page_ids.iter().map(|id| format!("{id} 0 R")).collect::<Vec<_>>().join(" ");
    objetos.push(
        format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids, page_ids.len()).into_bytes(),
    );
    let pages_id = objetos.len();
    objetos.push(format!("<< /Type /Catalog /Pages {pages_id} 0 R >>").into_bytes());
    let catalog_id = objetos.len();

    // Resolve o /Parent das páginas.
    for &pid in &page_ids {
        let s = String::from_utf8(objetos[pid - 1].clone()).unwrap();
        objetos[pid - 1] = s.replace("PAGES 0 R", &format!("{pages_id} 0 R")).into_bytes();
    }

    // Monta o arquivo com xref.
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, obj) in objetos.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(obj);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objetos.len() + 1).as_bytes(),
    );
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root {} 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objetos.len() + 1,
            catalog_id
        )
        .as_bytes(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdf_estrutura_valida() {
        let img = PdfImagem {
            nome: "Im0".into(),
            w: 2,
            h: 2,
            rgb: vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0],
            alpha: vec![255, 255, 255, 128],
        };
        let pagina = PdfPagina {
            w_pt: 200.0,
            h_pt: 100.0,
            draws: vec![PdfDraw { img: 0, cx: 50.0, cy: 50.0, w: 40.0, h: 40.0, angle: 30.0, mirror: false }],
        };
        let bytes = gerar(&[pagina], &[img]);
        assert!(bytes.starts_with(b"%PDF-1.4"));
        assert!(bytes.ends_with(b"%%EOF\n"));
        let localizar = |padrao: &[u8]| -> usize {
            bytes.windows(padrao.len()).position(|w| w == padrao).unwrap()
        };
        localizar(b"/Type /Catalog");
        localizar(b"/SMask");
        localizar(b"/MediaBox [0 0 200.00 100.00]");
        // O startxref aponta para o xref real.
        let xref_at = localizar(b"\nxref\n") + 1;
        let sx = localizar(b"startxref\n") + 10;
        let fim = bytes[sx..].iter().position(|&b| b == b'\n').unwrap();
        let declarado: usize = String::from_utf8(bytes[sx..sx + fim].to_vec()).unwrap().parse().unwrap();
        assert_eq!(declarado, xref_at);
        // Todos os offsets do xref apontam para "N 0 obj".
        let secao = &bytes[xref_at..];
        let texto = String::from_utf8_lossy(secao);
        let linhas: Vec<&str> = texto.lines().collect();
        let n: usize = linhas[1].split_whitespace().nth(1).unwrap().parse().unwrap();
        for i in 1..n {
            let off: usize = linhas[2 + i].split_whitespace().next().unwrap().parse().unwrap();
            let trecho = &bytes[off..off + 12];
            let esperado = format!("{i} 0 obj");
            assert!(trecho.starts_with(esperado.as_bytes()), "objeto {i} fora do lugar");
        }
    }
}
