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

echo -e "${COLOR_BLUE}========================================================================${COLOR_RESET}"
echo -e "${COLOR_BLUE}               RUST ANDROID OPTIMIZER - INSTALADOR NATIVO               ${COLOR_RESET}"
echo -e "${COLOR_BLUE}========================================================================${COLOR_RESET}"

# 1. Checagem de Arquitetura
ARCH=$(uname -m)
if [ "$ARCH" != "aarch64" ] && [ "$ARCH" != "arm64" ]; then
    echo -e "${COLOR_RED}[ERRO FATAL] Arquitetura incompativel: $ARCH. Apenas aarch64 (ARM64) e suportada.${COLOR_RESET}"
    exit 1
fi
echo -e "${COLOR_GREEN}[OK] Arquitetura detectada: $ARCH${COLOR_RESET}"

# 2. Checagem do Shizuku / Rish
RISH_PATH="/data/data/com.termux/files/usr/bin/rish"
if [ ! -f "$RISH_PATH" ]; then
    echo -e "${COLOR_RED}[ERRO FATAL] Binario do Shizuku rish nao encontrado em $RISH_PATH${COLOR_RESET}"
    echo -e "${COLOR_YELLOW}Para utilizar o modulo, e necessario ter o Shizuku ativo e configurado no Termux:${COLOR_RESET}"
    echo -e "  1. Instale e abra o aplicativo Shizuku."
    echo -e "  2. Inicie o servico Shizuku via Wireless Debugging ou Root."
    echo -e "  3. Exporte os arquivos do rish para o Termux seguindo o guia oficial do Shizuku."
    echo -e "  4. Execute novamente este instalador."
    exit 1
fi

echo -e "[INFO] Testando permissao do Shizuku..."
if ! "$RISH_PATH" -c "id" >/dev/null 2>&1; then
    echo -e "${COLOR_RED}[ERRO FATAL] O Shizuku rish esta instalado mas nao respondeu ao teste de comando.${COLOR_RESET}"
    echo -e "${COLOR_YELLOW}Certifique-se de que o servico do Shizuku esta em execucao e o Termux autorizado.${COLOR_RESET}"
    exit 1
fi
echo -e "${COLOR_GREEN}[OK] Shizuku rish ativo e com permissoes concedidas.${COLOR_RESET}"

# 3. Checagem do compilador Rust / Cargo
if ! command -v cargo >/dev/null 2>&1 || ! command -v rustc >/dev/null 2>&1; then
    echo -e "${COLOR_YELLOW}[INFO] Ferramentas Rust/Cargo nao encontradas. Instalando via pkg...${COLOR_RESET}"
    pkg update -y
    pkg install -y rust
fi
echo -e "${COLOR_GREEN}[OK] Compilador Rust disponivel: $(rustc --version)${COLOR_RESET}"

# 4. Compilacao Nativa com Otimizacao Extrema
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo -e "${COLOR_BLUE}[COMPILACAO] Compilando binario nativo com otimizacao extrema...${COLOR_RESET}"
echo -e "             Target CPU: native | Opt-Level: 3 | LTO: Fat | Strip: true"

RUSTFLAGS="-C target-cpu=native" cargo build --release

BIN_SOURCE="$SCRIPT_DIR/target/release/rust-android-optimizer"
BIN_DEST="$PREFIX/bin/rust-android-optimizer"

if [ ! -f "$BIN_SOURCE" ]; then
    echo -e "${COLOR_RED}[ERRO] Falha ao gerar o binario compilado em $BIN_SOURCE${COLOR_RESET}"
    exit 1
fi

echo -e "[INFO] Instalando binario em $BIN_DEST..."
cp -f "$BIN_SOURCE" "$BIN_DEST"
chmod 755 "$BIN_DEST"
echo -e "${COLOR_GREEN}[OK] Binario instalado com sucesso!${COLOR_RESET}"

# 5. Insercao de Aliases no Bash/Zsh
inject_aliases() {
    local rc_file="$1"
    if [ -f "$rc_file" ]; then
        if ! grep -q "rust-optimizer-start" "$rc_file"; then
            echo "" >> "$rc_file"
            echo "# --- Rust Android Optimizer Aliases ---" >> "$rc_file"
            echo "alias rust-optimizer-start=\"rust-android-optimizer start\"" >> "$rc_file"
            echo "alias rust-optimizer-status=\"rust-android-optimizer status\"" >> "$rc_file"
            echo "alias rust-optimizer-stop=\"rust-android-optimizer stop\"" >> "$rc_file"
            echo -e "${COLOR_GREEN}[OK] Aliases adicionados a $rc_file${COLOR_RESET}"
        fi
    fi
}

inject_aliases "$HOME/.bashrc"
inject_aliases "$HOME/.zshrc"

# 6. Benchmark de Hardware Opcional
echo ""
echo -e "${COLOR_YELLOW}Deseja executar o benchmark e teste de deteccao de hardware agora? (S/n)${COLOR_RESET}"
read -r -t 15 RUN_BENCH || RUN_BENCH="s"
if [[ "$RUN_BENCH" =~ ^[SsYy]?$ ]]; then
    echo ""
    "$BIN_DEST" bench || true
fi

# 7. Opcao de Limpeza de Cache de Build
echo ""
echo -e "${COLOR_YELLOW}Deseja limpar os arquivos temporarios de build (pasta target) para economizar espaco? (S/n)${COLOR_RESET}"
read -r -t 15 CLEAN_BUILD || CLEAN_BUILD="s"
if [[ "$CLEAN_BUILD" =~ ^[SsYy]?$ ]]; then
    echo -e "[INFO] Limpando pasta target/..."
    rm -rf "$SCRIPT_DIR/target"
    echo -e "${COLOR_GREEN}[OK] Espaco em disco liberado.${COLOR_RESET}"
fi

# 8. Mensagem Final
echo ""
echo -e "${COLOR_GREEN}========================================================================${COLOR_RESET}"
echo -e "${COLOR_GREEN}               INSTALACAO CONCLUIDA COM SUCESSO!                       ${COLOR_RESET}"
echo -e "${COLOR_GREEN}========================================================================${COLOR_RESET}"
echo -e "Comandos disponiveis no seu terminal:"
echo -e "  * ${COLOR_BLUE}rust-optimizer-start${COLOR_RESET}   : Inicia o daemon em background em uma nova aba!"
echo -e "  * ${COLOR_BLUE}rust-optimizer-status${COLOR_RESET}  : Verifica se o daemon esta rodando e o PID!"
echo -e "  * ${COLOR_BLUE}rust-optimizer-stop${COLOR_RESET}    : Para o processo e restaura o sistema!"
echo ""
echo -e "Dica: Em caso de novo terminal, utilize: ${COLOR_BLUE}rust-optimizer-start${COLOR_RESET}"
echo ""
echo -e "${COLOR_YELLOW}Obrigado por instalar o rust-android-optimizer... nome meio grande, ne? :D${COLOR_RESET}"
echo -e "${COLOR_YELLOW}by: IAs & Mochilamv${COLOR_RESET}"
echo -e "${COLOR_GREEN}========================================================================${COLOR_RESET}"
