//! Os índices: apps pelos arquivos .desktop (os do menu e os módulos de configuração do KDE,
//! `kcm_*`, que o menu esconde) e as ações de sessão (bloquear, suspender, sair, reiniciar,
//! desligar). Abrir um item cria um escopo do systemd para o app, como o KDE faz.

use std::{
    collections::{HashMap, HashSet},
    fs,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use crate::busca::{self, Chave};

use super::icones::dirs_dados;

pub struct Item {
    pub nome: String,
    /// ID do .desktop (`org.kde.dolphin.desktop`) ou `sistema:<nome>`; chave do uso.
    pub id: String,
    pub argv: Vec<String>,
    pub dir: Option<String>,
    pub terminal: bool,
    /// Nome no tema de ícones ou caminho absoluto.
    pub icone: String,
    pub chaves: Vec<Chave>,
}

/// (nome, apelidos, comando, ícone). As três últimas pedem confirmação na tela do KDE.
const SESSAO: &[(&str, &str, &[&str], &str)] = &[
    ("Bloquear a tela", "lock,bloquear", &["loginctl", "lock-session"], "system-lock-screen"),
    ("Suspender", "suspend,sleep,dormir", &["systemctl", "suspend"], "system-suspend"),
    ("Encerrar sessão", "logout,sair,log out", &["busctl", "--user", "call", "org.kde.LogoutPrompt", "/LogoutPrompt", "org.kde.LogoutPrompt", "promptLogout"], "system-log-out"),
    ("Reiniciar", "reboot,restart", &["busctl", "--user", "call", "org.kde.LogoutPrompt", "/LogoutPrompt", "org.kde.LogoutPrompt", "promptReboot"], "system-reboot"),
    ("Desligar", "shutdown,poweroff,power off", &["busctl", "--user", "call", "org.kde.LogoutPrompt", "/LogoutPrompt", "org.kde.LogoutPrompt", "promptShutDown"], "system-shutdown"),
];

pub fn dir_dados() -> PathBuf {
    let d = dirs_dados()[0].join("lancador");
    let _ = fs::create_dir_all(&d);
    d
}

pub fn carregar_sistema() -> Vec<Item> {
    SESSAO
        .iter()
        .map(|&(nome, apelidos, argv, icone)| {
            let mut chaves = vec![Chave::new(nome)];
            chaves.extend(apelidos.split(',').map(Chave::new));
            Item {
                nome: nome.into(),
                id: format!("sistema:{nome}"),
                argv: argv.iter().map(|s| s.to_string()).collect(),
                dir: None,
                terminal: false,
                icone: icone.into(),
                chaves,
            }
        })
        .collect()
}

/// Valor de string do .desktop sem os escapes (`\s`, `\n`, `\t`, `\r`, `\\`).
fn desescapar(v: &str) -> String {
    let mut s = String::new();
    let mut it = v.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            s.push(c);
            continue;
        }
        match it.next() {
            Some('s') => s.push(' '),
            Some('n') => s.push('\n'),
            Some('t') => s.push('\t'),
            Some('r') => s.push('\r'),
            Some(o) => s.push(o),
            None => {}
        }
    }
    s
}

/// Grupo `[Desktop Entry]` do arquivo, com as chaves localizadas.
fn ler_entrada(texto: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    let mut dentro = false;
    for linha in texto.lines() {
        let l = linha.trim();
        if l.starts_with('[') {
            dentro = l == "[Desktop Entry]";
            continue;
        }
        if !dentro || l.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            m.entry(k.trim().to_string()).or_insert_with(|| v.trim().to_string());
        }
    }
    m
}

/// Valor localizado (pt_BR, pt, sem idioma).
fn local(m: &HashMap<String, String>, chave: &str) -> Option<String> {
    ["[pt_BR]", "[pt]", ""]
        .iter()
        .find_map(|sufixo| m.get(&format!("{chave}{sufixo}")))
        .map(|v| desescapar(v))
        .filter(|v| !v.is_empty())
}

fn no_path(prog: &str) -> bool {
    if prog.contains('/') {
        return Path::new(prog).exists();
    }
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(prog).is_file()))
        .unwrap_or(false)
}

/// Linha `Exec` em argumentos: aspas e escapes da especificação, códigos de campo fora
/// (`%f %u %F %U` sem arquivo; `%c` = nome; `%k` = o próprio .desktop; `%%` = `%`).
fn dividir_exec(exec: &str, nome: &str, arquivo: &Path, icone: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    let mut atual = String::new();
    let mut tem = false;
    let mut aspas = false;
    let mut it = exec.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '"' => {
                aspas = !aspas;
                tem = true;
            }
            '\\' if aspas => {
                if let Some(n) = it.next() {
                    atual.push(n);
                }
            }
            ' ' | '\t' if !aspas => {
                if tem || !atual.is_empty() {
                    args.push(std::mem::take(&mut atual));
                    tem = false;
                }
            }
            _ => atual.push(c),
        }
    }
    if tem || !atual.is_empty() {
        args.push(atual);
    }
    let mut saida = Vec::new();
    for a in args {
        match a.as_str() {
            "%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => {}
            "%i" => {
                if let Some(i) = icone {
                    saida.push("--icon".into());
                    saida.push(i.into());
                }
            }
            _ => {
                let mut s = String::new();
                let mut cs = a.chars();
                while let Some(c) = cs.next() {
                    if c != '%' {
                        s.push(c);
                        continue;
                    }
                    match cs.next() {
                        Some('%') => s.push('%'),
                        Some('c') => s.push_str(nome),
                        Some('k') => s.push_str(&arquivo.to_string_lossy()),
                        _ => {}
                    }
                }
                saida.push(s);
            }
        }
    }
    saida
}

/// .desktop de todas as pastas de dados, com o ID da especificação (subpasta vira `-`).
/// O primeiro ID visto ganha: ~/.local/share sobrepõe /usr/share.
fn arquivos_desktop() -> Vec<(String, PathBuf)> {
    fn varrer(raiz: &Path, dir: &Path, v: &mut Vec<(String, PathBuf)>) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                varrer(raiz, &p, v);
            } else if p.extension().is_some_and(|x| x == "desktop") {
                if let Ok(rel) = p.strip_prefix(raiz) {
                    v.push((rel.to_string_lossy().replace('/', "-"), p));
                }
            }
        }
    }
    let mut vistos = HashSet::new();
    let mut lista = Vec::new();
    for d in dirs_dados() {
        let raiz = d.join("applications");
        let mut v = Vec::new();
        varrer(&raiz, &raiz, &mut v);
        for (id, p) in v {
            if vistos.insert(id.clone()) {
                lista.push((id, p));
            }
        }
    }
    lista
}

/// Apps do menu e módulos de configuração do KDE. Roda fora da thread da janela.
pub fn listar_apps() -> Vec<Item> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "KDE".into());
    let ambientes: Vec<&str> = desktop.split(':').collect();
    let mut itens = Vec::new();
    for (id, caminho) in arquivos_desktop() {
        let Ok(texto) = fs::read_to_string(&caminho) else { continue };
        let m = ler_entrada(&texto);
        if m.get("Type").map(String::as_str) != Some("Application") || m.get("Hidden").is_some_and(|v| v == "true") {
            continue;
        }
        let kcm = id.starts_with("kcm_");
        if m.get("NoDisplay").is_some_and(|v| v == "true") && !kcm {
            continue;
        }
        let em = |k: &str| m.get(k).map(|v| v.split(';').any(|a| ambientes.contains(&a)));
        if em("OnlyShowIn") == Some(false) || em("NotShowIn") == Some(true) {
            continue;
        }
        if m.get("TryExec").is_some_and(|t| !no_path(&desescapar(t))) {
            continue;
        }
        let (Some(nome), Some(exec)) = (local(&m, "Name"), m.get("Exec")) else { continue };
        let icone = m.get("Icon").map(|i| desescapar(i));
        let argv = dividir_exec(&desescapar(exec), &nome, &caminho, icone.as_deref());
        if argv.is_empty() {
            continue;
        }
        let mut chaves = vec![Chave::new(&nome)];
        let mut extra: Vec<String> = Vec::new();
        // Nome do executável ("dolphin", "alacritty") e o genérico ("Navegador da Web").
        if let Some(exe) = argv.first().and_then(|a| a.rsplit('/').next()) {
            if !["env", "flatpak", "systemsettings", "kcmshell6"].contains(&exe) {
                extra.push(exe.into());
            }
        }
        extra.extend(local(&m, "GenericName"));
        if let Some(k) = local(&m, "Keywords") {
            extra.extend(k.split(';').filter(|s| !s.is_empty()).map(str::to_string));
        }
        let nome_dobrado = busca::dobrar(&nome);
        for e in extra {
            if busca::dobrar(&e) != nome_dobrado {
                chaves.push(Chave::new(&e));
            }
        }
        itens.push(Item {
            nome,
            id,
            argv,
            dir: m.get("Path").map(|p| desescapar(p)).filter(|p| !p.is_empty()),
            terminal: m.get("Terminal").is_some_and(|v| v == "true"),
            icone: icone.unwrap_or_else(|| "application-x-executable".into()),
            chaves,
        });
    }
    itens
}

/// Nome de unidade do systemd aceitável a partir do ID.
fn unidade(id: &str) -> String {
    let base: String = id
        .trim_end_matches(".desktop")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' })
        .collect();
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("app-lancador-{base}-{:x}.scope", n & 0xffff_ffff)
}

/// Abre o item num escopo próprio do systemd (o app não fica no grupo do serviço do lançador).
/// `token` é o de ativação do Wayland, para o app novo receber o foco.
pub fn lancar(it: &Item, terminal: &str, token: Option<&str>) -> bool {
    let mut argv = it.argv.clone();
    if it.terminal {
        argv.splice(0..0, [terminal.to_string(), "-e".to_string()]);
    }
    let mut cmd = Command::new("systemd-run");
    cmd.args(["--user", "--scope", "--quiet", "--collect", "--slice=app.slice"])
        .arg(format!("--unit={}", unidade(&it.id)))
        .arg("--")
        .args(&argv);
    if !no_path("systemd-run") {
        cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..]);
        // Sem systemd: sessão própria, para não morrer junto com o lançador.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    if let Some(d) = &it.dir {
        cmd.current_dir(d);
    }
    if let Some(t) = token {
        cmd.env("XDG_ACTIVATION_TOKEN", t).env("DESKTOP_STARTUP_ID", t);
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    match cmd.spawn() {
        Ok(mut filho) => {
            // Com --scope, o systemd-run vira o próprio app: alguém precisa colher o processo.
            std::thread::spawn(move || {
                let _ = filho.wait();
            });
            true
        }
        Err(e) => {
            eprintln!("lancador: não abriu {}: {e}", it.id);
            false
        }
    }
}

pub fn carregar_uso() -> HashMap<String, u32> {
    fs::read_to_string(dir_dados().join("uso.tsv"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let (id, n) = l.rsplit_once('\t')?;
            Some((id.to_string(), n.parse().ok()?))
        })
        .collect()
}

pub fn salvar_uso(uso: &HashMap<String, u32>) {
    let texto: String = uso.iter().map(|(id, n)| format!("{id}\t{n}\n")).collect();
    let _ = fs::write(dir_dados().join("uso.tsv"), texto);
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn exec_com_aspas_e_campos() {
        let p = Path::new("/x/a.desktop");
        assert_eq!(dividir_exec("dolphin %u", "Dolphin", p, None), vec!["dolphin"]);
        assert_eq!(
            dividir_exec(r#"sh -c "echo \"oi\" %%" %F"#, "X", p, None),
            vec!["sh", "-c", "echo \"oi\" %"]
        );
        assert_eq!(dividir_exec("app --name=%c %k", "Meu App", p, None), vec!["app", "--name=Meu App", "/x/a.desktop"]);
        assert_eq!(dividir_exec("app %i", "A", p, Some("ico")), vec!["app", "--icon", "ico"]);
    }

    #[test]
    fn escapes_do_valor() {
        assert_eq!(desescapar(r"a\sb\\c"), r"a b\c");
    }
}
