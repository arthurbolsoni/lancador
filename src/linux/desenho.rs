//! Desenho em software num `Pixmap` do tiny-skia (RGBA pré-multiplicado): retângulos
//! arredondados, texto rasterizado pelo ab_glyph e ícones.

use ab_glyph::{point, Font, FontVec, GlyphId, PxScale, ScaleFont};
use resvg::tiny_skia::{Color, FillRule, Paint, Path, PathBuilder, Pixmap, PixmapPaint, Rect, Transform};

/// Cor RGB + alfa (0..1), sem pré-multiplicar.
#[derive(Clone, Copy)]
pub struct Cor {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Cor {
    pub fn hex(h: u32, a: f32) -> Cor {
        Cor {
            r: ((h >> 16) & 0xFF) as f32 / 255.0,
            g: ((h >> 8) & 0xFF) as f32 / 255.0,
            b: (h & 0xFF) as f32 / 255.0,
            a,
        }
    }

    pub fn com_alfa(self, a: f32) -> Cor {
        Cor { a: self.a * a, ..self }
    }

    fn paint(self) -> Paint<'static> {
        let mut p = Paint::default();
        p.set_color(Color::from_rgba(self.r, self.g, self.b, self.a.clamp(0.0, 1.0)).unwrap_or(Color::BLACK));
        p.anti_alias = true;
        p
    }
}

pub fn retangulo(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, cor: Cor, clip: Option<(f32, f32)>) {
    let (y, h) = recortar(y, h, clip);
    if let Some(r) = Rect::from_xywh(x, y, w, h) {
        pm.fill_rect(r, &cor.paint(), Transform::identity(), None);
    }
}

/// Faixa vertical visível de um retângulo dentro de `clip` (topo, base).
fn recortar(y: f32, h: f32, clip: Option<(f32, f32)>) -> (f32, f32) {
    match clip {
        Some((topo, base)) => {
            let y0 = y.max(topo);
            let y1 = (y + h).min(base);
            (y0, (y1 - y0).max(0.0))
        }
        None => (y, h),
    }
}

pub fn caminho_arredondado(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let k = 0.552_284_8 * r;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

pub fn arredondado(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, cor: Cor) {
    if let Some(p) = caminho_arredondado(x, y, w, h, r) {
        pm.fill_path(&p, &cor.paint(), FillRule::Winding, Transform::identity(), None);
    }
}

/// Contorno de 1 px por dentro de um retângulo arredondado.
pub fn contorno(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, cor: Cor) {
    if let Some(p) = caminho_arredondado(x + 0.5, y + 0.5, w - 1.0, h - 1.0, r) {
        let stroke = resvg::tiny_skia::Stroke { width: 1.0, ..Default::default() };
        pm.stroke_path(&p, &cor.paint(), &stroke, Transform::identity(), None);
    }
}

/// Ícone (já no tamanho) com transparência global `alfa`, recortado na faixa `clip`.
pub fn icone(pm: &mut Pixmap, img: &Pixmap, x: i32, y: i32, alfa: f32, clip: (f32, f32)) {
    let (topo, base) = (clip.0 as i32, clip.1 as i32);
    if y >= base || y + img.height() as i32 <= topo {
        return;
    }
    let paint = PixmapPaint { opacity: alfa, ..Default::default() };
    if y >= topo && y + img.height() as i32 <= base {
        pm.draw_pixmap(x, y, img.as_ref(), &paint, Transform::identity(), None);
        return;
    }
    // Parcialmente fora: desenha só as linhas visíveis.
    let y0 = topo.max(y);
    let y1 = base.min(y + img.height() as i32);
    let w = img.width();
    let Some(mut parte) = Pixmap::new(w, (y1 - y0) as u32) else { return };
    let ini = ((y0 - y) as u32 * w * 4) as usize;
    let fim = ini + (parte.height() * w * 4) as usize;
    parte.data_mut().copy_from_slice(&img.data()[ini..fim]);
    pm.draw_pixmap(x, y0, parte.as_ref(), &paint, Transform::identity(), None);
}

pub struct Fonte {
    f: FontVec,
}

impl Fonte {
    /// Fonte pelo padrão do fontconfig (`"Noto Sans:light"`). O ab_glyph lê cada glifo do
    /// arquivo só quando ele é desenhado (o fontdue processava a fonte inteira ao carregar).
    pub fn carregar(padrao: &str) -> Option<Fonte> {
        let saida = std::process::Command::new("fc-match").args(["-f", "%{file}", padrao]).output().ok()?;
        let arquivo = String::from_utf8(saida.stdout).ok()?;
        let dados = std::fs::read(arquivo.trim()).ok()?;
        Some(Fonte { f: FontVec::try_from_vec(dados).ok()? })
    }

    /// Escala do ab_glyph (relativa à altura da linha) para um tamanho de em em pixels,
    /// que é o que o DirectWrite usa no Windows.
    fn escala(&self, px: f32) -> PxScale {
        let em = self.f.units_per_em().unwrap_or(1000.0);
        PxScale::from(px * self.f.height_unscaled() / em)
    }

    /// Avanço acumulado antes de cada caractere, mais o total no fim.
    pub fn posicoes(&self, texto: &[char], px: f32) -> Vec<f32> {
        let sf = self.f.as_scaled(self.escala(px));
        let mut v = Vec::with_capacity(texto.len() + 1);
        let mut x = 0.0;
        let mut ant: Option<GlyphId> = None;
        for &c in texto {
            let id = sf.glyph_id(c);
            if let Some(a) = ant {
                x += sf.kern(a, id);
            }
            v.push(x);
            x += sf.h_advance(id);
            ant = Some(id);
        }
        v.push(x);
        v
    }

    pub fn largura(&self, texto: &[char], px: f32) -> f32 {
        *self.posicoes(texto, px).last().unwrap_or(&0.0)
    }

    /// Altura da linha (ascendente - descendente).
    pub fn altura_linha(&self, px: f32) -> f32 {
        let sf = self.f.as_scaled(self.escala(px));
        sf.ascent() - sf.descent()
    }

    /// Texto centrado na vertical em [y, y + h], a partir de x, cortado em `max_x` com "…".
    #[allow(clippy::too_many_arguments)]
    pub fn desenhar(&self, pm: &mut Pixmap, texto: &[char], x: f32, y: f32, h: f32, px: f32, max_x: f32, cor: Cor, clip: Option<(f32, f32)>) {
        let escala = self.escala(px);
        let sf = self.f.as_scaled(escala);
        let (asc, desc) = (sf.ascent(), sf.descent());
        let base = (y + (h - (asc - desc)) / 2.0 + asc).round();
        let mut texto: Vec<char> = texto.to_vec();
        if x + self.largura(&texto, px) > max_x {
            let reticencias = sf.h_advance(sf.glyph_id('…'));
            while !texto.is_empty() && x + self.largura(&texto, px) + reticencias > max_x {
                texto.pop();
            }
            while texto.last().is_some_and(|c| c.is_whitespace()) {
                texto.pop();
            }
            texto.push('…');
        }
        let pos = self.posicoes(&texto, px);
        let (topo, fundo) = clip.unwrap_or((0.0, pm.height() as f32));
        let (pw, ph) = (pm.width() as i32, pm.height() as i32);
        let dados = pm.data_mut();
        for (i, &c) in texto.iter().enumerate() {
            let g = self.f.glyph_id(c).with_scale_and_position(escala, point(x + pos[i], base));
            let Some(contorno) = self.f.outline_glyph(g) else { continue };
            let b = contorno.px_bounds();
            contorno.draw(|gx, gy, cob| {
                let px_ = b.min.x as i32 + gx as i32;
                let py = b.min.y as i32 + gy as i32;
                if cob <= 0.0 || px_ < 0 || px_ >= pw || py < 0 || py >= ph || (py as f32) < topo || (py as f32) >= fundo {
                    return;
                }
                let a = cob.min(1.0) * cor.a;
                let i = ((py * pw + px_) * 4) as usize;
                let d = &mut dados[i..i + 4];
                let mix = |src: f32, dst: u8| ((src * a + dst as f32 / 255.0 * (1.0 - a)) * 255.0).round().clamp(0.0, 255.0) as u8;
                d[0] = mix(cor.r, d[0]);
                d[1] = mix(cor.g, d[1]);
                d[2] = mix(cor.b, d[2]);
                d[3] = ((a + d[3] as f32 / 255.0 * (1.0 - a)) * 255.0).round().clamp(0.0, 255.0) as u8;
            });
        }
    }
}
