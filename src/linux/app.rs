//! Janela única, criada no atalho e destruída ao fechar: superfície layer-shell (camada
//! Overlay) no alto da tela, com o blur do KWin atrás (ext-background-effect) e o conteúdo
//! desenhado em software. Mesmo comportamento e medidas da versão do Windows (`src/janela.rs`).

use std::{
    collections::{HashMap, HashSet},
    io::Read,
    os::{fd::AsRawFd, unix::net::UnixListener},
    path::Path,
    time::{Duration, Instant},
};

use resvg::tiny_skia::Pixmap;
use smithay_client_toolkit::{
    activation::{ActivationHandler, ActivationState, RequestData},
    background_effect::{BackgroundEffectHandler, BackgroundEffectState},
    compositor::{CompositorHandler, CompositorState, FrameCallbackData, Region},
    data_device_manager::{
        data_device::{DataDevice, DataDeviceHandler},
        data_offer::{DataOfferHandler, DragOffer},
        DataDeviceManagerState,
    },
    delegate_dispatch2, delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{
            channel::{self, Sender},
            generic::Generic,
            timer::{TimeoutAction, Timer},
            EventLoop, Interest, LoopHandle, Mode, PostAction, RegistrationToken,
        },
        calloop_wayland_source::WaylandSource,
        client::{
            globals::registry_queue_init,
            protocol::{wl_data_device::WlDataDevice, wl_data_device_manager::DndAction, wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
            Connection, QueueHandle,
        },
        protocols::ext::background_effect::v1::client::ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler, BTN_LEFT},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};

use super::{
    desenho::{self, Cor, Fonte},
    icones::{self, Icones},
    itens::{self, Item},
    kde::{self, Kde},
};
use crate::{busca, tema::Tema};

/// Releitura dos apps no máximo uma vez por este intervalo, ao abrir.
const INTERVALO_APPS: Duration = Duration::from_secs(60);

// Origem de um resultado; o maior ganha no empate de pontuação.
const SISTEMA: u8 = 0;
const APP: u8 = 1;
const MAX_VISIVEIS: usize = 8;

// Medidas em DIPs (as mesmas do Windows).
const LARGURA: f32 = 680.0;
const ALT_BUSCA: f32 = 60.0;
const ALT_ITEM: f32 = 44.0;
const TOPO_LISTA: f32 = ALT_BUSCA + 6.0;
const FUNDO_LISTA: f32 = 8.0;
const ALT_MAX: f32 = TOPO_LISTA + MAX_VISIVEIS as f32 * ALT_ITEM + FUNDO_LISTA;
const X_TEXTO_BUSCA: f32 = 22.0;
const X_ICONE: f32 = 22.0;
const TAM_ICONE: f32 = 26.0;
const X_NOME: f32 = 62.0;
const RAIO: f32 = 10.0;
const PX_BUSCA: f32 = 22.0;
const PX_NOME: f32 = 14.5;

// Constantes de tempo das animações (s): exponencial, sem fim abrupto.
const TAU_SELECAO: f32 = 0.045;
const TAU_ALTURA: f32 = 0.06;
const DUR_ABERTURA: f32 = 0.16;
const PISCA_CARET: Duration = Duration::from_millis(530);

fn mistura(a: u32, b: u32, f: f32) -> u32 {
    let canal = |s: u32| {
        let (x, y) = (((a >> s) & 0xFF) as f32, ((b >> s) & 0xFF) as f32);
        ((x + (y - x) * f).round() as u32) << s
    };
    canal(16) | canal(8) | canal(0)
}

/// Cores do lançador derivadas do tema.
struct Cores {
    /// Véu sobre o blur: o fundo do tema, com um pouco do blur passando.
    veu: Cor,
    borda: Cor,
    texto: Cor,
    dica: Cor,
    nome: Cor,
    selecao: Cor,
    linha: Cor,
    marca_texto: Cor,
    destaque: Cor,
}

fn cores(t: &Tema) -> Cores {
    Cores {
        veu: Cor::hex(t.fundo, 0.84),
        borda: Cor::hex(t.mudo, 1.0),
        texto: Cor::hex(t.texto_forte, 1.0),
        dica: Cor::hex(mistura(t.texto, t.fundo, 0.45), 1.0),
        nome: Cor::hex(t.texto, 1.0),
        selecao: Cor::hex(t.selecao, 1.0),
        linha: Cor::hex(t.selecao, 1.0),
        marca_texto: Cor::hex(t.destaque, 0.38),
        destaque: Cor::hex(t.destaque, 1.0),
    }
}

fn aproximar(atual: &mut f32, alvo: f32, dt: f32, tau: f32) -> bool {
    *atual += (alvo - *atual) * (1.0 - (-dt / tau).exp());
    if (alvo - *atual).abs() < 0.25 {
        *atual = alvo;
    }
    *atual == alvo
}

fn suavizar(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
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

struct Lancador {
    conn: Connection,
    qh: QueueHandle<Lancador>,
    laco: LoopHandle<'static, Lancador>,
    registro: RegistryState,
    assentos: SeatState,
    saidas: OutputState,
    compositor: CompositorState,
    camadas: LayerShell,
    shm: Shm,
    pool: SlotPool,
    ativacao: Option<ActivationState>,
    efeito: BackgroundEffectState,
    area: Option<DataDeviceManagerState>,
    dispositivo: Option<DataDevice>,
    assento: Option<wl_seat::WlSeat>,
    teclado: Option<wl_keyboard::WlKeyboard>,
    ponteiro: Option<wl_pointer::WlPointer>,
    serial: u32,
    mods: Modifiers,

    camada: Option<LayerSurface>,
    blur: Option<ExtBackgroundEffectSurfaceV1>,
    configurado: bool,
    com_foco: bool,
    escala: i32,

    apps: Vec<Item>,
    sistema: Vec<Item>,
    apps_lidos: Option<Instant>,
    listando: bool,
    envio_apps: Sender<Vec<Item>>,
    /// (origem, índice na lista da origem)
    resultados: Vec<(u8, usize)>,
    sel: usize,
    topo: usize,
    texto: Vec<char>,
    cursor: usize,
    tudo_marcado: bool,
    caret_aceso: bool,
    uso: HashMap<String, u32>,
    kde: Kde,
    cores: Cores,
    icones: Icones,
    imagens: HashMap<(String, u32), Option<Pixmap>>,
    familia: String,
    fonte_busca: Option<Fonte>,
    fonte_nome: Option<Fonte>,
    tela: Pixmap,

    // animação
    sel_y: f32,
    altura: f32,
    abertura: Option<Instant>,
    animando: bool,
    quadro_pedido: bool,
    ultimo_quadro: Instant,
    caret: Option<RegistrationToken>,
    mouse: Option<(f64, f64)>,
    roda: f64,
    /// Resultado esperando o token de ativação para abrir.
    abrindo: Option<(u8, usize)>,
    sair: bool,
}

pub fn rodar(ouvinte: UnixListener, abrir_agora: bool) {
    let conn = Connection::connect_to_env().expect("sem conexão com o Wayland");
    let (globais, fila) = registry_queue_init::<Lancador>(&conn).expect("registro do Wayland");
    let qh = fila.handle();
    let mut laco: EventLoop<'static, Lancador> = EventLoop::try_new().expect("laço de eventos");
    WaylandSource::new(conn.clone(), fila).insert(laco.handle()).expect("fonte do Wayland");

    let compositor = CompositorState::bind(&globais, &qh).expect("wl_compositor");
    let camadas = LayerShell::bind(&globais, &qh).expect("o compositor não tem layer-shell");
    let shm = Shm::bind(&globais, &qh).expect("wl_shm");
    let pool = SlotPool::new((LARGURA * ALT_MAX * 4.0) as usize, &shm).expect("memória compartilhada");
    let (envio_apps, recebe_apps) = channel::channel::<Vec<Item>>();
    laco.handle()
        .insert_source(recebe_apps, |ev, _, st: &mut Lancador| {
            if let channel::Event::Msg(apps) = ev {
                st.receber_apps(apps);
            }
        })
        .expect("canal dos apps");

    let _ = ouvinte.set_nonblocking(true);
    laco.handle()
        .insert_source(Generic::new(ouvinte, Interest::READ, Mode::Level), |_, l, st: &mut Lancador| {
            while let Ok((mut s, _)) = l.accept() {
                let _ = s.set_nonblocking(false);
                let _ = s.set_read_timeout(Some(Duration::from_millis(200)));
                let mut pedido = String::new();
                let _ = s.read_to_string(&mut pedido);
                st.pedido(&pedido);
            }
            Ok(PostAction::Continue)
        })
        .expect("socket");

    let kde = kde::ler();
    let familia = kde.fonte.clone();
    let mut st = Lancador {
        registro: RegistryState::new(&globais),
        assentos: SeatState::new(&globais, &qh),
        saidas: OutputState::new(&globais, &qh),
        ativacao: ActivationState::bind(&globais, &qh).ok(),
        efeito: BackgroundEffectState::new(&globais, &qh),
        area: DataDeviceManagerState::bind(&globais, &qh).ok(),
        dispositivo: None,
        assento: None,
        teclado: None,
        ponteiro: None,
        serial: 0,
        mods: Modifiers::default(),
        conn,
        qh,
        laco: laco.handle(),
        compositor,
        camadas,
        shm,
        pool,
        camada: None,
        blur: None,
        configurado: false,
        com_foco: false,
        escala: 1,
        apps: Vec::new(),
        sistema: itens::carregar_sistema(),
        apps_lidos: None,
        listando: false,
        envio_apps,
        resultados: Vec::new(),
        sel: 0,
        topo: 0,
        texto: Vec::new(),
        cursor: 0,
        tudo_marcado: false,
        caret_aceso: true,
        uso: itens::carregar_uso(),
        cores: cores(&kde.tema),
        icones: Icones::new(&kde.icones),
        kde,
        imagens: HashMap::new(),
        fonte_busca: Fonte::carregar(&format!("{familia}:light")),
        fonte_nome: Fonte::carregar(&familia),
        familia,
        tela: Pixmap::new(LARGURA as u32, ALT_MAX as u32).unwrap(),
        sel_y: 0.0,
        altura: ALT_BUSCA,
        abertura: None,
        animando: false,
        quadro_pedido: false,
        ultimo_quadro: Instant::now(),
        caret: None,
        mouse: None,
        roda: 0.0,
        abrindo: None,
        sair: false,
    };
    st.atualizar_busca();
    st.verificar_apps();
    if abrir_agora {
        st.mostrar();
    }
    while !st.sair {
        if laco.dispatch(None, &mut st).is_err() {
            break;
        }
    }
    let _ = std::fs::remove_file(super::caminho_socket());
}

impl Lancador {
    fn pedido(&mut self, p: &str) {
        let mut linhas = p.split('\n');
        match linhas.next().unwrap_or("") {
            "alternar" => {
                if self.camada.is_some() {
                    self.esconder()
                } else {
                    self.mostrar()
                }
            }
            "sair" => self.sair = true,
            "testar" => {
                let texto = linhas.next().unwrap_or("").to_string();
                let png = linhas.next().unwrap_or("/tmp/lancador.png").to_string();
                self.testar(&texto, &png);
            }
            _ => {}
        }
    }

    // ---------- campo de busca ----------

    fn tecla(&mut self, ev: KeyEvent) {
        let ctrl = self.mods.ctrl;
        let shift = self.mods.shift;
        match ev.keysym {
            Keysym::Down => self.mover(1),
            Keysym::Up => self.mover(-1),
            Keysym::Tab => self.mover(if shift { -1 } else { 1 }),
            Keysym::ISO_Left_Tab => self.mover(-1),
            Keysym::Page_Down => self.mover(MAX_VISIVEIS as i32),
            Keysym::Page_Up => self.mover(-(MAX_VISIVEIS as i32)),
            Keysym::Return | Keysym::KP_Enter => self.abrir(),
            Keysym::Escape => self.esconder(),
            Keysym::BackSpace | Keysym::Delete if self.tudo_marcado => {
                self.texto.clear();
                self.cursor = 0;
                self.texto_mudou();
            }
            Keysym::BackSpace => {
                if self.cursor > 0 {
                    let inicio = if ctrl { inicio_palavra(&self.texto, self.cursor) } else { self.cursor - 1 };
                    self.texto.drain(inicio..self.cursor);
                    self.cursor = inicio;
                    self.texto_mudou();
                }
            }
            Keysym::Delete => {
                if self.cursor < self.texto.len() {
                    self.texto.remove(self.cursor);
                    self.texto_mudou();
                }
            }
            Keysym::Left | Keysym::Right | Keysym::Home | Keysym::End => {
                self.cursor = match ev.keysym {
                    Keysym::Home => 0,
                    Keysym::End => self.texto.len(),
                    Keysym::Left if ctrl => inicio_palavra(&self.texto, self.cursor),
                    Keysym::Left => self.cursor.saturating_sub(1),
                    _ if ctrl => fim_palavra(&self.texto, self.cursor),
                    _ => (self.cursor + 1).min(self.texto.len()),
                };
                self.tudo_marcado = false;
                self.acender_caret();
            }
            Keysym::a | Keysym::A if ctrl => {
                self.tudo_marcado = !self.texto.is_empty();
                self.redesenhar();
            }
            Keysym::v | Keysym::V if ctrl => {
                let colado: Vec<char> = self.colar().lines().next().unwrap_or("").chars().collect();
                self.inserir(&colado);
            }
            _ if !ctrl => {
                if let Some(t) = ev.utf8 {
                    let cs: Vec<char> = t.chars().filter(|c| !c.is_control()).collect();
                    self.inserir(&cs);
                }
            }
            _ => {}
        }
    }

    fn inserir(&mut self, cs: &[char]) {
        if cs.is_empty() {
            return;
        }
        if self.tudo_marcado {
            self.texto.clear();
            self.cursor = 0;
        }
        for (i, &c) in cs.iter().enumerate() {
            self.texto.insert(self.cursor + i, c);
        }
        self.cursor += cs.len();
        self.texto_mudou();
    }

    fn texto_mudou(&mut self) {
        self.tudo_marcado = false;
        self.atualizar_busca();
        self.acender_caret();
    }

    /// Caret aceso logo depois de digitar/mover, e o pisca recomeça.
    fn acender_caret(&mut self) {
        self.caret_aceso = true;
        if let Some(t) = self.caret.take() {
            self.laco.remove(t);
        }
        if self.camada.is_some() {
            self.caret = self
                .laco
                .insert_source(Timer::from_duration(PISCA_CARET), |_, _, st: &mut Lancador| {
                    st.caret_aceso = !st.caret_aceso;
                    st.redesenhar();
                    TimeoutAction::ToDuration(PISCA_CARET)
                })
                .ok();
        }
        self.redesenhar();
    }

    /// Texto da área de transferência (a oferta chega ao lançador quando ele ganha o foco).
    fn colar(&mut self) -> String {
        let Some(oferta) = self.dispositivo.as_ref().and_then(|d| d.data().selection_offer()) else {
            return String::new();
        };
        let tipo = oferta.with_mime_types(|ts| {
            ["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"]
                .iter()
                .find(|t| ts.iter().any(|x| x == *t))
                .map(|t| t.to_string())
        });
        let Some(Ok(mut cano)) = tipo.map(|t| oferta.receive(t)) else { return String::new() };
        let _ = self.conn.flush();
        let fd = cano.as_raw_fd();
        let fim = Instant::now() + Duration::from_millis(300);
        let mut dados = Vec::new();
        let mut buf = [0u8; 4096];
        while dados.len() < 64 * 1024 {
            let resta = fim.saturating_duration_since(Instant::now()).as_millis() as i32;
            let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
            if resta <= 0 || unsafe { libc::poll(&mut pfd, 1, resta) } <= 0 {
                break;
            }
            match cano.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => dados.extend_from_slice(&buf[..n]),
            }
        }
        String::from_utf8_lossy(&dados).into_owned()
    }

    // ---------- resultados ----------

    fn item(&self, r: (u8, usize)) -> &Item {
        match r.0 {
            APP => &self.apps[r.1],
            _ => &self.sistema[r.1],
        }
    }

    fn atualizar_busca(&mut self) {
        let consulta = busca::dobrar(self.texto.iter().collect::<String>().trim());
        let mut v: Vec<(u32, u8, usize)> = Vec::new();
        for (origem, lista) in [(APP, &self.apps), (SISTEMA, &self.sistema)] {
            for (i, it) in lista.iter().enumerate() {
                let usos = self.uso.get(&it.id).copied().unwrap_or(0);
                if consulta.is_empty() {
                    // Campo vazio: os mais usados.
                    if usos > 0 {
                        v.push((usos, origem, i));
                    }
                    continue;
                }
                let p = it.chaves.iter().map(|k| busca::pontuar(&consulta, k)).max().unwrap_or(0);
                if p > 0 {
                    v.push((p + (usos * 15).min(150), origem, i));
                }
            }
        }
        v.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then(b.1.cmp(&a.1))
                .then(self.item((a.1, a.2)).nome.len().cmp(&self.item((b.1, b.2)).nome.len()))
        });
        // Com algum casamento de verdade (>= sigla solta), letras soltas pelo nome viram ruído.
        if !consulta.is_empty() && v.iter().any(|r| r.0 >= 500) {
            v.retain(|r| r.0 >= 500);
        }
        let mut vistos = HashSet::new();
        self.resultados = v
            .into_iter()
            .filter(|r| vistos.insert(busca::dobrar(&self.item((r.1, r.2)).nome)))
            .take(MAX_VISIVEIS)
            .map(|r| (r.1, r.2))
            .collect();
        self.sel = 0;
        self.topo = 0;
        self.animar();
    }

    fn visiveis(&self) -> usize {
        self.resultados.len().min(MAX_VISIVEIS)
    }

    fn altura_alvo(&self) -> f32 {
        match self.visiveis() {
            0 => ALT_BUSCA,
            n => TOPO_LISTA + n as f32 * ALT_ITEM + FUNDO_LISTA,
        }
    }

    /// Linha sob o y do ponteiro (coordenadas da superfície, em DIPs).
    fn linha_em(&self, y: f64) -> Option<usize> {
        let y = y as f32 - TOPO_LISTA;
        if y < 0.0 || y > self.altura - TOPO_LISTA {
            return None;
        }
        let i = (y / ALT_ITEM) as usize;
        (i < self.visiveis()).then(|| self.topo + i)
    }

    fn mover(&mut self, d: i32) {
        let n = self.resultados.len() as i32;
        if n == 0 {
            return;
        }
        self.sel = if d.abs() == 1 {
            (self.sel as i32 + d).rem_euclid(n) as usize
        } else {
            (self.sel as i32 + d).clamp(0, n - 1) as usize
        };
        if self.sel < self.topo {
            self.topo = self.sel;
        } else if self.sel >= self.topo + MAX_VISIVEIS {
            self.topo = self.sel + 1 - MAX_VISIVEIS;
        }
        self.animar();
    }

    // ---------- mostrar / esconder / abrir ----------

    /// Relê cores, ícones e fonte do KDE; troca o que mudou desde a última abertura.
    fn atualizar_kde(&mut self) {
        self.kde = kde::ler();
        self.cores = cores(&self.kde.tema);
        if self.icones.tema() != self.kde.icones {
            self.icones = Icones::new(&self.kde.icones);
            self.imagens.clear();
        }
        if self.familia != self.kde.fonte {
            self.familia = self.kde.fonte.clone();
            self.fonte_busca = Fonte::carregar(&format!("{}:light", self.familia));
            self.fonte_nome = Fonte::carregar(&self.familia);
        }
    }

    /// Margem do alto: um quarto da altura da tela, como no Windows.
    fn margem_topo(&self) -> i32 {
        self.saidas
            .outputs()
            .filter_map(|o| self.saidas.info(&o))
            .filter_map(|i| i.logical_size)
            .map(|(_, h)| h / 4)
            .next()
            .unwrap_or(250)
    }

    fn mostrar(&mut self) {
        if self.camada.is_some() {
            return;
        }
        self.atualizar_kde();
        self.verificar_apps();
        self.escala = self
            .saidas
            .outputs()
            .filter_map(|o| self.saidas.info(&o))
            .map(|i| i.scale_factor)
            .max()
            .unwrap_or(1)
            .max(1);
        self.preparar_tela();
        // Abre já no tamanho final; a animação de abertura é só do conteúdo.
        self.altura = self.altura_alvo();
        self.sel_y = (self.sel - self.topo) as f32 * ALT_ITEM;
        self.abertura = Some(Instant::now());
        let sup = self.compositor.create_surface(&self.qh);
        let camada = self.camadas.create_layer_surface(&self.qh, sup, Layer::Overlay, Some("lancador"), None);
        camada.set_anchor(Anchor::TOP);
        camada.set_size(LARGURA as u32, ALT_MAX as u32);
        camada.set_margin(self.margem_topo(), 0, 0, 0);
        camada.set_keyboard_interactivity(KeyboardInteractivity::OnDemand);
        self.blur = self.efeito.get_background_effect(camada.wl_surface(), &self.qh).ok();
        camada.commit();
        self.camada = Some(camada);
        self.configurado = false;
        self.com_foco = false;
        self.mouse = None;
        self.roda = 0.0;
        self.acender_caret();
    }

    /// Some e limpa: a próxima abertura começa do zero, com os mais usados.
    fn esconder(&mut self) {
        let Some(camada) = self.camada.take() else { return };
        if let Some(b) = self.blur.take() {
            b.destroy();
        }
        drop(camada);
        self.configurado = false;
        self.com_foco = false;
        self.animando = false;
        self.quadro_pedido = false;
        self.abrindo = None;
        if let Some(t) = self.caret.take() {
            self.laco.remove(t);
        }
        self.texto.clear();
        self.cursor = 0;
        self.tudo_marcado = false;
        self.atualizar_busca();
        let _ = self.conn.flush();
    }

    /// Pede o token de ativação (para o app novo ganhar o foco) e abre quando ele chega.
    fn abrir(&mut self) {
        let Some(&r) = self.resultados.get(self.sel) else { return };
        let (Some(at), Some(assento), Some(camada)) = (&self.ativacao, &self.assento, &self.camada) else {
            self.lancar(r, None);
            return;
        };
        self.abrindo = Some(r);
        at.request_token(
            &self.qh,
            RequestData {
                app_id: Some(self.item(r).id.trim_end_matches(".desktop").to_string()),
                seat_and_serial: Some((assento.clone(), self.serial)),
                surface: Some(camada.wl_surface().clone()),
                udata: (),
            },
        );
        // Sem resposta do compositor, abre sem token.
        let _ = self.laco.insert_source(Timer::from_duration(Duration::from_millis(250)), |_, _, st: &mut Lancador| {
            if let Some(r) = st.abrindo.take() {
                st.lancar(r, None);
            }
            TimeoutAction::Drop
        });
    }

    fn lancar(&mut self, r: (u8, usize), token: Option<String>) {
        self.abrindo = None;
        let it = self.item(r);
        let id = it.id.clone();
        let ok = itens::lancar(it, &self.kde.terminal, token.as_deref());
        self.esconder();
        if ok {
            *self.uso.entry(id).or_default() += 1;
            itens::salvar_uso(&self.uso);
            self.atualizar_busca();
        }
    }

    // ---------- animação ----------

    fn animar(&mut self) {
        if self.camada.is_none() {
            return;
        }
        if !self.animando {
            self.animando = true;
            self.ultimo_quadro = Instant::now();
        }
        if !self.quadro_pedido {
            self.quadro();
        }
    }

    fn quadro(&mut self) {
        let agora = Instant::now();
        let dt = (agora - self.ultimo_quadro).as_secs_f32().min(0.05);
        self.ultimo_quadro = agora;
        let alvo_sel = (self.sel - self.topo) as f32 * ALT_ITEM;
        let sel_ok = aproximar(&mut self.sel_y, alvo_sel, dt, TAU_SELECAO);
        let alvo_alt = self.altura_alvo();
        let alt_ok = aproximar(&mut self.altura, alvo_alt, dt, TAU_ALTURA);
        let abertura_ok = self.abertura.is_none_or(|t| t.elapsed().as_secs_f32() >= DUR_ABERTURA);
        if abertura_ok {
            self.abertura = None;
        }
        if sel_ok && alt_ok && abertura_ok {
            self.animando = false;
        }
        self.desenhar();
        self.apresentar();
    }

    fn redesenhar(&mut self) {
        if self.camada.is_none() || (self.animando && self.quadro_pedido) {
            return;
        }
        self.desenhar();
        self.apresentar();
    }

    /// Região arredondada (degraus de 1 px nos cantos) do tamanho visível da janela.
    fn regiao(&self) -> Option<Region> {
        let r = Region::new(&self.compositor).ok()?;
        let (w, h) = (LARGURA as i32, self.altura.ceil() as i32);
        let raio = RAIO as i32;
        for i in 0..raio {
            let dy = raio as f32 - i as f32 - 0.5;
            let dentro = (raio as f32 - (raio as f32 * raio as f32 - dy * dy).max(0.0).sqrt()).round() as i32;
            r.add(dentro, i, w - 2 * dentro, 1);
            r.add(dentro, h - 1 - i, w - 2 * dentro, 1);
        }
        r.add(0, raio, w, (h - 2 * raio).max(0));
        Some(r)
    }

    fn apresentar(&mut self) {
        if !self.configurado {
            return;
        }
        let Some(camada) = &self.camada else { return };
        let (w, h) = (self.tela.width() as i32, self.tela.height() as i32);
        let Ok((buf, tela)) = self.pool.create_buffer(w, h, w * 4, wl_shm::Format::Argb8888) else { return };
        // RGBA pré-multiplicado → ARGB8888 (BGRA na memória).
        for (d, s) in tela.chunks_exact_mut(4).zip(self.tela.data().chunks_exact(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }
        let sup = camada.wl_surface();
        sup.set_buffer_scale(self.escala);
        if let Some(r) = self.regiao() {
            // Clique fora da parte visível passa para a janela de baixo.
            sup.set_input_region(Some(r.wl_region()));
            if let Some(b) = &self.blur {
                b.set_blur_region(Some(r.wl_region()));
            }
        }
        sup.damage_buffer(0, 0, w, h);
        if self.animando {
            sup.frame(&self.qh, FrameCallbackData(sup.clone()));
            self.quadro_pedido = true;
        }
        let _ = buf.attach_to(sup);
        camada.commit();
    }

    // ---------- pintura ----------

    fn preparar_tela(&mut self) {
        let k = self.escala as f32;
        let (w, h) = ((LARGURA * k) as u32, (ALT_MAX * k) as u32);
        if self.tela.width() != w || self.tela.height() != h {
            self.tela = Pixmap::new(w, h).unwrap();
            self.imagens.clear();
        }
    }

    fn desenhar(&mut self) {
        let k = self.escala as f32;
        let tam_icone = (TAM_ICONE * k).round() as u32;
        // Ícones dos itens visíveis (pendentes são desenhados aqui, na primeira vez).
        let visiveis: Vec<(u8, usize)> = self.resultados.iter().skip(self.topo).take(MAX_VISIVEIS).copied().collect();
        for &r in &visiveis {
            let nome = self.item(r).icone.clone();
            let chave = (nome.clone(), tam_icone);
            if !self.imagens.contains_key(&chave) {
                let img = self.icones.achar(&nome, tam_icone).and_then(|p| icones::desenhar(&p, tam_icone));
                self.imagens.insert(chave, img);
            }
        }

        let c = &self.cores;
        let altura = self.altura;
        let t = self.abertura.map_or(1.0, |t| suavizar(t.elapsed().as_secs_f32() / DUR_ABERTURA));
        let alfa = t;
        // Abertura: o conteúdo desce 6 DIPs e aparece.
        let dy = (1.0 - t) * -6.0;
        let tela = &mut self.tela;
        tela.fill(resvg::tiny_skia::Color::TRANSPARENT);
        desenho::arredondado(tela, 0.0, 0.0, LARGURA * k, altura * k, RAIO * k, c.veu);
        desenho::contorno(tela, 0.0, 0.0, LARGURA * k, altura * k, RAIO * k, c.borda.com_alfa(0.9));

        // Campo de busca.
        let largura_campo = LARGURA - X_TEXTO_BUSCA * 2.0;
        if let Some(f) = self.fonte_busca.as_mut() {
            let px = PX_BUSCA * k;
            let (x0, y0, h0) = (X_TEXTO_BUSCA * k, dy * k, ALT_BUSCA * k);
            let max_x = (X_TEXTO_BUSCA + largura_campo) * k;
            if self.texto.is_empty() {
                let dica: Vec<char> = "Pesquisar apps e configurações".chars().collect();
                f.desenhar(tela, &dica, x0, y0, h0, px, max_x, c.dica.com_alfa(alfa), None);
            }
            let pos = f.posicoes(&self.texto, px);
            let alt_linha = f.altura_linha(px);
            let topo_linha = (h0 - alt_linha) / 2.0 + y0;
            if self.tudo_marcado {
                let w = *pos.last().unwrap_or(&0.0);
                desenho::retangulo(tela, x0, topo_linha, w, alt_linha, c.marca_texto.com_alfa(alfa), None);
            }
            if !self.texto.is_empty() {
                f.desenhar(tela, &self.texto, x0, y0, h0, px, f32::MAX, c.texto.com_alfa(alfa), None);
            }
            if self.caret_aceso && !self.tudo_marcado {
                let x = x0 + pos.get(self.cursor).copied().unwrap_or(0.0);
                desenho::retangulo(tela, x.round(), topo_linha + 3.0 * k, 1.5 * k, alt_linha - 6.0 * k, c.texto.com_alfa(alfa), None);
            }
        }

        let n = visiveis.len();
        if n == 0 {
            return;
        }
        desenho::retangulo(tela, 0.0, ALT_BUSCA * k, LARGURA * k, k, c.linha.com_alfa(alfa), None);
        let faixa = ((ALT_BUSCA + 1.0) * k, (altura - 1.0) * k);
        let clip = Some(faixa);
        let y0 = TOPO_LISTA + dy;
        let ys = y0 + self.sel_y;
        // Seleção recortada à lista: desenhada num pixmap à parte só quando sai da faixa.
        if ys + 2.0 >= ALT_BUSCA + 1.0 && ys + ALT_ITEM - 2.0 <= altura {
            desenho::arredondado(tela, 8.0 * k, (ys + 2.0) * k, (LARGURA - 16.0) * k, (ALT_ITEM - 4.0) * k, 6.0 * k, c.selecao.com_alfa(alfa));
            // Marca na cor de destaque do tema, à esquerda da seleção.
            desenho::arredondado(tela, 8.0 * k, (ys + 13.0) * k, 3.0 * k, (ALT_ITEM - 26.0) * k, 1.5 * k, c.destaque.com_alfa(alfa));
        } else {
            desenho::retangulo(tela, 8.0 * k, (ys + 2.0) * k, (LARGURA - 16.0) * k, (ALT_ITEM - 4.0) * k, c.selecao.com_alfa(alfa), clip);
        }

        for (vi, &r) in visiveis.iter().enumerate() {
            let it = match r.0 {
                APP => &self.apps[r.1],
                _ => &self.sistema[r.1],
            };
            let y = y0 + vi as f32 * ALT_ITEM;
            if let Some(Some(img)) = self.imagens.get(&(it.icone.clone(), tam_icone)) {
                let iy = ((y + (ALT_ITEM - TAM_ICONE) / 2.0) * k).round() as i32;
                desenho::icone(tela, img, (X_ICONE * k).round() as i32, iy, alfa, faixa);
            }
            if let Some(f) = self.fonte_nome.as_mut() {
                let nome: Vec<char> = it.nome.chars().collect();
                let cor = if self.topo + vi == self.sel { c.texto } else { c.nome };
                f.desenhar(tela, &nome, X_NOME * k, y * k, ALT_ITEM * k, PX_NOME * k, (LARGURA - 20.0) * k, cor.com_alfa(alfa), clip);
            }
        }
    }

    /// Desenha a janela com a busca `texto` num PNG, sem mostrar nada na tela.
    fn testar(&mut self, texto: &str, png: &str) {
        let visivel = self.camada.is_some();
        let antes = (self.texto.clone(), self.cursor);
        self.atualizar_kde();
        self.preparar_tela();
        self.texto = texto.chars().collect();
        self.cursor = self.texto.len();
        self.atualizar_busca();
        self.altura = self.altura_alvo();
        self.sel_y = 0.0;
        self.abertura = None;
        self.caret_aceso = true;
        self.desenhar();
        if let Err(e) = salvar_png(&self.tela, Path::new(png)) {
            eprintln!("lancador: PNG {png}: {e}");
        }
        if !visivel {
            self.texto = antes.0;
            self.cursor = antes.1;
            self.atualizar_busca();
        }
    }

    // ---------- apps ----------

    /// Relê os apps numa thread se a última leitura passou do intervalo.
    fn verificar_apps(&mut self) {
        if self.listando || self.apps_lidos.is_some_and(|t| t.elapsed() < INTERVALO_APPS) {
            return;
        }
        self.listando = true;
        let envio = self.envio_apps.clone();
        std::thread::spawn(move || {
            let _ = envio.send(itens::listar_apps());
        });
    }

    fn receber_apps(&mut self, apps: Vec<Item>) {
        self.listando = false;
        self.apps_lidos = Some(Instant::now());
        // Mantém a seleção se a lista nova chegou com a janela aberta e o mesmo texto.
        self.apps = apps;
        self.atualizar_busca();
    }
}

fn salvar_png(pm: &Pixmap, caminho: &Path) -> std::io::Result<()> {
    let arq = std::io::BufWriter::new(std::fs::File::create(caminho)?);
    let mut enc = png::Encoder::new(arq, pm.width(), pm.height());
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(std::io::Error::other)?;
    // Pré-multiplicado → direto.
    let dados: Vec<u8> = pm
        .data()
        .chunks_exact(4)
        .flat_map(|p| {
            let a = p[3] as u32;
            if a == 0 {
                [0, 0, 0, 0]
            } else {
                [(p[0] as u32 * 255 / a) as u8, (p[1] as u32 * 255 / a) as u8, (p[2] as u32 * 255 / a) as u8, p[3]]
            }
        })
        .collect();
    w.write_image_data(&dados).map_err(std::io::Error::other)
}

// ---------- handlers do Wayland ----------

impl CompositorHandler for Lancador {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, novo: i32) {
        if novo.max(1) != self.escala {
            self.escala = novo.max(1);
            self.preparar_tela();
            self.redesenhar();
        }
    }

    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.quadro_pedido = false;
        if self.animando {
            self.quadro();
        }
    }

    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}

    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for Lancador {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.saidas
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Lancador {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.esconder();
    }

    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface, _: LayerSurfaceConfigure, _: u32) {
        let primeiro = !self.configurado;
        self.configurado = true;
        if primeiro {
            self.animando = false;
            self.quadro_pedido = false;
            self.animar();
        }
    }
}

impl SeatHandler for Lancador {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.assentos
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, assento: wl_seat::WlSeat, cap: Capability) {
        self.assento = Some(assento.clone());
        if cap == Capability::Keyboard && self.teclado.is_none() {
            self.teclado = self
                .assentos
                .get_keyboard_with_repeat(qh, &assento, None, self.laco.clone(), Box::new(|st: &mut Lancador, _, ev| st.tecla(ev)))
                .ok();
            if let Some(a) = &self.area {
                self.dispositivo = Some(a.get_data_device(qh, &assento));
            }
        }
        if cap == Capability::Pointer && self.ponteiro.is_none() {
            self.ponteiro = self.assentos.get_pointer(qh, &assento).ok();
        }
    }

    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Keyboard {
            if let Some(t) = self.teclado.take() {
                t.release();
            }
        }
        if cap == Capability::Pointer {
            if let Some(p) = self.ponteiro.take() {
                p.release();
            }
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl Lancador {
    fn e_nossa(&self, s: &wl_surface::WlSurface) -> bool {
        self.camada.as_ref().is_some_and(|c| c.wl_surface() == s)
    }
}

impl KeyboardHandler for Lancador {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, s: &wl_surface::WlSurface, serial: u32, _: &[u32], _: &[Keysym]) {
        if self.e_nossa(s) {
            self.com_foco = true;
            self.serial = serial;
        }
    }

    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, s: &wl_surface::WlSurface, _: u32) {
        // Perder o foco fecha, como no Windows.
        if self.e_nossa(s) && self.com_foco && self.abrindo.is_none() {
            self.esconder();
        }
    }

    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, serial: u32, ev: KeyEvent) {
        self.serial = serial;
        if self.camada.is_some() {
            self.tecla(ev);
        }
    }

    fn repeat_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, ev: KeyEvent) {
        if self.camada.is_some() {
            self.tecla(ev);
        }
    }

    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}

    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, m: Modifiers, _: RawModifiers, _: u32) {
        self.mods = m;
    }
}

impl PointerHandler for Lancador {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, eventos: &[PointerEvent]) {
        for ev in eventos {
            if !self.e_nossa(&ev.surface) {
                continue;
            }
            match ev.kind {
                // A janela que aparece sob o ponteiro parado recebe a entrada: não mexer na seleção.
                PointerEventKind::Enter { .. } => self.mouse = Some(ev.position),
                PointerEventKind::Leave { .. } => self.mouse = None,
                PointerEventKind::Motion { .. } => {
                    if self.mouse != Some(ev.position) {
                        self.mouse = Some(ev.position);
                        if let Some(i) = self.linha_em(ev.position.1) {
                            if i != self.sel {
                                self.sel = i;
                                self.animar();
                            }
                        }
                    }
                }
                PointerEventKind::Press { serial, .. } => self.serial = serial,
                PointerEventKind::Release { button, serial, .. } => {
                    self.serial = serial;
                    if button == BTN_LEFT {
                        if let Some(i) = self.linha_em(ev.position.1) {
                            self.sel = i;
                            self.abrir();
                        }
                    }
                }
                PointerEventKind::Axis { vertical, .. } => {
                    self.roda += if vertical.value120 != 0 { vertical.value120 as f64 / 8.0 } else { vertical.absolute };
                    while self.roda >= 15.0 {
                        self.roda -= 15.0;
                        self.mover(1);
                    }
                    while self.roda <= -15.0 {
                        self.roda += 15.0;
                        self.mover(-1);
                    }
                }
            }
        }
    }
}

impl ShmHandler for Lancador {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ActivationHandler for Lancador {
    type RequestUdata = ();

    fn new_token(&mut self, token: String, _: &RequestData<()>) {
        if let Some(r) = self.abrindo.take() {
            self.lancar(r, Some(token));
        }
    }
}

impl BackgroundEffectHandler for Lancador {
    fn background_effect_state(&mut self) -> &mut BackgroundEffectState {
        &mut self.efeito
    }
    fn update_capabilities(&mut self) {}
}

impl DataDeviceHandler for Lancador {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice, _: f64, _: f64, _: &wl_surface::WlSurface) {}
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}
    fn motion(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice, _: f64, _: f64) {}
    fn selection(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}
    fn drop_performed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}
}

impl DataOfferHandler for Lancador {
    fn source_actions(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &mut DragOffer, _: DndAction) {}
    fn selected_action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &mut DragOffer, _: DndAction) {}
}

impl ProvidesRegistryState for Lancador {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registro
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(Lancador);
delegate_dispatch2!(Lancador);
