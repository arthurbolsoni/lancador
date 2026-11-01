//! Lançador no KDE Plasma (Wayland). Uma instância fica residente (serviço do usuário) com a
//! janela fechada; o atalho do KDE roda `lancador --alternar`, que só avisa a instância pelo
//! socket e sai.

mod app;
mod desenho;
mod icones;
mod itens;
mod kde;

use std::{
    io::Write,
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
};

pub fn caminho_socket() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("lancador.sock")
}

/// Pedido para a instância residente, a partir dos argumentos.
/// `--testar <texto> <png>` desenha a janela com a busca `texto` num PNG, sem mostrar nada.
fn pedido() -> Option<String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--alternar") => Some("alternar".into()),
        Some("--sair") => Some("sair".into()),
        Some("--testar") if args.len() >= 3 => Some(format!("testar\n{}\n{}", args[1], args[2])),
        _ => None,
    }
}

pub fn rodar() {
    let pedido = pedido();
    let sock = caminho_socket();
    if let Ok(mut s) = UnixStream::connect(&sock) {
        if let Some(p) = &pedido {
            let _ = s.write_all(p.as_bytes());
        }
        return;
    }
    if matches!(pedido.as_deref(), Some("sair")) {
        return;
    }
    // Socket de uma instância que morreu sem apagar.
    let _ = std::fs::remove_file(&sock);
    let ouvinte = match UnixListener::bind(&sock) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("lancador: não deu para criar {}: {e}", sock.display());
            std::process::exit(1);
        }
    };
    app::rodar(ouvinte, pedido.as_deref() == Some("alternar"));
}
