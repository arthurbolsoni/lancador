# lancador

Lançador mínimo para Windows, no lugar do Flow Launcher. Alt+Space abre; Esc ou perder o foco fecha e limpa o campo.

- Campo vazio: os apps fixados no Iniciar, na ordem do Iniciar.
- Busca: fixados + atalhos do sistema (Configurações, regedit, services.msc, Painel de Controle etc., lista em `src/itens.rs`). Sem acento e sem caixa; prefixo > início de palavra > sigla > substring > subsequência. Apps mais abertos ganham peso.
- Teclas: ↑/↓/Tab, PgUp/PgDn, Enter abre, Ctrl+Shift+Enter abre como administrador, Ctrl+Backspace apaga a palavra.

## Fixados

Vêm do `Export-StartLayout` (JSON com `desktopAppLink` / `packagedAppId`), guardado em `%LOCALAPPDATA%\lancador\fixados.json`. Quando o `start2.bin` do Iniciar fica mais novo que o cache, o export roda de novo numa thread ao abrir a janela.

## Instalar

```powershell
.\instalar.ps1
```

Compila, copia para `%LOCALAPPDATA%\Programs\lancador`, registra no `Run` do HKCU e reinicia. `lancador.exe --sair` encerra a instância em execução.

Dados: `%LOCALAPPDATA%\lancador\` (`fixados.json`, `uso.tsv`).
