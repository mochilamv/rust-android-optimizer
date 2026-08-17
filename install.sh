#!/data/data/com.termux/files/usr/bin/bash
# ==============================================================================
# Rust Android Optimizer - Native Installer
# Author: Mochilamv & IAs
# License: MIT
# ==============================================================================

set -e

COLOR_GREEN='\033[0;32m'
COLOR_BLUE='\033[0;34m'
COLOR_YELLOW='\033[1;33m'
COLOR_RED='\033[0;31m'
COLOR_RESET='\033[0m'

echo -e "${COLOR_BLUE}--- Rust Android Optimizer: Instalador Nativo ---${COLOR_RESET}"

# 1. Checagem de Arquitetura e CPU
ARCH=$(uname -m)
if [ "$ARCH" != "aarch64" ] && [ "$ARCH" != "arm64" ]; then
    echo -e "${COLOR_RED}[ERRO] Arquitetura incompativel: $ARCH. Apenas aarch64 (ARM64) e suportada.${COLOR_RESET}"
    exit 1
fi

CPU_MODEL=$(getprop ro.soc.model 2>/dev/null || true)
if [ -z "$CPU_MODEL" ]; then
    CPU_MODEL=$(getprop ro.hardware 2>/dev/null || uname -m)
fi
echo -e "${COLOR_GREEN}[OK] CPU / Arquitetura: $ARCH ($CPU_MODEL)${COLOR_RESET}"

# 2. Checagem do Shizuku / Rish
RISH_PATH="/data/data/com.termux/files/usr/bin/rish"
if [ ! -f "$RISH_PATH" ]; then
    echo -e "${COLOR_RED}[ERRO] Binario rish nao encontrado em $RISH_PATH${COLOR_RESET}"
    echo -e "${COLOR_YELLOW}Ative o Shizuku e exporte o rish para o Termux antes de prosseguir.${COLOR_RESET}"
    exit 1
fi

if ! "$RISH_PATH" -c "id" >/dev/null 2>&1; then
    echo -e "${COLOR_RED}[ERRO] Shizuku rish instalado mas sem permissao/inativo.${COLOR_RESET}"
    exit 1
fi
echo -e "${COLOR_GREEN}[OK] Shizuku rish ativo e autorizado.${COLOR_RESET}"

# 3. Blindagem de Segundo Plano (Phantom Process Killer & Whitelist de Bateria)
echo -e "[INFO] Blindando Termux e Shizuku contra encerramento em segundo plano..."
"$RISH_PATH" -c "/system/bin/device_config put activity_manager max_phantom_processes 2147483647 2>/dev/null; /system/bin/device_config set_sync_disabled_for_tests persistent 2>/dev/null; setprop persist.sys.fflag.override.settings_enable_monitor_phantom_procs false 2>/dev/null; dumpsys deviceidle whitelist +com.termux 2>/dev/null; dumpsys deviceidle whitelist +moe.shizuku.privileged.api 2>/dev/null; cmd appops set com.termux RUN_IN_BACKGROUND allow 2>/dev/null; cmd appops set com.termux RUN_ANY_IN_BACKGROUND allow 2>/dev/null; cmd appops set moe.shizuku.privileged.api RUN_IN_BACKGROUND allow 2>/dev/null; cmd appops set moe.shizuku.privileged.api RUN_ANY_IN_BACKGROUND allow 2>/dev/null" >/dev/null 2>&1 || true
echo -e "${COLOR_GREEN}[OK] Termux e Shizuku blindados na whitelist do sistema.${COLOR_RESET}"

# 4. Checagem do compilador Rust / Cargo
if ! command -v cargo >/dev/null 2>&1 || ! command -v rustc >/dev/null 2>&1; then
    echo -e "${COLOR_YELLOW}[INFO] Instalando compilador Rust via pkg...${COLOR_RESET}"
    pkg update -y
    pkg install -y rust
fi
echo -e "${COLOR_GREEN}[OK] Compilador Rust: $(rustc --version)${COLOR_RESET}"

# 5. Compilacao Nativa
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo -e "${COLOR_BLUE}[BUILD] Compilando binario nativo (target-cpu=native | opt-level=3)...${COLOR_RESET}"
RUSTFLAGS="-C target-cpu=native" cargo build --release

BIN_SOURCE="$SCRIPT_DIR/target/release/rust-android-optimizer"
BIN_DEST="$PREFIX/bin/rust-android-optimizer"

if [ ! -f "$BIN_SOURCE" ]; then
    echo -e "${COLOR_RED}[ERRO] Falha ao gerar binario em $BIN_SOURCE${COLOR_RESET}"
    exit 1
fi

cp -f "$BIN_SOURCE" "$BIN_DEST"
chmod 755 "$BIN_DEST"
echo -e "${COLOR_GREEN}[OK] Binario instalado em $BIN_DEST${COLOR_RESET}"

# 6. Insercao de Aliases
inject_aliases() {
    local rc_file="$1"
    if [ -f "$rc_file" ]; then
        if ! grep -q "rust-optimizer-start" "$rc_file"; then
            echo "" >> "$rc_file"
            echo "# --- Rust Android Optimizer Aliases ---" >> "$rc_file"
            echo "alias rust-optimizer-start=\"rust-android-optimizer start\"" >> "$rc_file"
            echo "alias rust-optimizer-status=\"rust-android-optimizer status\"" >> "$rc_file"
            echo "alias rust-optimizer-stop=\"rust-android-optimizer stop\"" >> "$rc_file"
            echo -e "${COLOR_GREEN}[OK] Aliases configurados em $rc_file${COLOR_RESET}"
        fi
    fi
}

inject_aliases "$HOME/.bashrc"
inject_aliases "$HOME/.zshrc"

# 7. Opcao de Auto-Inicializacao com o Sistema (Termux:Boot)
echo ""
echo -e "${COLOR_YELLOW}Permitir inicializacao junto ao boot? (S/n)${COLOR_RESET}"
read -r -t 15 ENABLE_BOOT || ENABLE_BOOT="s"
BOOT_DIR="$HOME/.termux/boot"
BOOT_SCRIPT="$BOOT_DIR/start-rust-optimizer.sh"

if [[ "$ENABLE_BOOT" =~ ^[SsYy]?$ ]]; then
    mkdir -p "$BOOT_DIR"
    cat << 'EOF' > "$BOOT_SCRIPT"
#!/data/data/com.termux/files/usr/bin/bash
# Rust Android Optimizer - Auto-Start on Boot
termux-wake-lock 2>/dev/null || true
for i in $(seq 1 30); do
    if [ "$(getprop sys.boot_completed 2>/dev/null)" = "1" ]; then
        break
    fi
    sleep 1
done
sleep 5
rust-android-optimizer start >/dev/null 2>&1
EOF
    chmod 755 "$BOOT_SCRIPT"
    echo -e "${COLOR_GREEN}[OK] Auto-inicializacao ativada em $BOOT_SCRIPT (Termux:Boot)${COLOR_RESET}"
else
    if [ -f "$BOOT_SCRIPT" ]; then
        rm -f "$BOOT_SCRIPT"
    fi
    echo -e "${COLOR_YELLOW}[INFO] Auto-inicializacao no boot desativada.${COLOR_RESET}"
fi

# 8. Selecao de Modo Operacional
echo ""
echo -e "${COLOR_BLUE}[MODO OPERACIONAL]${COLOR_RESET}"
echo -e "Escolha o modo de operacao:"
echo -e "  1) Adaptativo (Padrao) - Ajustes apenas em jogos; restaura tudo ao sair"
echo -e "  2) Performance Constante - FPS maximo, toque instantaneo e GPU ativa"
read -r -t 20 OP_MODE_CHOICE || OP_MODE_CHOICE="1"
if [ "$OP_MODE_CHOICE" = "2" ]; then
    echo "performance" > "$HOME/.rust-android-optimizer.mode"
    echo -e "${COLOR_GREEN}[OK] Modo definido: Performance Constante${COLOR_RESET}"
else
    echo "adaptive" > "$HOME/.rust-android-optimizer.mode"
    echo -e "${COLOR_GREEN}[OK] Modo definido: Adaptativo${COLOR_RESET}"
fi

# 9. Benchmark de Hardware Opcional
echo ""
echo -e "${COLOR_YELLOW}Executar teste e benchmark de hardware agora? (S/n)${COLOR_RESET}"
read -r -t 15 RUN_BENCH || RUN_BENCH="s"
if [[ "$RUN_BENCH" =~ ^[SsYy]?$ ]]; then
    echo ""
    "$BIN_DEST" bench || true
fi

# 10. Limpeza de Cache de Build
echo ""
echo -e "${COLOR_YELLOW}Limpar arquivos temporarios de build (target/) para liberar espaco? (S/n)${COLOR_RESET}"
read -r -t 15 CLEAN_BUILD || CLEAN_BUILD="s"
if [[ "$CLEAN_BUILD" =~ ^[SsYy]?$ ]]; then
    rm -rf "$SCRIPT_DIR/target"
    echo -e "${COLOR_GREEN}[OK] Cache de build limpo.${COLOR_RESET}"
fi

# 11. Conclusao
echo ""
echo -e "${COLOR_GREEN}--- Instalacao Concluida com Sucesso! ---${COLOR_RESET}"
echo -e "Comandos disponiveis:"
echo -e "  * ${COLOR_BLUE}rust-optimizer-start${COLOR_RESET}   : Inicia o daemon em background"
echo -e "  * ${COLOR_BLUE}rust-optimizer-status${COLOR_RESET}  : Exibe estado, modo ativo e PID"
echo -e "  * ${COLOR_BLUE}rust-optimizer-stop${COLOR_RESET}    : Para o daemon e restaura o sistema"
echo ""
echo -e "${COLOR_YELLOW}by: Mochilamv & IAs${COLOR_RESET}"
