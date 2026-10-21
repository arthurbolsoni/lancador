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
                Common::ITEMIDLIST, IShellItemImageFactory, SHCreateItemFromIDList, SHDefExtractIconW, SIIGBF_ICONONLY, SHGetFileInfoW, SHParseDisplayName,
                SHFILEINFOW, SHGFI_DISPLAYNAME, SHGFI_PIDL,
            },
            WindowsAndMessaging::{CreateIconIndirect, DestroyIcon, HICON, ICONINFO},
        },
    },
};

use crate::busca::Chave;

pub struct Item {
    pub nome: String,
    pub detalhe: String,
    /// Caminho, URI (`ms-settings:`) ou `shell:AppsFolder\<AUMID>`, passado ao ShellExecute.
    pub arquivo: String,
    pub args: String,
    pub chaves: Vec<Chave>,
    pub icone: HICON,
}

impl Item {
    pub fn id(&self) -> String {
        format!("{}|{}", self.arquivo, self.args)
    }
}

impl Drop for Item {
    fn drop(&mut self) {
        if !self.icone.is_invalid() {
            unsafe { DestroyIcon(self.icone).ok() };
        }
    }
}

/// (nome, apelidos separados por vírgula, arquivo, argumentos, ícone)
/// Ícone vazio = o do próprio arquivo; "arquivo,N" = N-ésimo ícone do arquivo.
const SISTEMA: &[(&str, &str, &str, &str, &str)] = &[
    ("Configurações", "settings,config,ajustes", "ms-settings:", "", CONFIG),
    ("Windows Update", "update,atualizacoes", "ms-settings:windowsupdate", "", CONFIG),
    ("Aplicativos instalados", "apps,desinstalar,uninstall", "ms-settings:appsfeatures", "", CONFIG),
    ("Aplicativos de inicialização", "startup,inicializar", "ms-settings:startupapps", "", CONFIG),
    ("Tela", "display,monitor,resolucao", "ms-settings:display", "", CONFIG),
    ("Bluetooth e dispositivos", "bluetooth", "ms-settings:bluetooth", "", CONFIG),
    ("Som (configurações)", "sound,audio", "ms-settings:sound", "", CONFIG),
    ("Rede e Internet", "network,wifi,rede", "ms-settings:network", "", CONFIG),
    ("Segurança do Windows", "defender,antivirus", "windowsdefender:", "", r"shell:AppsFolder\Microsoft.SecHealthUI_8wekyb3d8bbwe!SecHealthUI"),
    ("Editor do Registro", "regedit,registry,registro", "regedit.exe", "", ""),
    ("Gerenciador de Tarefas", "taskmgr,task manager", "taskmgr.exe", "", ""),
    ("Painel de Controle", "control panel,control", "control.exe", "", ""),
    ("Prompt de Comando", "cmd", "cmd.exe", "", ""),
    ("Windows PowerShell", "powershell", "powershell.exe", "", ""),
    ("PowerShell 7", "pwsh", "pwsh.exe", "", ""),
    ("Terminal", "wt,windows terminal", "wt.exe", "", ""),
    ("Explorador de Arquivos", "explorer", "explorer.exe", "", ""),
    ("Serviços", "services", "services.msc", "", ""),
    ("Gerenciador de Dispositivos", "devmgmt,device manager", "devmgmt.msc", "", ""),
    ("Gerenciamento de Disco", "diskmgmt,disk", "diskmgmt.msc", "", ""),
    ("Gerenciamento do Computador", "compmgmt", "compmgmt.msc", "", ""),
    ("Visualizador de Eventos", "eventvwr,event viewer,logs", "eventvwr.msc", "", ""),
    ("Agendador de Tarefas", "taskschd,task scheduler", "taskschd.msc", "", ""),
    ("Editor de Política de Grupo", "gpedit,group policy", "gpedit.msc", "", ""),
    ("Usuários e Grupos Locais", "lusrmgr,users", "lusrmgr.msc", "", ""),
    ("Firewall com Segurança Avançada", "wf,firewall", "wf.msc", "", ""),
    ("Monitor de Recursos", "resmon", "resmon.exe", "", ""),
    ("Monitor de Desempenho", "perfmon", "perfmon.exe", "", ""),
    ("Configuração do Sistema", "msconfig", "msconfig.exe", "", ""),
    ("Informações do Sistema", "msinfo32,system information", "msinfo32.exe", "", ""),
    ("Programas e Recursos", "appwiz", "control.exe", "appwiz.cpl", r"%windir%\System32\appwiz.cpl,0"),
    ("Conexões de Rede", "ncpa,adaptadores", "control.exe", "ncpa.cpl", r"%windir%\System32\ncpa.cpl,0"),
    ("Propriedades do Sistema", "sysdm,system properties", "control.exe", "sysdm.cpl", r"%windir%\System32\sysdm.cpl,0"),
    ("Variáveis de Ambiente", "env,environment,path", "rundll32.exe", "sysdm.cpl,EditEnvironmentVariables", r"%windir%\System32\sysdm.cpl,0"),
    ("Opções de Energia", "powercfg,energia", "control.exe", "powercfg.cpl", r"%windir%\System32\powercfg.cpl,0"),
    ("Som", "mmsys,playback,gravacao", "control.exe", "mmsys.cpl", r"%windir%\System32\mmsys.cpl,0"),
    ("Mouse", "main.cpl", "control.exe", "main.cpl", r"%windir%\System32\main.cpl,0"),
    ("Data e Hora", "timedate,relogio", "control.exe", "timedate.cpl", r"%windir%\System32\timedate.cpl,0"),
    ("Opções da Internet", "inetcpl,proxy", "control.exe", "inetcpl.cpl", r"%windir%\System32\inetcpl.cpl,0"),
    ("Gerenciador de Credenciais", "credential manager,senhas", "control.exe", "/name Microsoft.CredentialManager", r"%windir%\System32\shell32.dll,44"),
    ("Recursos do Windows", "optionalfeatures,features", "optionalfeatures.exe", "", ""),
    ("Limpeza de Disco", "cleanmgr", "cleanmgr.exe", "", ""),
    ("Otimizar Unidades", "dfrgui,desfragmentar", "dfrgui.exe", "", ""),
    ("Conexão de Área de Trabalho Remota", "mstsc,rdp,remote desktop", "mstsc.exe", "", ""),
    ("Bloco de Notas", "notepad", "notepad.exe", "", ""),
    ("Mapa de Caracteres", "charmap", "charmap.exe", "", ""),
    ("Lixeira", "recycle bin", "shell:RecycleBinFolder", "", ""),
    ("Bloquear", "lock", "rundll32.exe", "user32.dll,LockWorkStation", r"%windir%\System32\imageres.dll,54"),
];

const CONFIG: &str = r"shell:AppsFolder\windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel";

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

pub fn carregar_fixados(tam: i32) -> Vec<Item> {
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
            let (nome, icone) = info_shell(&arquivo, tam)?;
            Some(Item {
                chaves: vec![Chave::new(&nome)],
                nome,
                detalhe: "Fixado no Iniciar".into(),
                arquivo,
                args: String::new(),
                icone,
            })
        })
        .collect()
}

pub fn carregar_sistema(tam: i32) -> Vec<Item> {
    SISTEMA
        .iter()
        .filter_map(|&(nome, apelidos, arquivo, args, icone)| {
            let caminho = if arquivo.ends_with(".exe") || arquivo.ends_with(".msc") {
                achar(arquivo)?
            } else {
                arquivo.to_string()
            };
            let icone = if icone.is_empty() {
                info_shell(&caminho, tam).map(|(_, i)| i)
            } else if let Some((f, n)) = icone.rsplit_once(',').filter(|(_, n)| n.parse::<i32>().is_ok()) {
                icone_de_arquivo(&expandir(f), n.parse().unwrap(), tam)
            } else {
                info_shell(icone, tam).map(|(_, i)| i)
            };
            let mut chaves = vec![Chave::new(nome)];
            chaves.extend(apelidos.split(',').map(Chave::new));
            let detalhe = if args.is_empty() { arquivo.to_string() } else { format!("{arquivo} {args}") };
            Some(Item {
                nome: nome.into(),
                detalhe,
                arquivo: caminho,
                args: args.into(),
                chaves,
                icone: icone.unwrap_or_default(),
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
fn info_shell(caminho: &str, tam: i32) -> Option<(String, HICON)> {
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
        Some((String::from_utf16_lossy(&fi.szDisplayName[..len]), icone.unwrap_or_default()))
    }
}

/// HBITMAP 32 bpp com alfa → HICON (o DrawIconEx respeita o alfa da cor).
unsafe fn bitmap_para_icone(cor: HBITMAP, tam: i32) -> Option<HICON> {
    let mascara = CreateBitmap(tam, tam, 1, 1, None);
    let ii = ICONINFO { fIcon: true.into(), hbmMask: mascara, hbmColor: cor, ..Default::default() };
    let icone = CreateIconIndirect(&ii).ok();
    let _ = DeleteObject(mascara.into());
    let _ = DeleteObject(cor.into());
    icone
}

fn icone_de_arquivo(arquivo: &str, indice: i32, tam: i32) -> Option<HICON> {
    let w = HSTRING::from(arquivo);
    let mut h = HICON::default();
    unsafe { SHDefExtractIconW(&w, indice, 0, Some(&mut h), None, tam as u32 | (16 << 16)) }.ok().ok()?;
    Some(h)
}
