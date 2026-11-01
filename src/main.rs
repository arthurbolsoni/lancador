#![cfg_attr(windows, windows_subsystem = "windows")]

mod busca;
mod tema;

#[cfg(windows)]
mod itens;
#[cfg(windows)]
mod janela;
#[cfg(target_os = "linux")]
mod linux;

fn main() {
    #[cfg(windows)]
    janela::rodar();
    #[cfg(target_os = "linux")]
    linux::rodar();
}
