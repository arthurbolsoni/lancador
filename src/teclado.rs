//! Gancho de teclado: Win apertado e solto sozinho abre o lançador no lugar do Iniciar.
//!
//! Ao soltar o Win sem outra tecla no meio, o gancho segura o Win solto e manda 0xE8 (tecla
//! sem uso) + Win solto simulados: o Iniciar vê uma combinação e não abre. Win+qualquer tecla
//! passa intacto. Janela de programa rodando como administrador em foco não passa pelo gancho,
//! e aí o Iniciar abre.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Para onde postar `MENSAGEM`.
static DESTINO: AtomicIsize = AtomicIsize::new(0);
static MENSAGEM: AtomicIsize = AtomicIsize::new(0);
/// Win apertado e nenhuma outra tecla desde então.
static SOZINHO: AtomicBool = AtomicBool::new(false);
/// Só para teste (LANCADOR_TESTE=1): aceita teclas simuladas por SendInput.
static ACEITA_SIMULADA: AtomicBool = AtomicBool::new(false);

const MASCARA: VIRTUAL_KEY = VIRTUAL_KEY(0xE8);

unsafe extern "system" fn gancho(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let k = &*(lp.0 as *const KBDLLHOOKSTRUCT);
        let vk = VIRTUAL_KEY(k.vkCode as u16);
        // as nossas (0xE8 e o Win solto) passam sempre; as dos outros só fora do teste
        let nossa = k.dwExtraInfo == MARCA;
        let injetada = k.flags.0 & LLKHF_INJECTED.0 != 0 && !ACEITA_SIMULADA.load(Ordering::Relaxed);
        if !nossa && !injetada {
            let desce = wp.0 as u32 == WM_KEYDOWN || wp.0 as u32 == WM_SYSKEYDOWN;
            let win = vk == VK_LWIN || vk == VK_RWIN;
            if win && desce {
                // a repetição automática manda vários "desce": só o primeiro arma
                let outro = if vk == VK_LWIN { VK_RWIN } else { VK_LWIN };
                if !apertada(vk) && !apertada(outro) {
                    SOZINHO.store(true, Ordering::Relaxed);
                }
            } else if win {
                if SOZINHO.swap(false, Ordering::Relaxed) {
                    soltar_mascarado(vk);
                    let d = HWND(DESTINO.load(Ordering::Relaxed) as *mut _);
                    _ = PostMessageW(Some(d), MENSAGEM.load(Ordering::Relaxed) as u32, WPARAM(0), LPARAM(0));
                    return LRESULT(1);
                }
            } else if desce {
                SOZINHO.store(false, Ordering::Relaxed);
            }
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

fn apertada(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000 != 0 }
}

/// Marca das teclas que o próprio gancho manda.
const MARCA: usize = 0x4C41_4E43;

/// 0xE8 aperta e solta, depois o Win solta: tudo atrás do Win solto real, que foi segurado.
unsafe fn soltar_mascarado(win: VIRTUAL_KEY) {
    let tecla = |vk: VIRTUAL_KEY, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                dwExtraInfo: MARCA,
                ..Default::default()
            },
        },
    };
    let mut seq = [tecla(MASCARA, false), tecla(MASCARA, true), tecla(win, true)];
    seq[2].Anonymous.ki.dwFlags |= KEYEVENTF_EXTENDEDKEY;
    SendInput(&seq, size_of::<INPUT>() as i32);
}

/// Instala o gancho numa linha de execução só dele, para o laço da janela (pintura, ícones)
/// não atrasar o teclado do sistema inteiro.
pub fn instalar(destino: HWND, mensagem: u32) {
    DESTINO.store(destino.0 as isize, Ordering::Relaxed);
    MENSAGEM.store(mensagem as isize, Ordering::Relaxed);
    ACEITA_SIMULADA.store(std::env::var_os("LANCADOR_TESTE").is_some(), Ordering::Relaxed);
    std::thread::spawn(|| unsafe {
        let inst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default();
        let Ok(_g) = SetWindowsHookExW(WH_KEYBOARD_LL, Some(gancho), Some(inst.into()), 0) else { return };
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    });
}
