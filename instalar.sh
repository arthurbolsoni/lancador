#!/bin/sh
# KDE Plasma (Wayland): compila, instala em ~/.local/bin, registra o serviço do usuário
# (lancador.service, sobe com a sessão gráfica) e o .desktop que o atalho do KDE chama.
set -e
cd "$(dirname "$0")"
cargo build --release

bin="$HOME/.local/bin/lancador"
mkdir -p "$HOME/.local/bin" "$HOME/.config/systemd/user" "$HOME/.local/share/applications"
# Cópia + mv: o binário em uso não pode ser sobrescrito no lugar.
cp target/release/lancador "$bin.novo"
mv -f "$bin.novo" "$bin"

cat > "$HOME/.config/systemd/user/lancador.service" <<EOF
[Unit]
Description=Lançador (Alt+Space)
After=graphical-session.target
PartOf=graphical-session.target

[Service]
Type=simple
ExecStart=$bin
Restart=on-failure
RestartSec=2

[Install]
WantedBy=graphical-session.target
EOF

cat > "$HOME/.local/share/applications/lancador-alternar.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Lançador (abrir/fechar)
Exec=$bin --alternar
Icon=system-search
NoDisplay=true
StartupNotify=false
X-KDE-StartupNotify=false
EOF

systemctl --user daemon-reload
systemctl --user enable lancador.service
systemctl --user restart lancador.service
echo "Instalado. Atalho: Configurações do sistema > Atalhos > Lançador (abrir/fechar)."
