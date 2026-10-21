//! Casamento da consulta com os nomes, no espírito do StringMatcher do Flow Launcher:
//! prefixo > início de palavra > sigla > substring > subsequência. Sem acento e sem caixa.

/// Minúsculas e sem acento.
pub fn dobrar(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            c => c,
        })
        .collect()
}

/// Texto pesquisável de um item (nome ou apelido), pré-processado uma vez.
pub struct Chave {
    completo: String,
    palavras: Vec<String>,
    iniciais: String,
}

impl Chave {
    pub fn new(s: &str) -> Self {
        // Palavra nova em separador e na virada minúscula→maiúscula ("OpenRGB" = open, rgb).
        let mut palavras = Vec::new();
        let mut atual = String::new();
        let mut anterior_minuscula = false;
        for c in s.chars() {
            if !c.is_alphanumeric() {
                if !atual.is_empty() {
                    palavras.push(std::mem::take(&mut atual));
                }
                anterior_minuscula = false;
                continue;
            }
            if c.is_uppercase() && anterior_minuscula && !atual.is_empty() {
                palavras.push(std::mem::take(&mut atual));
            }
            atual.push(c);
            anterior_minuscula = c.is_lowercase();
        }
        if !atual.is_empty() {
            palavras.push(atual);
        }
        let palavras: Vec<String> = palavras.iter().map(|p| dobrar(p)).collect();
        let iniciais = palavras.iter().filter_map(|p| p.chars().next()).collect();
        Chave { completo: dobrar(s), palavras, iniciais }
    }
}

/// Pontua `consulta` (já dobrada e aparada) contra a chave. 0 = não casa.
pub fn pontuar(consulta: &str, k: &Chave) -> u32 {
    if consulta.is_empty() {
        return 0;
    }
    if k.completo == consulta {
        return 1000;
    }
    if k.completo.starts_with(consulta) {
        return 900;
    }
    if k.palavras.iter().any(|p| p.starts_with(consulta)) {
        return 800;
    }
    let tokens: Vec<&str> = consulta.split_whitespace().collect();
    if tokens.len() > 1 && tokens_em_palavras(&tokens, &k.palavras) {
        return 750;
    }
    let junto: String = tokens.concat();
    if junto.chars().count() >= 2 && k.iniciais.starts_with(&junto) {
        return 700;
    }
    if k.completo.contains(consulta) {
        return 600;
    }
    if junto.chars().count() >= 2 && subsequencia(&junto, &k.iniciais).is_some() {
        return 500;
    }
    match subsequencia(&junto, &k.completo) {
        Some(lacunas) => 400u32.saturating_sub(lacunas * 15).max(100),
        None => 0,
    }
}

/// Cada token é prefixo de uma palavra diferente, na ordem.
fn tokens_em_palavras(tokens: &[&str], palavras: &[String]) -> bool {
    let mut i = 0;
    for t in tokens {
        match palavras[i..].iter().position(|p| p.starts_with(t)) {
            Some(pos) => i += pos + 1,
            None => return false,
        }
    }
    true
}

/// `agulha` aparece em ordem dentro de `palheiro`? Devolve quantos caracteres pulou no meio.
fn subsequencia(agulha: &str, palheiro: &str) -> Option<u32> {
    let mut it = palheiro.chars();
    let mut lacunas = 0;
    let mut primeiro = true;
    for a in agulha.chars() {
        let mut pulou = 0;
        loop {
            match it.next() {
                Some(c) if c == a => break,
                Some(_) => pulou += 1,
                None => return None,
            }
        }
        if !primeiro {
            lacunas += pulou;
        }
        primeiro = false;
    }
    Some(lacunas)
}

#[cfg(test)]
mod testes {
    use super::*;

    fn p(q: &str, alvo: &str) -> u32 {
        pontuar(&dobrar(q), &Chave::new(alvo))
    }

    #[test]
    fn ordem_dos_tipos_de_casamento() {
        assert_eq!(p("discord", "Discord"), 1000);
        assert_eq!(p("disc", "Discord"), 900);
        assert_eq!(p("tarefas", "Gerenciador de Tarefas"), 800);
        assert_eq!(p("ger tar", "Gerenciador de Tarefas"), 750);
        assert_eq!(p("gdt", "Gerenciador de Tarefas"), 700);
        assert_eq!(p("renc", "Gerenciador de Tarefas"), 600);
        assert_eq!(p("gt", "Gerenciador de Tarefas"), 500);
        assert!(p("gnd", "Gerenciador") > 0);
        assert_eq!(p("xyz", "Discord"), 0);
    }

    #[test]
    fn acento_caixa_e_camel_case() {
        assert_eq!(p("configuracoes", "Configurações"), 1000);
        assert_eq!(p("SERV", "Serviços"), 900);
        assert_eq!(p("rgb", "OpenRGB"), 800);
        assert_eq!(p("or", "OpenRGB"), 700);
    }

    #[test]
    fn subsequencia_prefere_menos_lacunas() {
        assert!(p("brv", "Brave") > p("bae", "Brave"));
    }
}
