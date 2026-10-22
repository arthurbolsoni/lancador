# lancador

Lançador mínimo para Windows, no lugar do Flow Launcher. Alt+Space abre; Esc ou perder o foco fecha e limpa o campo.

- Campo vazio: os apps fixados no Iniciar, na ordem do Iniciar.
- Busca: fixados + todos os apps do Iniciar (`shell:AppsFolder`, relido numa thread ao abrir, no máximo 1x por minuto; ícone extraído só quando o app aparece) + atalhos do sistema (Configurações, regedit, services.msc, Painel de Controle etc., lista em `src/itens.rs`). Desinstaladores, links de site e documentos ficam fora. Se há casamento real, os de letras soltas somem. Sem acento e sem caixa; prefixo > início de palavra > sigla > substring > subsequência. Apps mais abertos ganham peso.
- Teclas: ↑/↓/Tab, PgUp/PgDn, Enter abre, Ctrl+Shift+Enter abre como administrador; no campo, ←/→ (Ctrl = palavra), Home/End, Ctrl+Backspace, Ctrl+A, Ctrl+V.

## Visual

Fundo acrílico do DWM (`DWMSBT_TRANSIENTWINDOW`) com véu na cor de fundo do tema por cima. O conteúdo é desenhado em Direct2D/DirectWrite (modo software: o hardware carregava o driver da GPU, +50 MB) num DIB de 32 bits com alfa, copiado para a área cliente com o frame estendido. O campo de busca é próprio porque o EDIT do Win32 pinta fundo opaco. Fixados usam o ícone do app; atalhos do sistema usam glifos da Segoe Fluent Icons. A seleção desliza entre as linhas e a janela cresce/encolhe com easing exponencial.

## Tema

Segue o tema da barra: a lista de `src/tema.rs` é cópia de `barra/src/tema.rs`, na mesma ordem, e o índice escolhido vem de `HKCU\Softwarearra\Tema` (sem a chave, o primeiro). É relido a cada Alt+Space. Tema claro usa acrílico claro. Tema novo na barra precisa ser copiado para cá, na mesma posição.

## Fixados

Vêm do `Export-StartLayout` (JSON com `desktopAppLink` / `packagedAppId`), guardado em `%LOCALAPPDATA%\lancador\fixados.json`. Quando o `start2.bin` do Iniciar fica mais novo que o cache, o export roda de novo numa thread ao abrir a janela.

## Instalar

```powershell
.\instalar.ps1
```

Compila, copia para `%LOCALAPPDATA%\Programs\lancador`, registra no `Run` do HKCU e reinicia. `lancador.exe --sair` encerra a instância em execução.

Dados: `%LOCALAPPDATA%\lancador\` (`fixados.json`, `uso.tsv`).
