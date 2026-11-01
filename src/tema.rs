//! Temas da barra (cópia de `barra/src/tema.rs`, mesma ordem), que são os temas do Omarchy.
//! No Windows, a barra grava o índice do tema escolhido em HKCU\Software\barra\Tema e o
//! lançador lê o mesmo valor ao abrir. No KDE, o tema vem do esquema de cores ativo
//! (`OmarchyMatteBlack` = `matte-black`, gravado pelo `omarchy-tema`).

#[allow(dead_code)]
#[derive(Clone, Copy)]
pub struct Tema {
    pub nome: &'static str,
    pub claro: bool,
    pub fundo: u32,
    pub fundo_claro: u32,
    pub selecao: u32,
    pub mudo: u32,
    pub texto: u32,
    pub texto_escuro: u32,
    pub texto_forte: u32,
    pub destaque: u32,
    pub vermelho: u32,
    pub amarelo: u32,
    pub verde: u32,
}

macro_rules! t {
    ($nome:expr, $claro:expr, $fundo:expr, $fundo_claro:expr, $selecao:expr, $mudo:expr, $texto:expr, $texto_escuro:expr, $texto_forte:expr, $destaque:expr, $vermelho:expr, $amarelo:expr, $verde:expr) => {
        Tema {
            nome: $nome,
            claro: $claro,
            fundo: $fundo,
            fundo_claro: $fundo_claro,
            selecao: $selecao,
            mudo: $mudo,
            texto: $texto,
            texto_escuro: $texto_escuro,
            texto_forte: $texto_forte,
            destaque: $destaque,
            vermelho: $vermelho,
            amarelo: $amarelo,
            verde: $verde,
        }
    };
}

pub const TEMAS: &[Tema] = &[
    t!("tokyo-night", false, 0x1a1b26, 0x24283b, 0x292e42, 0x414868, 0xa9b1d6, 0x565f89, 0xc0caf5, 0x7aa2f7, 0xf7768e, 0xe0af68, 0x9ece6a),
    t!("catppuccin", false, 0x1e1e2e, 0x313244, 0x45475a, 0x585b70, 0xcdd6f4, 0x6c7086, 0xcdd6f4, 0x89b4fa, 0xf38ba8, 0xf9e2af, 0xa6e3a1),
    t!("catppuccin-latte", true, 0xeff1f5, 0xdce0e8, 0xccd0da, 0xacb0be, 0x4c4f69, 0x9ca0b0, 0x4c4f69, 0x1e66f5, 0xd20f39, 0xdf8e1d, 0x40a02b),
    t!("ethereal", false, 0x060b1e, 0x131a3a, 0x252e56, 0x6d7db6, 0xffcead, 0x6d7db6, 0xffcead, 0x7d82d9, 0xed5b5a, 0xe9bb4f, 0x92a593),
    t!("everforest", false, 0x2d353b, 0x343f44, 0x3d484d, 0x475258, 0xd3c6aa, 0x4f585e, 0xd3c6aa, 0x7fbbb3, 0xe67e80, 0xdbbc7f, 0xa7c080),
    t!("flexoki-light", true, 0xfffcf0, 0xe6e4d9, 0xcecdc3, 0xb7b5ac, 0x100f0f, 0x878580, 0x100f0f, 0x205ea6, 0xd14d41, 0xd0a215, 0x879a39),
    t!("gruvbox", false, 0x282828, 0x3c3836, 0x504945, 0x665c54, 0xd4be98, 0x7c6f64, 0xd4be98, 0x7daea3, 0xea6962, 0xd8a657, 0xa9b665),
    t!("hackerman", false, 0x0b0c16, 0x151828, 0x1f253a, 0x2d3450, 0xddf7ff, 0x6a6e95, 0xddf7ff, 0x82fb9c, 0x50f872, 0x50f7d4, 0x4fe88f),
    t!("kanagawa", false, 0x1f1f28, 0x223249, 0x363646, 0x54546d, 0xdcd7ba, 0x727169, 0xdcd7ba, 0xdcd7ba, 0xc34043, 0xc0a36e, 0x76946a),
    t!("last-horizon", false, 0x0c0b0c, 0x0c0b0c, 0x584e51, 0x584e51, 0xfafcfb, 0x584e51, 0xe2dddc, 0xb59790, 0xc38b7b, 0x6b5e73, 0x87a9b0),
    t!("lumon", false, 0x16242d, 0x1b2d40, 0x243d56, 0x304860, 0xd6e2ee, 0x4d86b0, 0xf2fcff, 0x8bc9eb, 0x4d86b0, 0x6fa4c9, 0x5e95bc),
    t!("lupine", true, 0xfafafa, 0xf5f5f5, 0xd0d0d0, 0x9e9e9e, 0x212121, 0x757575, 0x000000, 0x3264eb, 0xc900c4, 0x026fde, 0x4a2fd0),
    t!("matte-black", false, 0x121212, 0x1e1e1e, 0x2a2a2a, 0x333333, 0xbebebe, 0x555555, 0xbebebe, 0xe68e0d, 0xd35f5f, 0xb91c1c, 0xffc107),
    t!("miasma", false, 0x222222, 0x2c2c2c, 0x383838, 0x666666, 0xc2c2b0, 0x555555, 0xc2c2b0, 0x78824b, 0x685742, 0xb36d43, 0x5f875f),
    t!("nord", false, 0x2e3440, 0x3b4252, 0x434c5e, 0x4c566a, 0xd8dee9, 0x667080, 0xd8dee9, 0x81a1c1, 0xbf616a, 0xebcb8b, 0xa3be8c),
    t!("osaka-jade", false, 0x111c18, 0x23372b, 0x32473b, 0x53685b, 0xc1c497, 0x81b8a8, 0xf7e8b2, 0x509475, 0xff5345, 0x459451, 0x549e6a),
    t!("retro-82", false, 0x05182e, 0x0a2540, 0x134e5a, 0x2a6b78, 0xf6dcac, 0x3f8f8a, 0xf6dcac, 0xfaa968, 0xf85525, 0xe97b3c, 0x028391),
    t!("ristretto", false, 0x2c2525, 0x3d2f2a, 0x403e41, 0x72696a, 0xe6d9db, 0x72696a, 0xe6d9db, 0xf38d70, 0xfd6883, 0xf9cc6c, 0xadda78),
    t!("rose-pine", true, 0xfaf4ed, 0xf2e9e1, 0xdfdad9, 0xcecacd, 0x575279, 0x9893a5, 0x575279, 0x56949f, 0xb4637a, 0xea9d34, 0x286983),
    t!("solitude", false, 0x101315, 0x101315, 0x343d41, 0x4b4e55, 0xcacccc, 0x4b4e55, 0xa5aeb4, 0x798186, 0x565d60, 0xd9dbdc, 0x9fa5a9),
    t!("starship", false, 0x1c1c1e, 0x2c2c2e, 0x3a3a3c, 0x8e8e93, 0xf5f5f7, 0x98989d, 0xffffff, 0xff9f0a, 0xff5f4a, 0xffc27a, 0xff9f0a),
    t!("vantablack", false, 0x000000, 0x1a1a1a, 0x1a1a1a, 0x7a7a7a, 0xffffff, 0x505050, 0xffffff, 0x8d8d8d, 0xa4a4a4, 0xcecece, 0xb6b6b6),
    t!("white", true, 0xffffff, 0xc0c0c0, 0xc0c0c0, 0x808080, 0x000000, 0xc0c0c0, 0x000000, 0x6e6e6e, 0x2a2a2a, 0x4a4a4a, 0x3a3a3a),
];

#[cfg(windows)]
pub fn por_indice(i: u32) -> &'static Tema {
    TEMAS.get(i as usize).unwrap_or(&TEMAS[0])
}

/// Tema pelo nome do esquema de cores do KDE: "Omarchy" + nome em CamelCase.
#[cfg(target_os = "linux")]
pub fn por_esquema_kde(esquema: &str) -> Option<&'static Tema> {
    let nome = esquema.strip_prefix("Omarchy")?;
    TEMAS.iter().find(|t| {
        let camel: String = t
            .nome
            .split('-')
            .map(|p| {
                let mut c = p.chars();
                c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
            })
            .collect();
        camel == nome
    })
}

/// Tema escolhido no menu da barra; sem a chave, o primeiro (o padrão da barra).
#[cfg(windows)]
pub fn escolhido() -> &'static Tema {
    use std::ffi::c_void;
    use windows::{
        core::w,
        Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD},
    };
    let mut v = 0u32;
    let mut tam = 4u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\barra"),
            w!("Tema"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut _ as *mut c_void),
            Some(&mut tam),
        )
        .is_ok()
    };
    por_indice(if ok { v } else { 0 })
}
