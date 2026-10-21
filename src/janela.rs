//! Janela única, escondida até o Alt+Space. Campo EDIT nativo e lista desenhada em GDI.

use std::{collections::HashMap, fs, ptr::null_mut};

use windows::{
    core::{w, HSTRING, PCWSTR},
    Win32::{
        Foundation::*,
        Graphics::{Dwm::*, Gdi::*},
        System::{
            Com::{CoInitializeEx, COINIT_APARTMENTTHREADED},
            LibraryLoader::GetModuleHandleW,
            Threading::CreateMutexW,
        },
        UI::{
            HiDpi::*,
            Input::KeyboardAndMouse::*,
            Shell::{ShellExecuteExW, SHELLEXECUTEINFOW},
            WindowsAndMessaging::*,
        },
    },
};

use windows::Win32::UI::Controls::EM_SETSEL;

use crate::{
    busca,
    itens::{self, Item},
};

const CLASSE: PCWSTR = w!("LancadorRs");
const WM_FIXADOS: u32 = WM_APP + 1;
const WM_MOSTRAR: u32 = WM_APP + 2;
const EN_CHANGE: u32 = 0x0300;
const MAX_VISIVEIS: usize = 9;

// Medidas a 96 dpi.
const LARGURA: i32 = 640;
const MARGEM: i32 = 8;
const ALT_BUSCA: i32 = 52;
const ALT_ITEM: i32 = 46;
const TAM_ICONE: i32 = 32;

const fn rgb(r: u32, g: u32, b: u32) -> COLORREF {
    COLORREF(r | (g << 8) | (b << 16))
}
const COR_FUNDO: COLORREF = rgb(32, 32, 32);
const COR_SELECAO: COLORREF = rgb(56, 56, 56);
const COR_SEPARADOR: COLORREF = rgb(58, 58, 58);
const COR_TEXTO: COLORREF = rgb(240, 240, 240);
const COR_DETALHE: COLORREF = rgb(150, 150, 150);
const COR_DESTAQUE: COLORREF = rgb(76, 194, 255);

struct Estado {
    hwnd: HWND,
    edit: HWND,
    fixados: Vec<Item>,
    sistema: Vec<Item>,
    /// (é fixado, índice na lista)
    resultados: Vec<(bool, usize)>,
    sel: usize,
    topo: usize,
    uso: HashMap<String, u32>,
    dpi: u32,
    fonte_busca: HFONT,
    fonte_nome: HFONT,
    fonte_detalhe: HFONT,
    pincel_fundo: HBRUSH,
    exportando: bool,
    mouse: (i32, i32),
}

// Só a thread da janela toca no estado.
static mut ESTADO: *mut Estado = null_mut();

fn st() -> &'static mut Estado {
    unsafe { &mut *ESTADO }
}

fn esc(v: i32) -> i32 {
    v * st().dpi as i32 / 96
}

pub fn rodar() {
    let sair = std::env::args().any(|a| a == "--sair");
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _mutex = CreateMutexW(None, false, w!("Local\\lancador-rs"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if let Ok(h) = FindWindowW(CLASSE, PCWSTR::null()) {
                let msg = if sair { WM_CLOSE } else { WM_MOSTRAR };
                let _ = PostMessageW(Some(h), msg, WPARAM(0), LPARAM(0));
            }
            return;
        }
        if sair {
            return;
        }
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let inst: HINSTANCE = GetModuleHandleW(None).unwrap().into();
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc_janela),
            hInstance: inst,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: CLASSE,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            CLASSE,
            w!("Lançador"),
            WS_POPUP | WS_CLIPCHILDREN,
            0,
            0,
            LARGURA,
            100,
            None,
            None,
            Some(inst),
            None,
        )
        .unwrap();
        let edit = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("EDIT"),
            w!(""),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            0,
            0,
            10,
            10,
            Some(hwnd),
            None,
            Some(inst),
            None,
        )
        .unwrap();

        let canto = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &canto as *const _ as _, 4);
        let borda = rgb(70, 70, 70);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, &borda as *const _ as _, 4);

        let dpi = GetDpiForWindow(hwnd).max(96);
        ESTADO = Box::into_raw(Box::new(Estado {
            hwnd,
            edit,
            fixados: Vec::new(),
            sistema: Vec::new(),
            resultados: Vec::new(),
            sel: 0,
            topo: 0,
            uso: carregar_uso(),
            dpi,
            fonte_busca: HFONT::default(),
            fonte_nome: HFONT::default(),
            fonte_detalhe: HFONT::default(),
            pincel_fundo: CreateSolidBrush(COR_FUNDO),
            exportando: false,
            mouse: (-1, -1),
        }));
        criar_fontes();
        let tam = esc(TAM_ICONE);
        st().fixados = itens::carregar_fixados(tam);
        st().sistema = itens::carregar_sistema(tam);
        atualizar_busca();
        verificar_fixados();

        if RegisterHotKey(Some(hwnd), 1, MOD_ALT | MOD_NOREPEAT, VK_SPACE.0 as u32).is_err() {
            MessageBoxW(None, w!("Alt+Space já está em uso por outro programa."), w!("Lançador"), MB_ICONERROR);
            return;
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.hwnd == edit && tecla_do_campo(&msg) {
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Teclas de navegação no campo de busca. true = consumida.
unsafe fn tecla_do_campo(msg: &MSG) -> bool {
    let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0;
    let shift = GetKeyState(VK_SHIFT.0 as i32) < 0;
    match msg.message {
        WM_KEYDOWN => match VIRTUAL_KEY(msg.wParam.0 as u16) {
            VK_DOWN => mover(1),
            VK_UP => mover(-1),
            VK_TAB => mover(if shift { -1 } else { 1 }),
            VK_NEXT => mover(MAX_VISIVEIS as i32),
            VK_PRIOR => mover(-(MAX_VISIVEIS as i32)),
            VK_RETURN => abrir(ctrl && shift),
            VK_ESCAPE => esconder(),
            VK_BACK if ctrl => apagar_palavra(),
            _ => return false,
        },
        // Sem o WM_CHAR desses o EDIT apita.
        WM_CHAR => return matches!(msg.wParam.0, 0x0d | 0x1b | 0x09 | 0x7f),
        _ => return false,
    }
    true
}

unsafe extern "system" fn proc_janela(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if ESTADO.is_null() {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    match msg {
        WM_HOTKEY => {
            if IsWindowVisible(hwnd).as_bool() {
                esconder()
            } else {
                mostrar()
            }
        }
        WM_MOSTRAR => mostrar(),
        WM_FIXADOS => {
            st().exportando = false;
            st().fixados = itens::carregar_fixados(esc(TAM_ICONE));
            atualizar_busca();
        }
        WM_ACTIVATE => {
            if (wp.0 & 0xffff) as u32 == WA_INACTIVE {
                esconder();
            }
        }
        WM_COMMAND => {
            if ((wp.0 >> 16) & 0xffff) as u32 == EN_CHANGE {
                atualizar_busca();
            }
        }
        WM_CTLCOLOREDIT => {
            let hdc = HDC(wp.0 as _);
            SetTextColor(hdc, COR_TEXTO);
            SetBkColor(hdc, COR_FUNDO);
            return LRESULT(st().pincel_fundo.0 as isize);
        }
        WM_ERASEBKGND => return LRESULT(1),
        WM_PAINT => pintar(hwnd),
        WM_MOUSEMOVE => {
            let p = ((lp.0 & 0xffff) as i16 as i32, ((lp.0 >> 16) & 0xffff) as i16 as i32);
            // Janela que aparece sob o cursor parado recebe um WM_MOUSEMOVE: não mexer na seleção.
            if p != st().mouse {
                st().mouse = p;
                if let Some(i) = linha_em(p.1) {
                    if i != st().sel {
                        st().sel = i;
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                }
            }
        }
        WM_LBUTTONUP => {
            if let Some(i) = linha_em(((lp.0 >> 16) & 0xffff) as i16 as i32) {
                st().sel = i;
                abrir(false);
            }
        }
        WM_MOUSEWHEEL => mover(if ((wp.0 >> 16) & 0xffff) as i16 > 0 { -1 } else { 1 }),
        WM_DPICHANGED => {
            st().dpi = ((wp.0 & 0xffff) as u32).max(96);
            criar_fontes();
            ajustar_tamanho();
        }
        WM_DESTROY => PostQuitMessage(0),
        _ => return DefWindowProcW(hwnd, msg, wp, lp),
    }
    LRESULT(0)
}

unsafe fn criar_fontes() {
    let s = st();
    for f in [s.fonte_busca, s.fonte_nome, s.fonte_detalhe] {
        if !f.is_invalid() {
            let _ = DeleteObject(f.into());
        }
    }
    let fonte = |px: i32, peso: i32| {
        let mut lf = LOGFONTW {
            lfHeight: -esc(px),
            lfWeight: peso,
            lfCharSet: DEFAULT_CHARSET,
            lfQuality: CLEARTYPE_QUALITY,
            ..Default::default()
        };
        for (i, c) in "Segoe UI".encode_utf16().enumerate() {
            lf.lfFaceName[i] = c;
        }
        CreateFontIndirectW(&lf)
    };
    s.fonte_busca = fonte(22, 400);
    s.fonte_nome = fonte(15, 400);
    s.fonte_detalhe = fonte(12, 400);
    SendMessageW(s.edit, WM_SETFONT, Some(WPARAM(s.fonte_busca.0 as usize)), Some(LPARAM(1)));
}

fn item(r: (bool, usize)) -> &'static Item {
    let s = st();
    if r.0 {
        &s.fixados[r.1]
    } else {
        &s.sistema[r.1]
    }
}

fn texto_do_campo() -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(st().edit, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n])
}

unsafe fn atualizar_busca() {
    let consulta = busca::dobrar(texto_do_campo().trim());
    let s = st();
    s.resultados = if consulta.is_empty() {
        (0..s.fixados.len()).map(|i| (true, i)).collect()
    } else {
        let mut v: Vec<(u32, bool, usize)> = Vec::new();
        for (fixado, lista) in [(true, &s.fixados), (false, &s.sistema)] {
            for (i, it) in lista.iter().enumerate() {
                let p = it.chaves.iter().map(|k| busca::pontuar(&consulta, k)).max().unwrap_or(0);
                if p > 0 {
                    let bonus = s.uso.get(&it.id()).map_or(0, |n| (n * 15).min(150));
                    v.push((p + bonus, fixado, i));
                }
            }
        }
        v.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then(b.1.cmp(&a.1))
                .then(item((a.1, a.2)).nome.len().cmp(&item((b.1, b.2)).nome.len()))
        });
        let mut vistos = std::collections::HashSet::new();
        v.into_iter()
            .filter(|r| vistos.insert(busca::dobrar(&item((r.1, r.2)).nome)))
            .take(MAX_VISIVEIS)
            .map(|r| (r.1, r.2))
            .collect()
    };
    s.sel = 0;
    s.topo = 0;
    ajustar_tamanho();
    let _ = InvalidateRect(Some(s.hwnd), None, false);
}

fn visiveis() -> usize {
    st().resultados.len().min(MAX_VISIVEIS)
}

fn topo_lista() -> i32 {
    esc(MARGEM + ALT_BUSCA) + esc(5)
}

fn altura() -> i32 {
    match visiveis() {
        0 => esc(MARGEM * 2 + ALT_BUSCA),
        n => topo_lista() + n as i32 * esc(ALT_ITEM) + esc(MARGEM),
    }
}

unsafe fn ajustar_tamanho() {
    let s = st();
    let _ = SetWindowPos(
        s.hwnd,
        None,
        0,
        0,
        esc(LARGURA),
        altura(),
        SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
    );
    let alt_edit = esc(30);
    let _ = SetWindowPos(
        s.edit,
        None,
        esc(20),
        esc(MARGEM) + (esc(ALT_BUSCA) - alt_edit) / 2,
        esc(LARGURA - 40),
        alt_edit,
        SWP_NOZORDER | SWP_NOACTIVATE,
    );
}

fn linha_em(y: i32) -> Option<usize> {
    let t = topo_lista();
    if y < t {
        return None;
    }
    let i = ((y - t) / esc(ALT_ITEM)) as usize;
    (i < visiveis()).then(|| st().topo + i)
}

unsafe fn mover(d: i32) {
    let s = st();
    let n = s.resultados.len() as i32;
    if n == 0 {
        return;
    }
    s.sel = if d.abs() == 1 {
        (s.sel as i32 + d).rem_euclid(n) as usize
    } else {
        (s.sel as i32 + d).clamp(0, n - 1) as usize
    };
    if s.sel < s.topo {
        s.topo = s.sel;
    } else if s.sel >= s.topo + MAX_VISIVEIS {
        s.topo = s.sel + 1 - MAX_VISIVEIS;
    }
    let _ = InvalidateRect(Some(s.hwnd), None, false);
}

unsafe fn apagar_palavra() {
    let t = texto_do_campo();
    let t = t.trim_end();
    let corte = t.rfind(char::is_whitespace).map_or(0, |i| i + 1);
    let novo = HSTRING::from(&t[..corte]);
    let _ = SetWindowTextW(st().edit, &novo);
    let n = novo.len();
    SendMessageW(st().edit, EM_SETSEL, Some(WPARAM(n)), Some(LPARAM(n as isize)));
}

unsafe fn mostrar() {
    verificar_fixados();
    let s = st();
    let mut p = POINT::default();
    let _ = GetCursorPos(&mut p);
    let mon = MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST);
    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    let _ = GetMonitorInfoW(mon, &mut mi);
    let (mut dx, mut dy) = (96, 96);
    let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
    if dx != s.dpi {
        s.dpi = dx.max(96);
        criar_fontes();
    }
    let w = mi.rcWork;
    let largura = esc(LARGURA);
    let x = w.left + (w.right - w.left - largura) / 2;
    let y = w.top + (w.bottom - w.top) / 4;
    let _ = SetWindowPos(s.hwnd, Some(HWND_TOPMOST), x, y, largura, altura(), SWP_NOACTIVATE);
    ajustar_tamanho();
    let _ = ShowWindow(s.hwnd, SW_SHOW);
    let _ = SetForegroundWindow(s.hwnd);
    let _ = SetFocus(Some(s.edit));
    let mut c = p;
    let _ = ScreenToClient(s.hwnd, &mut c);
    s.mouse = (c.x, c.y);
}

/// Some e limpa: o próximo Alt+Space abre do zero, com os fixados.
unsafe fn esconder() {
    let s = st();
    if !IsWindowVisible(s.hwnd).as_bool() {
        return;
    }
    let _ = ShowWindow(s.hwnd, SW_HIDE);
    let _ = SetWindowTextW(s.edit, w!(""));
}

unsafe fn abrir(admin: bool) {
    let s = st();
    let Some(&r) = s.resultados.get(s.sel) else { return };
    let it = item(r);
    let arquivo = HSTRING::from(&it.arquivo);
    let args = HSTRING::from(&it.args);
    let id = it.id();
    let mut sei = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        hwnd: s.hwnd,
        lpVerb: if admin { w!("runas") } else { PCWSTR::null() },
        lpFile: PCWSTR(arquivo.as_ptr()),
        lpParameters: if args.is_empty() { PCWSTR::null() } else { PCWSTR(args.as_ptr()) },
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // Abre antes de esconder: com a janela ainda em primeiro plano o app novo pode tomar o foco.
    let ok = ShellExecuteExW(&mut sei).is_ok();
    esconder();
    if ok {
        *st().uso.entry(id).or_default() += 1;
        salvar_uso();
    }
}

unsafe fn pintar(hwnd: HWND) {
    let s = st();
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);
    let mem = CreateCompatibleDC(Some(hdc));
    let bmp = CreateCompatibleBitmap(hdc, rc.right, rc.bottom);
    let antigo = SelectObject(mem, bmp.into());

    FillRect(mem, &rc, s.pincel_fundo);
    SetBkMode(mem, TRANSPARENT);
    let preencher = |r: RECT, cor: COLORREF| {
        let b = CreateSolidBrush(cor);
        FillRect(mem, &r, b);
        let _ = DeleteObject(b.into());
    };

    if visiveis() > 0 {
        let y = esc(MARGEM + ALT_BUSCA);
        preencher(RECT { left: esc(12), top: y, right: rc.right - esc(12), bottom: y + 1 }, COR_SEPARADOR);
    }
    let alt = esc(ALT_ITEM);
    let icone = esc(TAM_ICONE);
    let x_texto = esc(18) + icone + esc(12);
    for (vi, &r) in s.resultados.iter().skip(s.topo).take(MAX_VISIVEIS).enumerate() {
        let it = item(r);
        let y = topo_lista() + vi as i32 * alt;
        if s.topo + vi == s.sel {
            let b = CreateSolidBrush(COR_SELECAO);
            let pb = SelectObject(mem, b.into());
            let pp = SelectObject(mem, GetStockObject(NULL_PEN));
            let _ = RoundRect(mem, esc(6), y + 1, rc.right - esc(6), y + alt - 1, esc(8), esc(8));
            SelectObject(mem, pb);
            SelectObject(mem, pp);
            let _ = DeleteObject(b.into());
            preencher(
                RECT { left: esc(6), top: y + esc(13), right: esc(9), bottom: y + alt - esc(13) },
                COR_DESTAQUE,
            );
        }
        if !it.icone.is_invalid() {
            let _ = DrawIconEx(mem, esc(18), y + (alt - icone) / 2, it.icone, icone, icone, 0, None, DI_NORMAL);
        }
        let direita = rc.right - esc(16);
        let mut nome: Vec<u16> = it.nome.encode_utf16().collect();
        let mut detalhe: Vec<u16> = it.detalhe.encode_utf16().collect();
        let fmt = DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX | DT_VCENTER;
        SelectObject(mem, s.fonte_nome.into());
        SetTextColor(mem, COR_TEXTO);
        let mut rn = RECT { left: x_texto, top: y + esc(4), right: direita, bottom: y + esc(25) };
        DrawTextW(mem, &mut nome, &mut rn, fmt);
        SelectObject(mem, s.fonte_detalhe.into());
        SetTextColor(mem, COR_DETALHE);
        let mut rd = RECT { left: x_texto, top: y + esc(25), right: direita, bottom: y + esc(42) };
        DrawTextW(mem, &mut detalhe, &mut rd, fmt);
    }

    let _ = BitBlt(hdc, 0, 0, rc.right, rc.bottom, Some(mem), 0, 0, SRCCOPY);
    SelectObject(mem, antigo);
    let _ = DeleteObject(bmp.into());
    let _ = DeleteDC(mem);
    let _ = EndPaint(hwnd, &ps);
}

/// Se o layout do Iniciar mudou, exporta de novo numa thread e avisa a janela no fim.
fn verificar_fixados() {
    let s = st();
    if s.exportando || !itens::fixados_desatualizados() {
        return;
    }
    s.exportando = true;
    let h = s.hwnd.0 as isize;
    std::thread::spawn(move || {
        itens::exportar_fixados();
        unsafe {
            let _ = PostMessageW(Some(HWND(h as _)), WM_FIXADOS, WPARAM(0), LPARAM(0));
        }
    });
}

fn carregar_uso() -> HashMap<String, u32> {
    fs::read_to_string(itens::dir_dados().join("uso.tsv"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let (id, n) = l.rsplit_once('\t')?;
            Some((id.to_string(), n.parse().ok()?))
        })
        .collect()
}

fn salvar_uso() {
    let texto: String = st().uso.iter().map(|(id, n)| format!("{id}\t{n}\n")).collect();
    let _ = fs::write(itens::dir_dados().join("uso.tsv"), texto);
}
