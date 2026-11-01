//! O que vem das configurações do KDE (`kdeglobals`): tema de cores, tema de ícones, fonte e
//! terminal. Relido a cada abertura, como o tema da barra no Windows.

use std::{collections::HashMap, fs, path::PathBuf};

use crate::tema::{self, Tema};

pub struct Kde {
    pub tema: Tema,
    pub icones: String,
    pub fonte: String,
    pub terminal: String,
}

pub fn dir_config() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"))
}

/// Seções e chaves de um arquivo INI do KDE.
pub fn ler_ini(texto: &str) -> HashMap<String, HashMap<String, String>> {
    let mut ini: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut secao = String::new();
    for linha in texto.lines() {
        let l = linha.trim();
        if l.starts_with('[') && l.ends_with(']') {
            secao = l[1..l.len() - 1].to_string();
        } else if let Some((k, v)) = l.split_once('=') {
            ini.entry(secao.clone()).or_default().insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    ini
}

pub fn ler() -> Kde {
    let ini = ler_ini(&fs::read_to_string(dir_config().join("kdeglobals")).unwrap_or_default());
    let valor = |secao: &str, chave: &str| ini.get(secao).and_then(|s| s.get(chave)).cloned();
    let tema = valor("General", "ColorScheme")
        .and_then(|e| tema::por_esquema_kde(&e).copied())
        .unwrap_or_else(|| tema_das_cores(&ini));
    Kde {
        tema,
        icones: valor("Icons", "Theme").unwrap_or_else(|| "breeze".into()),
        fonte: valor("General", "font")
            .and_then(|f| f.split(',').next().map(str::to_string))
            .filter(|f| !f.is_empty())
            .unwrap_or_else(|| "Noto Sans".into()),
        terminal: valor("General", "TerminalApplication").unwrap_or_else(|| "konsole".into()),
    }
}

fn rgb(v: Option<&String>) -> Option<u32> {
    let p: Vec<u32> = v?.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    (p.len() >= 3).then(|| (p[0] << 16) | (p[1] << 8) | p[2])
}

fn mistura(a: u32, b: u32, f: f32) -> u32 {
    let canal = |s: u32| {
        let (x, y) = (((a >> s) & 0xFF) as f32, ((b >> s) & 0xFF) as f32);
        ((x + (y - x) * f).round() as u32) << s
    };
    canal(16) | canal(8) | canal(0)
}

/// Esquema de cores que não é do Omarchy: monta um tema com as cores da visão e da seleção.
fn tema_das_cores(ini: &HashMap<String, HashMap<String, String>>) -> Tema {
    let cor = |secao: &str, chave: &str, padrao: u32| {
        rgb(ini.get(secao).and_then(|s| s.get(chave))).unwrap_or(padrao)
    };
    let fundo = cor("Colors:View", "BackgroundNormal", 0x202020);
    let texto = cor("Colors:View", "ForegroundNormal", 0xe0e0e0);
    let mudo = cor("Colors:View", "ForegroundInactive", mistura(texto, fundo, 0.5));
    let destaque = cor("Colors:Selection", "BackgroundNormal", 0x3daee9);
    let luz = (((fundo >> 16) & 0xFF) * 3 + ((fundo >> 8) & 0xFF) * 6 + (fundo & 0xFF)) / 10;
    Tema {
        nome: "kde",
        claro: luz > 128,
        fundo,
        fundo_claro: mistura(fundo, texto, 0.06),
        selecao: mistura(fundo, texto, 0.12),
        mudo,
        texto,
        texto_escuro: mudo,
        texto_forte: texto,
        destaque,
        vermelho: cor("Colors:View", "ForegroundNegative", 0xda4453),
        amarelo: cor("Colors:View", "ForegroundNeutral", 0xf67400),
        verde: cor("Colors:View", "ForegroundPositive", 0x27ae60),
    }
}
