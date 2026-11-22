#!/bin/bash
# ============================================================
#  GL Server - Ubuntu/Linux 启动脚本
#  行为：先停止所有 gl-server 进程，再启动服务
#  用法：./start.sh [start|stop|restart]   （默认 restart）
# ============================================================
set -euo pipefail

# 定位到脚本所在目录（项目根目录）
cd "$(dirname "$0")"

BINARY="target/release/gl-server"
CONFIG_PATH="$(pwd)/config.toml"
PID_FILE="/tmp/gl-server.pid"
LOG_FILE="/tmp/gl-server.log"

ACTION="${1:-restart}"

stop_server() {
    echo "[stop] Stopping existing gl-server processes..."

    # 1) 优先用 PID 文件优雅停止
    if [[ -f "$PID_FILE" ]]; then
        local pid
        pid="$(cat "$PID_FILE" 2>/dev/null || true)"
        if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
            echo "       Sending SIGTERM to PID $pid ..."
            kill "$pid" 2>/dev/null || true
            # 等待最多 10 秒退出
            for _ in $(seq 1 20); do
                kill -0 "$pid" 2>/dev/null || break
                sleep 0.5
            done
            # 仍未退出则强制杀死
            if kill -0 "$pid" 2>/dev/null; then
                echo "       Force killing PID $pid ..."
                kill -9 "$pid" 2>/dev/null || true
            fi
        fi
        rm -f "$PID_FILE"
    fi

    # 2) 兜底：按进程名清除所有残留 gl-server
    if pgrep -x gl-server >/dev/null 2>&1; then
        echo "       Found leftover gl-server by name, killing ..."
        pkill -x gl-server 2>/dev/null || true
        sleep 1
        pkill -9 -x gl-server 2>/dev/null || true
    fi

    echo "       All gl-server processes stopped."
}

start_server() {
    if [[ ! -f "$BINARY" ]]; then
        echo "[build] Binary not found, building release ..."
        cargo build --release -p gl-server
    fi

    echo "[start] Starting gl-server ..."
    export CONFIG_PATH
    nohup "$BINARY" >> "$LOG_FILE" 2>&1 &
    local pid=$!
    echo "$pid" > "$PID_FILE"
    echo "       gl-server started (PID $pid)."
    echo "       Log:  $LOG_FILE"
    echo "       Health: http://127.0.0.1:8080/health"
}

case "$ACTION" in
    start)
        start_server
        ;;
    stop)
        stop_server
        ;;
    restart)
        stop_server
        start_server
        ;;
    *)
        echo "Usage: $0 [start|stop|restart]"
        exit 1
        ;;
esac
