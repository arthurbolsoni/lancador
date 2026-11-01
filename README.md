# lancador

Lançador mínimo para Windows (no lugar do Flow Launcher) e para o KDE Plasma no Wayland (no lugar do Vicinae). A busca (`src/busca.rs`) e a lista de temas (`src/tema.rs`) são as mesmas nos dois; janela, índices e abertura são de cada sistema (`src/janela.rs` e `src/itens.rs` no Windows, `src/linux/` no KDE). A seção [KDE Plasma](#kde-plasma-wayland) descreve o que muda lá.

## Windows

Alt+Space ou Win sozinho abre (o Win vem do gancho de teclado da barra, que posta `WM_APP + 4`; sem a barra, só o Alt+Space); Esc ou perder o foco fecha e limpa o campo.

- Campo vazio: os apps fixados no Iniciar, na ordem do Iniciar.
- Busca: fixados + todos os apps do Iniciar (`shell:AppsFolder`, relido numa thread ao abrir, no máximo 1x por minuto; ícone extraído só quando o app aparece) + atalhos do sistema (Configurações, regedit, services.msc, Painel de Controle etc., lista em `src/itens.rs`). Desinstaladores, links de site e documentos ficam fora. Se há casamento real, os de letras soltas somem. Sem acento e sem caixa; prefixo > início de palavra > sigla > substring > subsequência. Apps mais abertos ganham peso.
- Teclas: ↑/↓/Tab, PgUp/PgDn, Enter abre, Ctrl+Shift+Enter abre como administrador; no campo, ←/→ (Ctrl = palavra), Home/End, Ctrl+Backspace, Ctrl+A, Ctrl+V.

### Visual

Fundo acrílico do DWM (`DWMSBT_TRANSIENTWINDOW`) com véu na cor de fundo do tema por cima. O conteúdo é desenhado em Direct2D/DirectWrite (modo software: o hardware carregava o driver da GPU, +50 MB) num DIB de 32 bits com alfa, copiado para a área cliente com o frame estendido. O campo de busca é próprio porque o EDIT do Win32 pinta fundo opaco. Fixados usam o ícone do app; atalhos do sistema usam glifos da Segoe Fluent Icons. A seleção desliza entre as linhas e a janela cresce/encolhe com easing exponencial.

### Tema

Segue o tema da barra: a lista de `src/tema.rs` é cópia de `barra/src/tema.rs`, na mesma ordem, e o índice escolhido vem de `HKCU\Software\barra\Tema` (sem a chave, o primeiro). É relido a cada Alt+Space. Tema claro usa acrílico claro. Tema novo na barra precisa ser copiado para cá, na mesma posição.

### Fixados

Vêm do `Export-StartLayout` (JSON com `desktopAppLink` / `packagedAppId`), guardado em `%LOCALAPPDATA%\lancador\fixados.json`. Quando o `start2.bin` do Iniciar fica mais novo que o cache, o export roda de novo numa thread ao abrir a janela.

### Instalar

```powershell
.\instalar.ps1
```

Compila, copia para `%LOCALAPPDATA%\Programs\lancador`, registra no `Run` do HKCU e reinicia. `lancador.exe --sair` encerra a instância em execução.

Dados: `%LOCALAPPDATA%\lancador\` (`fixados.json`, `uso.tsv`).

## KDE Plasma (Wayland)

Uma instância fica residente como serviço do usuário (`lancador.service`, sobe com a sessão gráfica), com a janela fechada. O atalho do KDE (Meta sozinho e Alt+Space) roda `lancador --alternar`, que avisa a instância pelo socket `$XDG_RUNTIME_DIR/lancador.sock` e sai. Esc ou perder o foco fecha e limpa o campo.

- Campo vazio: os mais usados (contagem em `~/.local/share/lancador/uso.tsv`), até 8.
- Busca: os apps dos arquivos .desktop (`~/.local/share/applications` sobrepõe `/usr/share/applications`, com `NoDisplay`, `Hidden`, `OnlyShowIn`/`NotShowIn` e `TryExec` respeitados), os módulos de configuração do KDE (`kcm_*.desktop`, que o menu esconde e abrem no `systemsettings`) e as ações de sessão (bloquear, suspender, encerrar sessão, reiniciar e desligar; as três últimas pela tela de confirmação do KDE). Além do nome, casam o executável, o nome genérico e as palavras-chave do .desktop. Os apps são relidos numa thread ao abrir, no máximo 1x por minuto. A pontuação é a mesma do Windows.
- Abrir: cada app sobe num escopo próprio do systemd (`systemd-run --user --scope`), como o KDE faz, e recebe o token de ativação do Wayland para ganhar o foco. `Terminal=true` abre no terminal do KDE (`TerminalApplication` do `kdeglobals`).
- Teclas: as do Windows, menos o Ctrl+Shift+Enter (abrir como administrador). Ctrl+V lê a área de transferência do Wayland.

### Visual

Superfície layer-shell na camada Overlay, centrada no alto da tela a um quarto da altura, com o blur do KWin atrás (protocolo ext-background-effect) e véu na cor de fundo do tema por cima. O conteúdo é desenhado em software num pixmap do tiny-skia: texto pelo ab_glyph (que lê só os glifos usados; o fontdue processava a fonte inteira e ocupava ~16 MB) na fonte do KDE (`font` do `kdeglobals`, Noto Sans por padrão; a busca em Light), ícones do tema de ícones do KDE (especificação freedesktop, SVG pelo resvg e PNG pelo crate png). A superfície tem a altura máxima; a região de entrada e a do blur acompanham a altura animada, e o clique abaixo dela passa para a janela de baixo. Medidas e animações são as do Windows. ~2,5 MB privados com a janela fechada.

### Tema

Vem do esquema de cores do KDE, relido a cada abertura: `OmarchyMatteBlack` (gravado pelo `omarchy-tema`) vira o tema `matte-black` de `src/tema.rs`. Esquema que não é do Omarchy usa as cores de visão e seleção do `kdeglobals`.

### Instalar

```sh
./instalar.sh
```

Compila, copia para `~/.local/bin/lancador`, grava e reinicia o `lancador.service` e cria `~/.local/share/applications/lancador-alternar.desktop` (`lancador --alternar`), que é o que o atalho chama. Atalho em Configurações do sistema > Atalhos ou na seção `[services][lancador-alternar.desktop]` do `kglobalshortcutsrc` (`_launch=Meta\tAlt+Space`); editado no arquivo, vale a partir do próximo login.

- `lancador --sair` encerra a instância (o serviço não a reinicia: `Restart=on-failure`).
- `lancador --testar <texto> <arquivo.png>` desenha a janela com essa busca num PNG, sem mostrar nada na tela.
