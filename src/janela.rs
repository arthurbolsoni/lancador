//! Janela única, escondida até o Alt+Space. Fundo acrílico do DWM; o conteúdo é desenhado em
//! Direct2D num DIB de 32 bits com alfa e copiado para a área cliente (frame estendido).
//! O campo de busca é próprio: o EDIT do Win32 pinta fundo opaco e furaria o acrílico.

use std::{
    collections::HashMap,
    fs,
    ptr::null_mut,
    time::Instant,
};

use windows::{
    core::{w, HSTRING, PCWSTR},
    Win32::{
        Foundation::*,
        Graphics::{
            Direct2D::{Common::*, *},
            DirectWrite::*,
            Dwm::*,
            Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
            Gdi::*,
            Imaging::*,
        },
        System::{
            Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED},
            DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard},
            LibraryLoader::GetModuleHandleW,
            Memory::{GetProcessHeap, GlobalLock, GlobalUnlock, HeapCompact, HEAP_FLAGS},
            Threading::CreateMutexW,
        },
        UI::{
            Controls::MARGINS,
            HiDpi::*,
            Input::KeyboardAndMouse::*,
            Shell::{ShellExecuteExW, SHELLEXECUTEINFOW},
            WindowsAndMessaging::*,
        },
    },
};

use crate::{
    busca,
    itens::{self, Icone, Item},
    tema::{self, Tema},
};

const CLASSE: PCWSTR = w!("LancadorRs");
const WM_FIXADOS: u32 = WM_APP + 1;
const WM_APPS: u32 = WM_APP + 3;
/// Win solto sozinho (vem do gancho de teclado da barra): o mesmo que o Alt+Space.
const WM_WIN: u32 = WM_APP + 4;
/// Releitura de "Todos os aplicativos" no máximo uma vez por este intervalo, ao abrir.
const INTERVALO_APPS: std::time::Duration = std::time::Duration::from_secs(60);

// Origem de um resultado; o maior ganha no empate de pontuação.
const SISTEMA: u8 = 0;
const APP: u8 = 1;
const FIXADO: u8 = 2;
const WM_MOSTRAR: u32 = WM_APP + 2;
const TIMER_ANIM: usize = 1;
const TIMER_CARET: usize = 2;
const MAX_VISIVEIS: usize = 8;

// Medidas em DIPs.
const LARGURA: f32 = 680.0;
const ALT_BUSCA: f32 = 60.0;
const ALT_ITEM: f32 = 44.0;
const TOPO_LISTA: f32 = ALT_BUSCA + 6.0;
const FUNDO_LISTA: f32 = 8.0;
const X_TEXTO_BUSCA: f32 = 22.0;
const X_ICONE: f32 = 22.0;
const TAM_ICONE: f32 = 26.0;
const X_NOME: f32 = 62.0;

// Constantes de tempo das animações (s): exponencial, sem fim abrupto.
const TAU_SELECAO: f32 = 0.045;
const TAU_ALTURA: f32 = 0.06;
const DUR_ABERTURA: f32 = 0.16;

const fn cor(r: u8, g: u8, b: u8, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a }
}
fn hex(h: u32, a: f32) -> D2D1_COLOR_F {
    cor((h >> 16) as u8, (h >> 8) as u8, h as u8, a)
}

/// `a` levado `f` (0..1) na direção de `b`, como na barra.
fn mistura(a: u32, b: u32, f: f32) -> u32 {
    let canal = |s: u32| {
        let (x, y) = (((a >> s) & 0xFF) as f32, ((b >> s) & 0xFF) as f32);
        ((x + (y - x) * f).round() as u32) << s
    };
    canal(16) | canal(8) | canal(0)
}

/// Cores do lançador derivadas do tema da barra.
struct Cores {
    /// Véu sobre o acrílico: o fundo do tema, com um pouco do acrílico passando.
    veu: D2D1_COLOR_F,
    texto: D2D1_COLOR_F,
    dica: D2D1_COLOR_F,
    nome: D2D1_COLOR_F,
    selecao: D2D1_COLOR_F,
    linha: D2D1_COLOR_F,
    marca_texto: D2D1_COLOR_F,
    destaque: D2D1_COLOR_F,
}

fn cores(t: &Tema) -> Cores {
    Cores {
        veu: hex(t.fundo, 0.84),
        texto: hex(t.texto_forte, 1.0),
        dica: hex(mistura(t.texto, t.fundo, 0.45), 1.0),
        nome: hex(t.texto, 1.0),
        selecao: hex(t.selecao, 1.0),
        linha: hex(t.selecao, 1.0),
        marca_texto: hex(t.destaque, 0.38),
        destaque: hex(t.destaque, 1.0),
    }
}

struct Desenho {
    dw: IDWriteFactory,
    wic: IWICImagingFactory,
    fabrica: ID2D1Factory,
    rt: ID2D1DCRenderTarget,
    pincel: ID2D1SolidColorBrush,
    fmt_busca: IDWriteTextFormat,
    fmt_nome: IDWriteTextFormat,
    fmt_glifo: IDWriteTextFormat,
    bitmaps: HashMap<String, ID2D1Bitmap>,
    memdc: HDC,
    dib: HBITMAP,
    dib_tam: (i32, i32),
}

struct Estado {
    hwnd: HWND,
    d: Desenho,
    fixados: Vec<Item>,
    apps: Vec<Item>,
    apps_lidos: Option<Instant>,
    listando: bool,
    sistema: Vec<Item>,
    /// (origem, índice na lista da origem)
    resultados: Vec<(u8, usize)>,
    sel: usize,
    topo: usize,
    texto: Vec<char>,
    cursor: usize,
    tudo_marcado: bool,
    caret_aceso: bool,
    uso: HashMap<String, u32>,
    tema: &'static Tema,
    cores: Cores,
    escala: f32,
    exportando: bool,
    mouse: (i32, i32),
    // animação
    sel_y: f32,
    altura: f32,
    abertura: Option<Instant>,
    ultimo_quadro: Instant,
    animando: bool,
}

// Só a thread da janela toca no estado.
static mut ESTADO: *mut Estado = null_mut();

fn st() -> &'static mut Estado {
    unsafe { &mut *ESTADO }
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
            WS_POPUP,
            0,
            0,
            LARGURA as i32,
            ALT_BUSCA as i32,
            None,
            None,
            Some(inst),
            None,
        )
        .unwrap();

        let fundo = DWMSBT_TRANSIENTWINDOW;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, &fundo as *const _ as _, 4);
        let canto = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &canto as *const _ as _, 4);
        let _ = DwmExtendFrameIntoClientArea(
            hwnd,
            &MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 },
        );

        let escala = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        let t = tema::escolhido();
        aplicar_tema_dwm(hwnd, t);
        ESTADO = Box::into_raw(Box::new(Estado {
            hwnd,
            d: criar_desenho().expect("Direct2D"),
            fixados: itens::carregar_fixados(),
            apps: Vec::new(),
            apps_lidos: None,
            listando: false,
            sistema: itens::carregar_sistema(),
            resultados: Vec::new(),
            sel: 0,
            topo: 0,
            texto: Vec::new(),
            cursor: 0,
            tudo_marcado: false,
            caret_aceso: true,
            uso: carregar_uso(),
            tema: t,
            cores: cores(t),
            escala,
            exportando: false,
            mouse: (-1, -1),
            sel_y: 0.0,
            altura: ALT_BUSCA,
            abertura: None,
            ultimo_quadro: Instant::now(),
            animando: false,
        }));
        atualizar_busca();
        verificar_fixados();
        verificar_apps();

        if RegisterHotKey(Some(hwnd), 1, MOD_ALT | MOD_NOREPEAT, VK_SPACE.0 as u32).is_err() {
            MessageBoxW(None, w!("Alt+Space já está em uso por outro programa."), w!("Lançador"), MB_ICONERROR);
            return;
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Acrílico claro ou escuro conforme o tema; borda na cor apagada do tema.
unsafe fn aplicar_tema_dwm(hwnd: HWND, t: &Tema) {
    let escuro: i32 = (!t.claro).into();
    let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &escuro as *const _ as _, 4);
    // COLORREF é 0x00BBGGRR.
    let m = t.mudo;
    let borda = COLORREF(((m & 0xFF) << 16) | (m & 0xFF00) | ((m >> 16) & 0xFF));
    let _ = DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, &borda as *const _ as _, 4);
}

/// Relê o tema da barra; se mudou desde a última abertura, troca cores e acrílico.
unsafe fn atualizar_tema() {
    let s = st();
    let t = tema::escolhido();
    if t.nome != s.tema.nome {
        s.tema = t;
        s.cores = cores(t);
        aplicar_tema_dwm(s.hwnd, t);
    }
}

unsafe fn criar_desenho() -> windows::core::Result<Desenho> {
    let fabrica: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
    let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
    let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
    let rt = criar_alvo(&fabrica)?;
    let pincel = rt.CreateSolidColorBrush(&cor(0, 0, 0, 1.0), None)?;

    let formato = |familia: PCWSTR, peso: DWRITE_FONT_WEIGHT, tam: f32| -> windows::core::Result<IDWriteTextFormat> {
        let f = dw.CreateTextFormat(familia, None, peso, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, tam, w!("pt-BR"))?;
        f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        Ok(f)
    };
    let fmt_busca = formato(w!("Segoe UI Variable Display"), DWRITE_FONT_WEIGHT_SEMI_LIGHT, 22.0)?;
    let fmt_nome = formato(w!("Segoe UI Variable Text"), DWRITE_FONT_WEIGHT_NORMAL, 14.5)?;
    let reticencias = dw.CreateEllipsisTrimmingSign(&fmt_nome)?;
    fmt_nome.SetTrimming(
        &DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, ..Default::default() },
        &reticencias,
    )?;
    let fmt_glifo = formato(w!("Segoe Fluent Icons"), DWRITE_FONT_WEIGHT_NORMAL, 17.0)?;
    fmt_glifo.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;

    Ok(Desenho {
        dw,
        wic,
        fabrica,
        rt,
        pincel,
        fmt_busca,
        fmt_nome,
        fmt_glifo,
        bitmaps: HashMap::new(),
        memdc: CreateCompatibleDC(None),
        dib: HBITMAP::default(),
        dib_tam: (0, 0),
    })
}

unsafe fn criar_alvo(fabrica: &ID2D1Factory) -> windows::core::Result<ID2D1DCRenderTarget> {
    let props = D2D1_RENDER_TARGET_PROPERTIES {
        r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
        pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
        dpiX: 0.0,
        dpiY: 0.0,
        usage: D2D1_RENDER_TARGET_USAGE_GDI_COMPATIBLE,
        minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
    };
    // Software: a janela e pequena e o modo hardware carrega o driver da GPU (~50 MB privados).
    let rt = fabrica.CreateDCRenderTarget(&props)?;
    // Cinza (não ClearType): o ClearType não sabe misturar com fundo translúcido.
    rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
    Ok(rt)
}

unsafe extern "system" fn proc_janela(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if ESTADO.is_null() {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    match msg {
        WM_HOTKEY | WM_WIN => {
            if IsWindowVisible(hwnd).as_bool() {
                esconder()
            } else {
                mostrar()
            }
        }
        WM_MOSTRAR => mostrar(),
        WM_FIXADOS => {
            st().exportando = false;
            st().d.bitmaps.clear();
            st().fixados = itens::carregar_fixados();
            atualizar_busca();
        }
        WM_APPS => receber_apps(lp),
        WM_ACTIVATE => {
            if (wp.0 & 0xffff) as u32 == WA_INACTIVE {
                esconder();
            }
        }
        WM_KEYDOWN => tecla(VIRTUAL_KEY(wp.0 as u16)),
        WM_CHAR => {
            if let Some(c) = char::from_u32(wp.0 as u32).filter(|c| !c.is_control()) {
                inserir(&[c]);
            }
        }
        WM_TIMER => match wp.0 {
            TIMER_ANIM => quadro(),
            TIMER_CARET => {
                st().caret_aceso = !st().caret_aceso;
                redesenhar();
            }
            _ => {}
        },
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
                        animar();
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
            st().escala = ((wp.0 & 0xffff) as u32).max(96) as f32 / 96.0;
            aplicar_tamanho();
        }
        WM_DESTROY => PostQuitMessage(0),
        _ => return DefWindowProcW(hwnd, msg, wp, lp),
    }
    LRESULT(0)
}

// ---------- campo de busca ----------

unsafe fn tecla(vk: VIRTUAL_KEY) {
    let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0;
    let shift = GetKeyState(VK_SHIFT.0 as i32) < 0;
    let s = st();
    match vk {
        VK_DOWN => mover(1),
        VK_UP => mover(-1),
        VK_TAB => mover(if shift { -1 } else { 1 }),
        VK_NEXT => mover(MAX_VISIVEIS as i32),
        VK_PRIOR => mover(-(MAX_VISIVEIS as i32)),
        VK_RETURN => abrir(ctrl && shift),
        VK_ESCAPE => esconder(),
        VK_BACK | VK_DELETE if s.tudo_marcado => {
            s.texto.clear();
            s.cursor = 0;
            texto_mudou();
        }
        VK_BACK if s.cursor > 0 => {
            let inicio = if ctrl { inicio_palavra(&s.texto, s.cursor) } else { s.cursor - 1 };
            s.texto.drain(inicio..s.cursor);
            s.cursor = inicio;
            texto_mudou();
        }
        VK_DELETE if s.cursor < s.texto.len() => {
            s.texto.remove(s.cursor);
            texto_mudou();
        }
        VK_LEFT | VK_RIGHT | VK_HOME | VK_END => {
            s.cursor = match vk {
                VK_HOME => 0,
                VK_END => s.texto.len(),
                VK_LEFT if ctrl => inicio_palavra(&s.texto, s.cursor),
                VK_LEFT => s.cursor.saturating_sub(1),
                _ if ctrl => fim_palavra(&s.texto, s.cursor),
                _ => (s.cursor + 1).min(s.texto.len()),
            };
            s.tudo_marcado = false;
            acender_caret();
        }
        VIRTUAL_KEY(0x41) if ctrl => {
            s.tudo_marcado = !s.texto.is_empty();
            redesenhar();
        }
        VIRTUAL_KEY(0x56) if ctrl => {
            let colado: Vec<char> = area_de_transferencia().lines().next().unwrap_or("").chars().collect();
            inserir(&colado);
        }
        _ => {}
    }
}

fn inicio_palavra(t: &[char], mut i: usize) -> usize {
    while i > 0 && t[i - 1].is_whitespace() {
        i -= 1;
    }
    while i > 0 && !t[i - 1].is_whitespace() {
        i -= 1;
    }
    i
}

fn fim_palavra(t: &[char], mut i: usize) -> usize {
    while i < t.len() && t[i].is_whitespace() {
        i += 1;
    }
    while i < t.len() && !t[i].is_whitespace() {
        i += 1;
    }
    i
}

unsafe fn inserir(cs: &[char]) {
    if cs.is_empty() {
        return;
    }
    let s = st();
    if s.tudo_marcado {
        s.texto.clear();
        s.cursor = 0;
    }
    for (i, &c) in cs.iter().enumerate() {
        s.texto.insert(s.cursor + i, c);
    }
    s.cursor += cs.len();
    texto_mudou();
}

unsafe fn texto_mudou() {
    st().tudo_marcado = false;
    atualizar_busca();
    acender_caret();
}

/// Caret aceso logo depois de digitar/mover, e o pisca recomeça.
unsafe fn acender_caret() {
    let s = st();
    s.caret_aceso = true;
    if IsWindowVisible(s.hwnd).as_bool() {
        SetTimer(Some(s.hwnd), TIMER_CARET, GetCaretBlinkTime(), None);
    }
    redesenhar();
}

unsafe fn area_de_transferencia() -> String {
    let mut texto = String::new();
    if OpenClipboard(Some(st().hwnd)).is_ok() {
        if let Ok(h) = GetClipboardData(13) {
            // CF_UNICODETEXT
            let p = GlobalLock(HGLOBAL(h.0)) as *const u16;
            if !p.is_null() {
                let mut n = 0;
                while *p.add(n) != 0 {
                    n += 1;
                }
                texto = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
                let _ = GlobalUnlock(HGLOBAL(h.0));
            }
        }
        let _ = CloseClipboard();
    }
    texto
}

// ---------- resultados ----------

fn item(r: (u8, usize)) -> &'static mut Item {
    let s = st();
    match r.0 {
        FIXADO => &mut s.fixados[r.1],
        APP => &mut s.apps[r.1],
        _ => &mut s.sistema[r.1],
    }
}

unsafe fn atualizar_busca() {
    let s = st();
    let consulta = busca::dobrar(s.texto.iter().collect::<String>().trim());
    s.resultados = if consulta.is_empty() {
        (0..s.fixados.len()).map(|i| (FIXADO, i)).collect()
    } else {
        let mut v: Vec<(u32, u8, usize)> = Vec::new();
        for (origem, lista) in [(FIXADO, &s.fixados), (APP, &s.apps), (SISTEMA, &s.sistema)] {
            for (i, it) in lista.iter().enumerate() {
                let p = it.chaves.iter().map(|k| busca::pontuar(&consulta, k)).max().unwrap_or(0);
                if p > 0 {
                    let bonus = s.uso.get(&it.id()).map_or(0, |n| (n * 15).min(150));
                    v.push((p + bonus, origem, i));
                }
            }
        }
        v.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then(b.1.cmp(&a.1))
                .then(item((a.1, a.2)).nome.len().cmp(&item((b.1, b.2)).nome.len()))
        });
        // Com algum casamento de verdade (>= sigla solta), letras soltas pelo nome viram ruído.
        if v.iter().any(|r| r.0 >= 500) {
            v.retain(|r| r.0 >= 500);
        }
        let mut vistos = std::collections::HashSet::new();
        v.into_iter()
            .filter(|r| vistos.insert(busca::dobrar(&item((r.1, r.2)).nome)))
            .take(MAX_VISIVEIS)
            .map(|r| (r.1, r.2))
            .collect()
    };
    s.sel = 0;
    s.topo = 0;
    animar();
}

fn visiveis() -> usize {
    st().resultados.len().min(MAX_VISIVEIS)
}

fn altura_alvo() -> f32 {
    match visiveis() {
        0 => ALT_BUSCA,
        n => TOPO_LISTA + n as f32 * ALT_ITEM + FUNDO_LISTA,
    }
}

fn px(dip: f32) -> i32 {
    (dip * st().escala).ceil() as i32
}

/// Linha sob o y do mouse (pixels do cliente).
fn linha_em(y: i32) -> Option<usize> {
    let y = y as f32 / st().escala - TOPO_LISTA;
    if y < 0.0 {
        return None;
    }
    let i = (y / ALT_ITEM) as usize;
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
    animar();
}

// ---------- mostrar / esconder / abrir ----------

unsafe fn mostrar() {
    atualizar_tema();
    verificar_fixados();
    verificar_apps();
    let s = st();
    let mut p = POINT::default();
    let _ = GetCursorPos(&mut p);
    let mon = MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST);
    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    let _ = GetMonitorInfoW(mon, &mut mi);
    let (mut dx, mut dy) = (96, 96);
    let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
    s.escala = dx.max(96) as f32 / 96.0;

    // Abre já no tamanho final; a animação de abertura é só do conteúdo.
    s.altura = altura_alvo();
    s.sel_y = (s.sel - s.topo) as f32 * ALT_ITEM;
    s.abertura = Some(Instant::now());
    let w = mi.rcWork;
    let largura = px(LARGURA);
    let x = w.left + (w.right - w.left - largura) / 2;
    let y = w.top + (w.bottom - w.top) / 4;
    let _ = SetWindowPos(s.hwnd, Some(HWND_TOPMOST), x, y, largura, px(s.altura), SWP_NOACTIVATE);
    let _ = ShowWindow(s.hwnd, SW_SHOW);
    let _ = SetForegroundWindow(s.hwnd);
    let _ = SetFocus(Some(s.hwnd));
    let mut c = p;
    let _ = ScreenToClient(s.hwnd, &mut c);
    s.mouse = (c.x, c.y);
    acender_caret();
    animar();
}

/// Some e limpa: o próximo Alt+Space abre do zero, com os fixados.
unsafe fn esconder() {
    let s = st();
    if !IsWindowVisible(s.hwnd).as_bool() {
        return;
    }
    let _ = ShowWindow(s.hwnd, SW_HIDE);
    let _ = KillTimer(Some(s.hwnd), TIMER_ANIM);
    let _ = KillTimer(Some(s.hwnd), TIMER_CARET);
    s.animando = false;
    s.texto.clear();
    s.cursor = 0;
    s.tudo_marcado = false;
    atualizar_busca();
    descartar_icones();
}

/// Fechado, só os bitmaps dos fixados ficam (a próxima abertura mostra eles na hora).
/// Os outros são refeitos quando o app aparecer de novo numa busca.
unsafe fn descartar_icones() {
    let s = st();
    let manter: std::collections::HashSet<String> = s.fixados.iter().map(Item::id).collect();
    s.d.bitmaps.retain(|id, _| manter.contains(id));
    s.d.bitmaps.shrink_to_fit();
    if let Ok(heap) = GetProcessHeap() {
        HeapCompact(heap, HEAP_FLAGS(0));
    }
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

// ---------- animação ----------

unsafe fn animar() {
    let s = st();
    if !IsWindowVisible(s.hwnd).as_bool() {
        return;
    }
    if !s.animando {
        s.animando = true;
        s.ultimo_quadro = Instant::now();
        SetTimer(Some(s.hwnd), TIMER_ANIM, 8, None);
    }
    quadro();
}

fn aproximar(atual: &mut f32, alvo: f32, dt: f32, tau: f32) -> bool {
    *atual += (alvo - *atual) * (1.0 - (-dt / tau).exp());
    if (alvo - *atual).abs() < 0.25 {
        *atual = alvo;
    }
    *atual == alvo
}

unsafe fn quadro() {
    let s = st();
    let agora = Instant::now();
    let dt = (agora - s.ultimo_quadro).as_secs_f32().min(0.05);
    s.ultimo_quadro = agora;

    let sel_ok = aproximar(&mut s.sel_y, (s.sel - s.topo) as f32 * ALT_ITEM, dt, TAU_SELECAO);
    let antes = px(s.altura);
    let alt_ok = aproximar(&mut s.altura, altura_alvo(), dt, TAU_ALTURA);
    let abertura_ok = s.abertura.is_none_or(|t| t.elapsed().as_secs_f32() >= DUR_ABERTURA);
    if abertura_ok {
        s.abertura = None;
    }
    if px(s.altura) != antes {
        aplicar_tamanho();
    }
    redesenhar();
    if sel_ok && alt_ok && abertura_ok {
        s.animando = false;
        let _ = KillTimer(Some(s.hwnd), TIMER_ANIM);
    }
}

unsafe fn aplicar_tamanho() {
    let s = st();
    let _ = SetWindowPos(
        s.hwnd,
        None,
        0,
        0,
        px(LARGURA),
        px(s.altura),
        SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
    );
}

unsafe fn redesenhar() {
    let _ = InvalidateRect(Some(st().hwnd), None, false);
}

// ---------- pintura ----------

unsafe fn pintar(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);
    if rc.right > 0 && rc.bottom > 0 && preparar_dib(rc.right, rc.bottom) {
        let d = &st().d;
        if d.rt.BindDC(d.memdc, &rc).is_ok() {
            let dpi = st().escala * 96.0;
            d.rt.SetDpi(dpi, dpi);
            d.rt.BeginDraw();
            desenhar(rc.bottom as f32 / st().escala);
            if d.rt.EndDraw(None, None).is_err() {
                // Alvo perdido: recria; os bitmaps pertencem ao alvo antigo.
                let s = st();
                if let Ok(rt) = criar_alvo(&s.d.fabrica) {
                    if let Ok(p) = rt.CreateSolidColorBrush(&cor(0, 0, 0, 1.0), None) {
                        s.d.rt = rt;
                        s.d.pincel = p;
                        s.d.bitmaps.clear();
                    }
                }
            }
            let _ = BitBlt(hdc, 0, 0, rc.right, rc.bottom, Some(st().d.memdc), 0, 0, SRCCOPY);
        }
    }
    let _ = EndPaint(hwnd, &ps);
}

/// DIB de 32 bits do tamanho da janela (o alfa vai junto no BitBlt e o DWM respeita).
unsafe fn preparar_dib(w: i32, h: i32) -> bool {
    let d = &mut st().d;
    if d.dib_tam.0 >= w && d.dib_tam.1 >= h && !d.dib.is_invalid() {
        return true;
    }
    if !d.dib.is_invalid() {
        let _ = DeleteObject(d.dib.into());
    }
    // Folga na altura para a janela crescer sem realocar a cada quadro.
    let (w, h) = (w, h.max(px(TOPO_LISTA + MAX_VISIVEIS as f32 * ALT_ITEM + FUNDO_LISTA)));
    let bi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = null_mut();
    match CreateDIBSection(Some(d.memdc), &bi, DIB_RGB_COLORS, &mut bits, None, 0) {
        Ok(b) => {
            SelectObject(d.memdc, b.into());
            d.dib = b;
            d.dib_tam = (w, h);
            true
        }
        Err(_) => false,
    }
}

fn ret(l: f32, t: f32, r: f32, b: f32) -> D2D_RECT_F {
    D2D_RECT_F { left: l, top: t, right: r, bottom: b }
}

fn com_alfa(c: D2D1_COLOR_F, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: c.a * a, ..c }
}

fn suavizar(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

unsafe fn desenhar(altura: f32) {
    let s = st();
    let d = &s.d;
    let c = &s.cores;
    let rt = &d.rt;
    let p = &d.pincel;
    let pinta = |c: D2D1_COLOR_F| p.SetColor(&c);

    rt.Clear(Some(&cor(0, 0, 0, 0.0)));
    pinta(c.veu);
    rt.FillRectangle(&ret(0.0, 0.0, LARGURA, altura), p);

    // Abertura: o conteúdo desce 6 DIPs e aparece.
    let t = s.abertura.map_or(1.0, |t| suavizar(t.elapsed().as_secs_f32() / DUR_ABERTURA));
    let alfa = t;
    let dy = (1.0 - t) * -6.0;

    // Campo de busca.
    let texto: Vec<u16> = s.texto.iter().collect::<String>().encode_utf16().collect();
    let largura_campo = LARGURA - X_TEXTO_BUSCA * 2.0;
    if texto.is_empty() {
        pinta(com_alfa(c.dica, alfa));
        let dica: Vec<u16> = "Pesquisar apps e configurações".encode_utf16().collect();
        rt.DrawText(
            &dica,
            &d.fmt_busca,
            &ret(X_TEXTO_BUSCA, dy, X_TEXTO_BUSCA + largura_campo, ALT_BUSCA + dy),
            p,
            D2D1_DRAW_TEXT_OPTIONS_CLIP,
            DWRITE_MEASURING_MODE_NATURAL,
        );
    }
    if let Ok(layout) = d.dw.CreateTextLayout(&texto, &d.fmt_busca, largura_campo, ALT_BUSCA) {
        let pos16: usize = s.texto[..s.cursor].iter().map(|c| c.len_utf16()).sum();
        let (mut cx, mut cy) = (0.0f32, 0.0f32);
        let mut hm = DWRITE_HIT_TEST_METRICS::default();
        let _ = layout.HitTestTextPosition(pos16 as u32, false, &mut cx, &mut cy, &mut hm);
        let alt_linha = if hm.height > 0.0 { hm.height } else { 30.0 };
        let topo_linha = (ALT_BUSCA - alt_linha) / 2.0 + dy;
        if s.tudo_marcado {
            let mut m = DWRITE_TEXT_METRICS::default();
            let _ = layout.GetMetrics(&mut m);
            pinta(com_alfa(c.marca_texto, alfa));
            rt.FillRectangle(
                &ret(X_TEXTO_BUSCA, topo_linha, X_TEXTO_BUSCA + m.widthIncludingTrailingWhitespace, topo_linha + alt_linha),
                p,
            );
        }
        if !texto.is_empty() {
            pinta(com_alfa(c.texto, alfa));
            rt.DrawText(
                &texto,
                &d.fmt_busca,
                &ret(X_TEXTO_BUSCA, dy, X_TEXTO_BUSCA + largura_campo, ALT_BUSCA + dy),
                p,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
        if s.caret_aceso && !s.tudo_marcado {
            pinta(com_alfa(c.texto, alfa));
            let x = X_TEXTO_BUSCA + cx;
            rt.FillRectangle(&ret(x, topo_linha + 3.0, x + 1.5, topo_linha + alt_linha - 3.0), p);
        }
    }

    let n = visiveis();
    if n == 0 {
        return;
    }
    pinta(com_alfa(c.linha, alfa));
    rt.FillRectangle(&ret(0.0, ALT_BUSCA, LARGURA, ALT_BUSCA + 1.0), p);

    rt.PushAxisAlignedClip(&ret(0.0, ALT_BUSCA + 1.0, LARGURA, altura), D2D1_ANTIALIAS_MODE_ALIASED);
    let y0 = TOPO_LISTA + dy;
    pinta(com_alfa(c.selecao, alfa));
    let ys = y0 + s.sel_y;
    rt.FillRoundedRectangle(
        &D2D1_ROUNDED_RECT { rect: ret(8.0, ys + 2.0, LARGURA - 8.0, ys + ALT_ITEM - 2.0), radiusX: 6.0, radiusY: 6.0 },
        p,
    );
    // Marca na cor de destaque do tema, à esquerda da seleção.
    pinta(com_alfa(c.destaque, alfa));
    rt.FillRoundedRectangle(
        &D2D1_ROUNDED_RECT { rect: ret(8.0, ys + 13.0, 11.0, ys + ALT_ITEM - 13.0), radiusX: 1.5, radiusY: 1.5 },
        p,
    );

    for (vi, &r) in s.resultados.iter().skip(s.topo).take(MAX_VISIVEIS).enumerate() {
        let it = item(r);
        let y = y0 + vi as f32 * ALT_ITEM;
        let caixa = ret(X_ICONE, y + (ALT_ITEM - TAM_ICONE) / 2.0, X_ICONE + TAM_ICONE, y + (ALT_ITEM + TAM_ICONE) / 2.0);
        if let Icone::Glifo(g) = it.icone {
            let mut buf = [0u16; 2];
            pinta(com_alfa(c.nome, alfa));
            rt.DrawText(g.encode_utf16(&mut buf), &d.fmt_glifo, &caixa, p, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        } else if let Some(b) = bitmap_do_item(it) {
            rt.DrawBitmap(&b, Some(&caixa), alfa, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
        }
        let nome: Vec<u16> = it.nome.encode_utf16().collect();
        pinta(com_alfa(if s.topo + vi == s.sel { c.texto } else { c.nome }, alfa));
        rt.DrawText(
            &nome,
            &d.fmt_nome,
            &ret(X_NOME, y, LARGURA - 20.0, y + ALT_ITEM),
            p,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
            DWRITE_MEASURING_MODE_NATURAL,
        );
    }
    rt.PopAxisAlignedClip();
}

/// Bitmap do Direct2D (PBGRA) do ícone do item, guardado por ID. Ícone pendente é extraído
/// aqui, na primeira vez que o item aparece.
unsafe fn bitmap_do_item(it: &mut Item) -> Option<ID2D1Bitmap> {
    let id = it.id();
    let d = &mut st().d;
    if let Some(b) = d.bitmaps.get(&id) {
        return Some(b.clone());
    }
    if matches!(it.icone, Icone::Pendente) {
        it.icone = itens::carregar_icone(&it.arquivo);
    }
    let Icone::Imagem(h) = it.icone else { return None };
    let wb = d.wic.CreateBitmapFromHICON(h).ok()?;
    let conv = d.wic.CreateFormatConverter().ok()?;
    conv.Initialize(&wb, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut)
        .ok()?;
    let b = d.rt.CreateBitmapFromWicBitmap(&conv, None).ok()?;
    d.bitmaps.insert(id, b.clone());
    // O bitmap já tem os pixels; o HICON só ocuparia memória em dobro.
    it.soltar_icone();
    Some(b)
}

/// Relê "Todos os aplicativos" numa thread se a última leitura passou do intervalo.
fn verificar_apps() {
    let s = st();
    if s.listando || s.apps_lidos.is_some_and(|t| t.elapsed() < INTERVALO_APPS) {
        return;
    }
    s.listando = true;
    let h = s.hwnd.0 as isize;
    std::thread::spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let apps = Box::new(itens::listar_apps());
        let _ = PostMessageW(Some(HWND(h as _)), WM_APPS, WPARAM(0), LPARAM(Box::into_raw(apps) as isize));
    });
}

/// Troca a lista de apps pela lida na thread. Os bitmaps já feitos continuam no cache por ID.
unsafe fn receber_apps(lp: LPARAM) {
    let apps = Box::from_raw(lp.0 as *mut Vec<(String, String)>);
    let s = st();
    s.listando = false;
    s.apps_lidos = Some(Instant::now());
    s.apps = apps.into_iter().map(|(nome, id)| itens::item_de_app(nome, &id)).collect();
    atualizar_busca();
}

// ---------- fixados e uso ----------

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
