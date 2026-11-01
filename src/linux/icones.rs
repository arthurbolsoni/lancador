//! Ícones pelo tema do KDE (especificação freedesktop): o nome é procurado no tema escolhido,
//! nos que ele herda e no hicolor, na pasta de tamanho mais próximo; depois em `pixmaps`.
//! SVG é desenhado pelo resvg e PNG pelo crate png, já no tamanho da linha.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use resvg::{
    tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform},
    usvg,
};

use super::kde;

enum Tipo {
    Fixo,
    Escalavel,
    Limiar,
}

struct Pasta {
    rel: String,
    tam: u32,
    tipo: Tipo,
    min: u32,
    max: u32,
    limiar: u32,
}

impl Pasta {
    /// Distância ao tamanho pedido; pasta menor que o pedido pesa o dobro (ícone ampliado borra).
    fn distancia(&self, tam: u32) -> u32 {
        let (lo, hi) = match self.tipo {
            Tipo::Fixo => (self.tam, self.tam),
            Tipo::Escalavel => (self.min, self.max),
            Tipo::Limiar => (self.tam.saturating_sub(self.limiar), self.tam + self.limiar),
        };
        if tam < lo {
            lo - tam
        } else if tam > hi {
            (tam - hi) * 2
        } else {
            0
        }
    }
}

struct TemaIcones {
    /// `<base>/<tema>` que existem (o mesmo tema pode estar em ~/.local/share/icons e /usr/share/icons).
    raizes: Vec<PathBuf>,
    pastas: Vec<Pasta>,
}

pub struct Icones {
    tema: String,
    cadeia: Vec<TemaIcones>,
    achados: HashMap<(String, u32), Option<PathBuf>>,
}

/// $XDG_DATA_HOME e as pastas de $XDG_DATA_DIRS, nessa ordem de prioridade.
pub fn dirs_dados() -> Vec<PathBuf> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let mut v = vec![std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share"))];
    let dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
    let dirs = if dirs.is_empty() { "/usr/local/share:/usr/share".to_string() } else { dirs };
    v.extend(dirs.split(':').filter(|d| !d.is_empty()).map(PathBuf::from));
    v.dedup();
    v
}

fn bases() -> Vec<PathBuf> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let mut v: Vec<PathBuf> = vec![home.join(".icons")];
    v.extend(dirs_dados().into_iter().map(|d| d.join("icons")));
    v
}

/// Lê o `index.theme` do tema; devolve o tema e os que ele herda.
fn carregar(nome: &str) -> Option<(TemaIcones, Vec<String>)> {
    let raizes: Vec<PathBuf> = bases().into_iter().map(|b| b.join(nome)).filter(|r| r.is_dir()).collect();
    let indice = raizes.iter().map(|r| r.join("index.theme")).find(|f| f.is_file())?;
    let ini = kde::ler_ini(&fs::read_to_string(indice).ok()?);
    let geral = ini.get("Icon Theme")?;
    let lista = |k: &str| -> Vec<String> {
        geral.get(k).map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default()
    };
    let num = |s: &HashMap<String, String>, k: &str, padrao: u32| s.get(k).and_then(|v| v.parse().ok()).unwrap_or(padrao);
    let pastas = lista("Directories")
        .into_iter()
        .filter_map(|rel| {
            let s = ini.get(&rel)?;
            // Pastas @2x servem a telas com escala 2; aqui a escala é tratada no tamanho em pixels.
            if num(s, "Scale", 1) != 1 {
                return None;
            }
            let tam = num(s, "Size", 0);
            let tipo = match s.get("Type").map(String::as_str) {
                Some("Fixed") => Tipo::Fixo,
                Some("Scalable") => Tipo::Escalavel,
                _ => Tipo::Limiar,
            };
            Some(Pasta { tam, tipo, min: num(s, "MinSize", tam), max: num(s, "MaxSize", tam), limiar: num(s, "Threshold", 2), rel })
        })
        .collect();
    Some((TemaIcones { raizes, pastas }, lista("Inherits")))
}

impl TemaIcones {
    fn achar(&self, nome: &str, tam: u32) -> Option<PathBuf> {
        let mut ordem: Vec<&Pasta> = self.pastas.iter().collect();
        ordem.sort_by_key(|p| p.distancia(tam));
        for p in ordem {
            for raiz in &self.raizes {
                for ext in ["svg", "png"] {
                    let f = raiz.join(&p.rel).join(format!("{nome}.{ext}"));
                    if f.is_file() {
                        return Some(f);
                    }
                }
            }
        }
        None
    }
}

impl Icones {
    pub fn new(tema: &str) -> Self {
        let mut cadeia = Vec::new();
        let mut vistos = Vec::new();
        let mut fila = vec![tema.to_string()];
        while let Some(t) = (!fila.is_empty()).then(|| fila.remove(0)) {
            if vistos.contains(&t) {
                continue;
            }
            vistos.push(t.clone());
            if let Some((ti, herda)) = carregar(&t) {
                cadeia.push(ti);
                fila.extend(herda);
            }
            if fila.is_empty() && !vistos.iter().any(|v| v == "hicolor") {
                fila.push("hicolor".into());
            }
        }
        Icones { tema: tema.to_string(), cadeia, achados: HashMap::new() }
    }

    pub fn tema(&self) -> &str {
        &self.tema
    }

    /// Caminho do ícone `nome` (nome no tema ou caminho absoluto) para `tam` pixels.
    pub fn achar(&mut self, nome: &str, tam: u32) -> Option<PathBuf> {
        if nome.starts_with('/') {
            return Path::new(nome).is_file().then(|| PathBuf::from(nome));
        }
        let chave = (nome.to_string(), tam);
        if let Some(c) = self.achados.get(&chave) {
            return c.clone();
        }
        let achado = self.cadeia.iter().find_map(|t| t.achar(nome, tam)).or_else(|| {
            dirs_dados().iter().flat_map(|d| ["svg", "png"].map(|e| d.join("pixmaps").join(format!("{nome}.{e}")))).find(|f| f.is_file())
        });
        self.achados.insert(chave, achado.clone());
        achado
    }
}

/// Desenha o ícone do arquivo num quadrado de `tam` pixels.
pub fn desenhar(caminho: &Path, tam: u32) -> Option<Pixmap> {
    let dados = fs::read(caminho).ok()?;
    let mut pm = Pixmap::new(tam, tam)?;
    let svg = caminho.extension().is_some_and(|e| e == "svg" || e == "svgz");
    if svg {
        let arvore = usvg::Tree::from_data(&dados, &usvg::Options::default()).ok()?;
        let s = arvore.size();
        let k = tam as f32 / s.width().max(s.height());
        let (dx, dy) = ((tam as f32 - s.width() * k) / 2.0, (tam as f32 - s.height() * k) / 2.0);
        resvg::render(&arvore, Transform::from_row(k, 0.0, 0.0, k, dx, dy), &mut pm.as_mut());
    } else {
        let orig = ler_png(&dados)?;
        let k = tam as f32 / orig.width().max(orig.height()) as f32;
        let (dx, dy) = ((tam as f32 - orig.width() as f32 * k) / 2.0, (tam as f32 - orig.height() as f32 * k) / 2.0);
        pm.draw_pixmap(
            0,
            0,
            orig.as_ref(),
            &PixmapPaint { quality: FilterQuality::Bicubic, ..Default::default() },
            Transform::from_row(k, 0.0, 0.0, k, dx, dy),
            None,
        );
    }
    Some(pm)
}

/// PNG em RGBA pré-multiplicado.
fn ler_png(dados: &[u8]) -> Option<Pixmap> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(dados));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut leitor = dec.read_info().ok()?;
    let mut buf = vec![0; leitor.output_buffer_size()?];
    let info = leitor.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width, info.height);
    let px = &buf[..info.buffer_size()];
    let rgba: Vec<[u8; 4]> = match info.color_type {
        png::ColorType::Rgba => px.chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]).collect(),
        png::ColorType::Rgb => px.chunks_exact(3).map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => px.chunks_exact(2).map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Grayscale => px.iter().map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    let mut pm = Pixmap::new(w, h)?;
    for (d, s) in pm.data_mut().chunks_exact_mut(4).zip(rgba) {
        let a = s[3] as u16;
        d.copy_from_slice(&[
            ((s[0] as u16 * a + 127) / 255) as u8,
            ((s[1] as u16 * a + 127) / 255) as u8,
            ((s[2] as u16 * a + 127) / 255) as u8,
            s[3],
        ]);
    }
    Some(pm)
}
