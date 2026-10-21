//! Os dois índices: fixados do Iniciar (Export-StartLayout) e atalhos do sistema (lista fixa).

use std::{fs, os::windows::process::CommandExt, path::PathBuf, process::Command, time::SystemTime};

use windows::{
    core::{HSTRING, PCWSTR},
    Win32::{
        Foundation::SIZE,
        Graphics::Gdi::{CreateBitmap, DeleteObject, HBITMAP},
        Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES,
        System::{Com::CoTaskMemFree, Environment::ExpandEnvironmentStringsW},
        UI::{
            Shell::{
                Common::ITEMIDLIST, IShellItemImageFactory, SHCreateItemFromIDList, SHGetFileInfoW,
                SHParseDisplayName, SHFILEINFOW, SHGFI_DISPLAYNAME, SHGFI_PIDL, SIIGBF_ICONONLY,
            },
            WindowsAndMessaging::{CreateIconIndirect, DestroyIcon, HICON, ICONINFO},
        },
    },
};

use crate::busca::Chave;

/// Tamanho em que os ícones dos apps são extraídos; o desenho reduz para o tamanho da linha.
pub const TAM_EXTRACAO: i32 = 64;

pub enum Icone {
    /// Ícone do próprio app (fixados).
    Imagem(HICON),
    /// Glifo da Segoe Fluent Icons (atalhos do sistema): um traço só para a lista toda.
    Glifo(char),
    Nenhum,
}

pub struct Item {
    pub nome: String,
    /// Caminho, URI (`ms-settings:`) ou `shell:AppsFolder\<AUMID>`, passado ao ShellExecute.
    pub arquivo: String,
    pub args: String,
    pub chaves: Vec<Chave>,
    pub icone: Icone,
}

impl Item {
    pub fn id(&self) -> String {
        format!("{}|{}", self.arquivo, self.args)
    }
}

impl Drop for Item {
    fn drop(&mut self) {
        if let Icone::Imagem(h) = self.icone {
            unsafe { DestroyIcon(h).ok() };
        }
    }
}

/// (nome, apelidos separados por vírgula, arquivo, argumentos, glifo da Segoe Fluent Icons)
const SISTEMA: &[(&str, &str, &str, &str, char)] = &[
    ("Configurações", "settings,config,ajustes", "ms-settings:", "", '\u{E713}'),
    ("Windows Update", "update,atualizacoes", "ms-settings:windowsupdate", "", '\u{E895}'),
    ("Aplicativos instalados", "apps,desinstalar,uninstall", "ms-settings:appsfeatures", "", '\u{E71D}'),
    ("Aplicativos de inicialização", "startup,inicializar", "ms-settings:startupapps", "", '\u{E768}'),
    ("Tela", "display,monitor,resolucao", "ms-settings:display", "", '\u{E7F4}'),
    ("Bluetooth e dispositivos", "bluetooth", "ms-settings:bluetooth", "", '\u{E702}'),
    ("Som (configurações)", "sound,audio", "ms-settings:sound", "", '\u{E767}'),
    ("Rede e Internet", "network,wifi,rede", "ms-settings:network", "", '\u{E774}'),
    ("Segurança do Windows", "defender,antivirus", "windowsdefender:", "", '\u{EA18}'),
    ("Editor do Registro", "regedit,registry,registro", "regedit.exe", "", '\u{EC7A}'),
    ("Gerenciador de Tarefas", "taskmgr,task manager", "taskmgr.exe", "", '\u{E9D9}'),
    ("Painel de Controle", "control panel,control", "control.exe", "", '\u{E770}'),
    ("Prompt de Comando", "cmd", "cmd.exe", "", '\u{E756}'),
    ("Windows PowerShell", "powershell", "powershell.exe", "", '\u{E756}'),
    ("PowerShell 7", "pwsh", "pwsh.exe", "", '\u{E756}'),
    ("Terminal", "wt,windows terminal", "wt.exe", "", '\u{E756}'),
    ("Explorador de Arquivos", "explorer", "explorer.exe", "", '\u{E8B7}'),
    ("Serviços", "services", "services.msc", "", '\u{E912}'),
    ("Gerenciador de Dispositivos", "devmgmt,device manager", "devmgmt.msc", "", '\u{E950}'),
    ("Gerenciamento de Disco", "diskmgmt,disk", "diskmgmt.msc", "", '\u{EDA2}'),
    ("Gerenciamento do Computador", "compmgmt", "compmgmt.msc", "", '\u{E7F8}'),
    ("Visualizador de Eventos", "eventvwr,event viewer,logs", "eventvwr.msc", "", '\u{E81C}'),
    ("Agendador de Tarefas", "taskschd,task scheduler", "taskschd.msc", "", '\u{E787}'),
    ("Editor de Política de Grupo", "gpedit,group policy", "gpedit.msc", "", '\u{E7EF}'),
    ("Usuários e Grupos Locais", "lusrmgr,users", "lusrmgr.msc", "", '\u{E716}'),
    ("Firewall com Segurança Avançada", "wf,firewall", "wf.msc", "", '\u{EA18}'),
    ("Monitor de Recursos", "resmon", "resmon.exe", "", '\u{E9F5}'),
    ("Monitor de Desempenho", "perfmon", "perfmon.exe", "", '\u{E9D2}'),
    ("Configuração do Sistema", "msconfig", "msconfig.exe", "", '\u{E90F}'),
    ("Informações do Sistema", "msinfo32,system information", "msinfo32.exe", "", '\u{E946}'),
    ("Programas e Recursos", "appwiz", "control.exe", "appwiz.cpl", '\u{ECAA}'),
    ("Conexões de Rede", "ncpa,adaptadores", "control.exe", "ncpa.cpl", '\u{E839}'),
    ("Propriedades do Sistema", "sysdm,system properties", "control.exe", "sysdm.cpl", '\u{E770}'),
    ("Variáveis de Ambiente", "env,environment,path", "rundll32.exe", "sysdm.cpl,EditEnvironmentVariables", '\u{E943}'),
    ("Opções de Energia", "powercfg,energia", "control.exe", "powercfg.cpl", '\u{E7E8}'),
    ("Som", "mmsys,playback,gravacao", "control.exe", "mmsys.cpl", '\u{E767}'),
    ("Mouse", "main.cpl", "control.exe", "main.cpl", '\u{E962}'),
    ("Data e Hora", "timedate,relogio", "control.exe", "timedate.cpl", '\u{E823}'),
    ("Opções da Internet", "inetcpl,proxy", "control.exe", "inetcpl.cpl", '\u{E774}'),
    ("Gerenciador de Credenciais", "credential manager,senhas", "control.exe", "/name Microsoft.CredentialManager", '\u{E8D7}'),
    ("Recursos do Windows", "optionalfeatures,features", "optionalfeatures.exe", "", '\u{EA86}'),
    ("Limpeza de Disco", "cleanmgr", "cleanmgr.exe", "", '\u{E74D}'),
    ("Otimizar Unidades", "dfrgui,desfragmentar", "dfrgui.exe", "", '\u{EDA2}'),
    ("Conexão de Área de Trabalho Remota", "mstsc,rdp,remote desktop", "mstsc.exe", "", '\u{E703}'),
    ("Bloco de Notas", "notepad", "notepad.exe", "", '\u{E70F}'),
    ("Mapa de Caracteres", "charmap", "charmap.exe", "", '\u{E8D2}'),
    ("Lixeira", "recycle bin", "shell:RecycleBinFolder", "", '\u{E74D}'),
    ("Bloquear", "lock", "rundll32.exe", "user32.dll,LockWorkStation", '\u{E72E}'),
];

pub fn dir_dados() -> PathBuf {
    let d = PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default()).join("lancador");
    let _ = fs::create_dir_all(&d);
    d
}

fn cache_fixados() -> PathBuf {
    dir_dados().join("fixados.json")
}

fn start2() -> PathBuf {
    PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default())
        .join(r"Packages\Microsoft.Windows.StartMenuExperienceHost_cw5n1h2txyewy\LocalState\start2.bin")
}

fn mtime(p: &PathBuf) -> Option<SystemTime> {
    fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// O layout do Iniciar mudou desde o último export (ou nunca foi exportado)?
pub fn fixados_desatualizados() -> bool {
    match (mtime(&start2()), mtime(&cache_fixados())) {
        (_, None) => true,
        (Some(s), Some(c)) => s > c,
        (None, Some(_)) => false,
    }
}

/// Roda o Export-StartLayout (bloqueia ~1 s: chamar fora da thread da janela).
pub fn exportar_fixados() -> bool {
    let tmp = dir_dados().join("fixados.tmp.json");
    let ok = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command"])
        .arg(format!("Export-StartLayout -Path '{}'", tmp.display()))
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    ok && fs::rename(&tmp, cache_fixados()).is_ok()
}

pub fn carregar_fixados() -> Vec<Item> {
    let Ok(texto) = fs::read_to_string(cache_fixados()) else { return Vec::new() };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(texto.trim_start_matches('\u{feff}')) else {
        return Vec::new();
    };
    let lista = json["pinnedList"].as_array().cloned().unwrap_or_default();
    lista
        .iter()
        .filter_map(|e| {
            let arquivo = if let Some(l) = e["desktopAppLink"].as_str() {
                expandir(l)
            } else if let Some(id) = e["packagedAppId"].as_str().or(e["desktopAppId"].as_str()) {
                format!(r"shell:AppsFolder\{id}")
            } else {
                return None;
            };
            let (nome, icone) = info_shell(&arquivo, TAM_EXTRACAO)?;
            Some(Item {
                chaves: vec![Chave::new(&nome)],
                nome,
                arquivo,
                args: String::new(),
                icone: icone.map_or(Icone::Nenhum, Icone::Imagem),
            })
        })
        .collect()
}

pub fn carregar_sistema() -> Vec<Item> {
    SISTEMA
        .iter()
        .filter_map(|&(nome, apelidos, arquivo, args, glifo)| {
            let caminho = if arquivo.ends_with(".exe") || arquivo.ends_with(".msc") {
                achar(arquivo)?
            } else {
                arquivo.to_string()
            };
            let mut chaves = vec![Chave::new(nome)];
            chaves.extend(apelidos.split(',').map(Chave::new));
            Some(Item {
                nome: nome.into(),
                arquivo: caminho,
                args: args.into(),
                chaves,
                icone: Icone::Glifo(glifo),
            })
        })
        .collect()
}

/// Procura um exe/msc no System32, no Windows e no PATH.
fn achar(nome: &str) -> Option<String> {
    let windir = std::env::var("windir").unwrap_or_else(|_| r"C:\Windows".into());
    let mut dirs = vec![PathBuf::from(&windir).join("System32"), PathBuf::from(&windir)];
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    dirs.into_iter().map(|d| d.join(nome)).find(|p| p.exists()).map(|p| p.to_string_lossy().into_owned())
}

fn expandir(s: &str) -> String {
    let w = HSTRING::from(s);
    let mut buf = vec![0u16; 1024];
    let n = unsafe { ExpandEnvironmentStringsW(&w, Some(&mut buf)) } as usize;
    if n == 0 || n > buf.len() {
        return s.to_string();
    }
    String::from_utf16_lossy(&buf[..n - 1])
}

/// Nome de exibição e ícone pelo shell (serve para .lnk, arquivo e shell:AppsFolder\...).
/// O ícone vem do IShellItemImageFactory: sai no tamanho pedido e sem a seta de atalho
/// que o SHGetFileInfo põe nos .lnk.
fn info_shell(caminho: &str, tam: i32) -> Option<(String, Option<HICON>)> {
    unsafe {
        let w = HSTRING::from(caminho);
        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        SHParseDisplayName(&w, None, &mut pidl, 0, None).ok()?;
        let mut fi = SHFILEINFOW::default();
        let r = SHGetFileInfoW(
            PCWSTR(pidl as *const u16),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut fi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_PIDL | SHGFI_DISPLAYNAME,
        );
        let icone = SHCreateItemFromIDList::<IShellItemImageFactory>(pidl)
            .ok()
            .and_then(|f| f.GetImage(SIZE { cx: tam, cy: tam }, SIIGBF_ICONONLY).ok())
            .and_then(|bmp| bitmap_para_icone(bmp, tam));
        CoTaskMemFree(Some(pidl as *const _));
        if r == 0 {
            return None;
        }
        let len = fi.szDisplayName.iter().position(|&c| c == 0).unwrap_or(fi.szDisplayName.len());
        Some((String::from_utf16_lossy(&fi.szDisplayName[..len]), icone))
    }
}

/// HBITMAP 32 bpp com alfa → HICON (o WIC lê o alfa do HICON).
unsafe fn bitmap_para_icone(cor: HBITMAP, tam: i32) -> Option<HICON> {
    let mascara = CreateBitmap(tam, tam, 1, 1, None);
    let ii = ICONINFO { fIcon: true.into(), hbmMask: mascara, hbmColor: cor, ..Default::default() };
    let icone = CreateIconIndirect(&ii).ok();
    let _ = DeleteObject(mascara.into());
    let _ = DeleteObject(cor.into());
    icone
}
