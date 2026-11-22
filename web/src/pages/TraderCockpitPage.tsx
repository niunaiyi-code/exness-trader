import React, { useEffect, useMemo, useRef, useState, useCallback } from 'react';
import { Alert, message, Button, Spin } from 'antd';
import { LeftOutlined, RightOutlined, UpOutlined, DownOutlined } from '@ant-design/icons';
import { api, apiErrorMessage, getSession } from '../lib/api';
import {
  GOLD_PROFIT_PER_DOLLAR_PER_LOT,
  DEFAULT_XAUUSD_SPREAD,
  USD_PROBE_PREFIX,
  USC_PROBE_PREFIX,
} from '../lib/constants';
import { PriceChart } from '../components/PriceChart';
import { QuickActions } from '../components/QuickActions';
import { HudBar } from '../components/HudBar';
import { palette } from '../theme';
import { playTradeSound } from '../lib/tradeSounds';
import { PositionItem } from '../components/orders/LivePositionsTable';
import { RawDeal } from '../components/orders/HistoryDealsTable';
import { TradingOrdersPanel, TradingOrdersPanelRef } from '../components/orders/TradingOrdersPanel';
import { StrategyParamsModal, StrategyParamsData } from '../components/StrategyParamsModal';

/** 后端 /accounts/:id/snapshot 返回的原始持仓结构 */
interface RawPosition {
  id: string;
  symbol: string;
  type?: string;
  position_type?: string;
  volume: string | number;
  open_price: string | number;
  current_price?: string | number;
  profit?: string | number;
  open_time?: number;
}

interface RawSnapshot {
  account_state?: {
    balance?: string;
    equity?: string;
    used_margin?: string;
    currency?: string;
  };
  positions?: RawPosition[];
}

/** WS 重连退避上限 */
const MAX_WS_RETRY_DELAY_MS = 30_000;

export const TraderCockpitPage: React.FC = () => {
  const session = getSession();
  const accountId = session?.username ?? '';

  const [currentBid, setCurrentBid] = useState(0);
  const [currentAsk, setCurrentAsk] = useState(0);
  const [balance, setBalance] = useState(0);
  const [equity, setEquity] = useState(0);
  const [usedMargin, setUsedMargin] = useState(0);
  const [positions, setPositions] = useState<PositionItem[]>([]);
  const [positionsLoading, setPositionsLoading] = useState(false);
  const [pageLoading, setPageLoading] = useState(true);
  const [isQuickActionsMinimized, setIsQuickActionsMinimized] = useState(false);
  const [isOrdersMinimized, setIsOrdersMinimized] = useState(false);
  const [viewportHeight, setViewportHeight] = useState(() =>
    typeof window !== 'undefined' ? window.innerHeight : 900,
  );

  useEffect(() => {
    const handleWinResize = () => setViewportHeight(window.innerHeight);
    window.addEventListener('resize', handleWinResize);
    return () => window.removeEventListener('resize', handleWinResize);
  }, []);

  // 动态自适应图表高度：
  // 1. 底部订单栏折叠后，图表垂直自适应填满屏幕剩余空间（保底 640px，在常见屏幕上扩展至 760px~850px+ 沉浸式巨幕视觉）；
  // 2. 底部订单栏展开时，维持 480px 标准紧凑高度，上下布局协调。
  const chartHeight = useMemo(() => {
    if (isOrdersMinimized) {
      return Math.max(640, viewportHeight - 140);
    }
    return 480;
  }, [isOrdersMinimized, viewportHeight]);

  const ordersPanelRef = useRef<TradingOrdersPanelRef>(null);
  const [isPaused, setIsPaused] = useState(false);
  const [isRunning, setIsRunning] = useState(false);
  const [actionLoading, setActionLoading] = useState(false);
  const [wsConnected, setWsConnected] = useState(false);
  const [utcClock, setUtcClock] = useState(() => new Date().toUTCString().slice(17, 25));

  // 策略运行参数状态
  const [strategyParams, setStrategyParams] = useState<StrategyParamsData | null>(null);
  const [isParamsModalOpen, setIsParamsModalOpen] = useState(false);

  const handleOpenParamsModal = () => {
    setIsParamsModalOpen(true);
  };

  const fetchStrategyParams = useCallback(async () => {
    if (!accountId) return;
    try {
      const resp = await api.get<StrategyParamsData>(`/accounts/${accountId}/params`);
      setStrategyParams(resp.data);
    } catch (err) {
      console.warn('[cockpit] Failed to fetch strategy params:', err);
    }
  }, [accountId]);

  useEffect(() => {
    fetchStrategyParams();
  }, [fetchStrategyParams]);

  // 用 ref 跟踪最新 bid 与账户币种，避免闭包过期（stale closure）
  const bidRef = useRef(0);
  const currencyRef = useRef('');
  const playedEventsRef = useRef<Set<string>>(new Set());

  // 快捷键：在快速交易最小化时，按 Q 可直接展开恢复
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      const activeEl = document.activeElement as HTMLElement | null;
      if (
        activeEl &&
        (activeEl.tagName === 'INPUT' ||
          activeEl.tagName === 'TEXTAREA' ||
          activeEl.isContentEditable)
      ) {
        return;
      }
      if (e.key === 'q' || e.key === 'Q') {
        if (isQuickActionsMinimized) {
          e.preventDefault();
          setIsQuickActionsMinimized(false);
          message.info({
            content: '已展开快速交易与应急控制台',
            key: 'q_restore_notice',
            duration: 1.5,
          });
        }
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isQuickActionsMinimized]);

  // UTC 时钟每秒自走（不依赖 tick 到达）
  useEffect(() => {
    const timer = setInterval(() => setUtcClock(new Date().toUTCString().slice(17, 25)), 1000);
    return () => clearInterval(timer);
  }, []);

  // 实时持仓：由 bid/ask 行情跟踪当前价格与浮动盈亏。
  const livePositions = useMemo(
    () =>
      positions.map((p) => {
        const ref = p.direction === 'BUY' ? currentBid : currentAsk;
        // 有实时 tick 行情时，以最新 tick 推导当前市价与浮动盈亏
        if (ref > 0) {
          const sign = p.direction === 'BUY' ? 1 : -1;
          const rawProfit = (ref - p.openPrice) * sign * p.lots * GOLD_PROFIT_PER_DOLLAR_PER_LOT;
          const profit = Math.round(rawProfit * 100) / 100;
          return { ...p, currentPrice: ref, profit };
        }
        // 若实时 tick 暂未到达（如建连等待期或休市），显示快照带回的市价和浮盈，绝不拿 openPrice 兜底成 0 浮盈
        return p;
      }),
    [positions, currentBid, currentAsk],
  );

  // 每次价格 tick 实时计算浮动盈亏（严格按分累加与精度对齐，确保与持仓明细逐笔金额严丝合缝、完全一致）
  const liveFloatingPnl = useMemo(() => {
    if (positions.length === 0) return 0;
    if (currentBid > 0 || currentAsk > 0) {
      const sum = livePositions.reduce((acc, p) => acc + p.profit, 0);
      return Math.round(sum * 100) / 100;
    }
    const posProfitSum = positions.reduce((acc, p) => acc + p.profit, 0);
    if (posProfitSum !== 0) return Math.round(posProfitSum * 100) / 100;
    return Math.round((equity - balance) * 100) / 100;
  }, [livePositions, positions, currentBid, currentAsk, equity, balance]);

  // 实时动态净值、可用保证金与保证金比例
  const liveEquity = balance + liveFloatingPnl;
  const liveFreeMargin = liveEquity - usedMargin;
  const liveMarginLevel = usedMargin > 0 ? (liveEquity / usedMargin) * 100 : 0;

  // 账户初始基线（余额/净值/保证金 + 持仓 + 状态）。页面加载或WS重连时主动请求一次，零定时轮询。
  const loadBaseline = useCallback(async (showPositionsSpinner = false) => {
    if (!accountId) return;
    if (showPositionsSpinner) setPositionsLoading(true);
    try {
      const [snapResp, stateResp] = await Promise.allSettled([
        api.get<RawSnapshot>(`/accounts/${accountId}/snapshot`),
        api.get<{ state?: string }>(`/accounts/${accountId}/state`),
      ]);
      if (snapResp.status === 'fulfilled') {
        const snap = snapResp.value.data || {};
        const st = snap.account_state || {};
        setBalance(parseFloat(st.balance ?? '') || 0);
        setEquity(parseFloat(st.equity ?? '') || 0);
        setUsedMargin(parseFloat(st.used_margin ?? '') || 0);
        if (st.currency) currencyRef.current = st.currency;
        const rawPositions = snap.positions || [];
        setPositions(
          rawPositions.map((p) => {
            const openPrice = parseFloat(String(p.open_price)) || 0;
            const currentPrice = parseFloat(String(p.current_price)) || 0;
            const profit = parseFloat(String(p.profit)) || 0;
            return {
              id: p.id,
              ticket: p.id,
              symbol: p.symbol,
              direction: ((p.type || p.position_type) === 'sell' ? 'SELL' : 'BUY') as 'BUY' | 'SELL',
              lots: parseFloat(String(p.volume)) || 0,
              openPrice,
              currentPrice,
              profit,
              openTime: p.open_time ? new Date(p.open_time * 1000).toISOString().slice(11, 19) : '',
            };
          }),
        );
      } else {
        console.warn('[cockpit] Failed to fetch account snapshot baseline:', snapResp.reason);
      }
      if (stateResp.status === 'fulfilled') {
        setIsRunning(stateResp.value.data?.state === 'running');
      } else {
        console.warn('[cockpit] Failed to fetch account state baseline:', stateResp.reason);
      }

      // 如果尚未收到任何实时 WS 行情 tick（例如刚进入页面或周末停盘期间），从最新 K 线对齐当前市价基准
      if (bidRef.current === 0) {
        try {
          const candleResp = await api.get<{ close: number }[]>('/market/candles', {
            params: { symbol: 'XAUUSD', timeframe: 'M1', count: 1 },
          });
          const lastCandle = candleResp.data?.[candleResp.data.length - 1];
          if (lastCandle && lastCandle.close > 0 && bidRef.current === 0) {
            const lastClose = lastCandle.close;
            bidRef.current = lastClose;
            setCurrentBid(lastClose);
            setCurrentAsk(lastClose + DEFAULT_XAUUSD_SPREAD);
          }
        } catch (e) {
          console.warn('[cockpit] Failed to fetch baseline price from candle:', e);
        }
      }
    } catch (err) {
      console.warn('[cockpit] Snapshot baseline load error:', err);
    } finally {
      setPageLoading(false);
      setPositionsLoading(false);
    }
  }, [accountId]);

  useEffect(() => {
    loadBaseline();
    // 30秒低频定时对账：在策略停止或网络异常时保底对齐最新持仓与资金状态
    const timer = setInterval(() => {
      void loadBaseline();
    }, 30_000);
    return () => clearInterval(timer);
  }, [loadBaseline]);

  // 通过 WebSocket 接收实时行情与成交事件；断线自动指数退避重连。
  useEffect(() => {
    if (!accountId) return;
    let ws: WebSocket | null = null;
    let retryCount = 0;
    let retryTimer: number | undefined;
    let closed = false;

    // 严格行情隔离：按账户币种接收对应探针的 tick（支持显式 currency 标记与探针前缀）
    const probeMatches = (dataCurrency?: string, probeId?: string) => {
      const accountCurrency = currencyRef.current.toUpperCase();
      if (!accountCurrency) return true;
      if (dataCurrency) {
        return dataCurrency.toUpperCase() === accountCurrency;
      }
      if (probeId) {
        if (accountCurrency === 'USC') return probeId.startsWith(USC_PROBE_PREFIX);
        if (accountCurrency === 'USD') return probeId.startsWith(USD_PROBE_PREFIX) || probeId.startsWith('19');
      }
      return true;
    };

    const matchesAccount = (target: string) => {
      if (!target) return true;
      if (accountId === 'admin') return true;
      return target === accountId;
    };

    const connect = () => {
      if (closed) return;
      try {
        const wsProtocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        const wsHost = window.location.hostname === 'localhost' ? 'localhost:8080' : window.location.host;
        const session = getSession();
        const wsUrl = `${wsProtocol}//${wsHost}/ws?account_id=${encodeURIComponent(accountId)}`;
        // Token 通过 WebSocket 子协议传递（Sec-WebSocket-Protocol: bearer.<jwt>），
        // 避免 JWT 明文进入 URL 被访问日志/反向代理记录。
        const subprotocols = session?.token ? [`bearer.${session.token}`] : [];
        ws = new WebSocket(wsUrl, subprotocols);
      } catch (err) {
        console.warn('[cockpit] WebSocket constructor error:', err);
        scheduleReconnect();
        return;
      }
      ws.onopen = () => {
        retryCount = 0;
        setWsConnected(true);
        // 连接或重连成功时主动对齐基线
        void loadBaseline();
        void ordersPanelRef.current?.refreshHistory();
      };
      ws.onmessage = (event) => {
        try {
          const data = JSON.parse(event.data);
          if (data.type === 'tick') {
            if (!probeMatches(data.currency, String(data.account_id || ''))) return;
            const bid = parseFloat(data.bid);
            const ask = parseFloat(data.ask);
            if (!isNaN(bid) && !isNaN(ask)) {
              bidRef.current = bid;
              setCurrentBid(bid);
              setCurrentAsk(ask);
            }
          } else if (data.type === 'runner_status' && matchesAccount(String(data.account_id || ''))) {
            setIsRunning(data.status === 'running');
          } else if (data.type === 'alert' && matchesAccount(String(data.account_id || ''))) {
            playTradeSound('alert');
            message.warning(data.message || '系统风险警报');
          } else if (data.type === 'event' && matchesAccount(String(data.account_id || ''))) {
            if (data.event_type === 'trade_event') {
              const trade = data.data || {};
              const eventId = String(trade.event_id || '');
              // 幂等去重：防止同一笔成交重复播放音效
              if (eventId) {
                if (playedEventsRef.current.has(eventId)) return;
                playedEventsRef.current.add(eventId);
                if (playedEventsRef.current.size > 200) {
                  const first = playedEventsRef.current.values().next().value;
                  if (first) playedEventsRef.current.delete(first);
                }
              }
              const eventType = trade.event_type;
              if (eventType === 'open_grid' || eventType === 'open') {
                playTradeSound('open');
              } else if (eventType === 'open_lock') {
                playTradeSound('lock');
              } else if (eventType === 'close_tp') {
                playTradeSound('close');
              } else if (eventType === 'close_sl') {
                playTradeSound('stop_loss');
              } else if (eventType === 'close_lock') {
                playTradeSound('close');
              }
            } else if (data.event_type === 'transaction') {
              const payload = data.data || {};
              const deals = (payload.deals || []) as RawDeal[];
              const posList = (payload.positions || []) as RawPosition[];
              const acctState = payload.account_state;

              // 资金变动
              if (acctState) {
                if (acctState.balance != null) setBalance(parseFloat(acctState.balance) || 0);
                if (acctState.equity != null) setEquity(parseFloat(acctState.equity) || 0);
                if (acctState.used_margin != null) setUsedMargin(parseFloat(acctState.used_margin) || 0);
              }

              // 平仓成交 deals（纯数据状态同步，音效统一由 trade_event 触发，避免双重发声）
              if (deals && deals.length > 0) {
                const closeDeals = deals.filter((d) => (d.type || d.deal_type || '').toLowerCase() === 'close');
                if (closeDeals.length > 0) {
                  ordersPanelRef.current?.prependClosedDeals(closeDeals);

                  const closedIds = new Set(closeDeals.map((d) => String(d.position_id || d.deal_id)));
                  setPositions((prev) => prev.filter((p) => !closedIds.has(p.id) && !closedIds.has(p.ticket)));
                }
              }

              // 新开仓 positions（纯数据状态同步）
              if (posList && posList.length > 0) {
                setPositions((prev) => {
                  const existingIds = new Set(prev.map((p) => p.id));
                  const toAdd: PositionItem[] = [];
                  for (const p of posList) {
                    if (!existingIds.has(p.id)) {
                      const openPrice = parseFloat(String(p.open_price)) || 0;
                      const currentPrice = parseFloat(String(p.current_price)) || 0;
                      const profit = parseFloat(String(p.profit)) || 0;
                      toAdd.push({
                        id: p.id,
                        ticket: p.id,
                        symbol: p.symbol,
                        direction: ((p.type || p.position_type) === 'sell' ? 'SELL' : 'BUY') as 'BUY' | 'SELL',
                        lots: parseFloat(String(p.volume)) || 0,
                        openPrice,
                        currentPrice,
                        profit,
                        openTime: p.open_time ? new Date(p.open_time * 1000).toISOString().slice(11, 19) : '',
                      });
                    }
                  }
                  if (toAdd.length > 0) {
                    return [...toAdd, ...prev];
                  }
                  return prev;
                });
              }
            }
          }
        } catch (err) {
          console.warn('[cockpit] Failed to parse WebSocket message frame:', err);
        }
      };
      ws.onclose = () => {
        setWsConnected(false);
        scheduleReconnect();
      };
      ws.onerror = () => {
        ws?.close();
      };
    };

    const scheduleReconnect = () => {
      if (closed) return;
      const delay = Math.min(MAX_WS_RETRY_DELAY_MS, 1000 * 2 ** retryCount);
      retryCount += 1;
      retryTimer = window.setTimeout(connect, delay);
    };

    connect();
    return () => {
      closed = true;
      if (retryTimer) clearTimeout(retryTimer);
      ws?.close();
    };
  }, [accountId, loadBaseline]);

  // 登录信息缺失（例如 localStorage 被清）时给出明确提示，而不是拿假账号号请求后端。
  // 必须放在所有 Hook 声明之后，保证 Hook 调用顺序无条件稳定。
  if (!accountId) {
    return (
      <Alert
        type="error"
        showIcon
        message="登录信息缺失"
        description="未找到交易账户标识，请退出后重新登录。"
      />
    );
  }

  // 快捷操作统一入口：in-flight 守卫防止重复提交（一键连点 = 重复市价单）
  const handleQuickAction = async (actionType: string, payload?: Record<string, unknown>) => {
    if (actionLoading) return;
    setActionLoading(true);
    try {
      if (actionType === 'start') {
        await api.post(`/accounts/${accountId}/start`, {});
        setIsRunning(true);
        message.success('实盘策略已成功挂载运行！');
      } else if (actionType === 'stop') {
        await api.post(`/accounts/${accountId}/stop`, {});
        setIsRunning(false);
        message.success('实盘策略已安全停止！');
      } else {
        const resp = await api.post<{ message?: string }>(`/accounts/${accountId}/actions`, {
          action: actionType,
          ...(payload || {}),
        });
        const label = actionType === 'buy' || actionType === 'sell' ? `市价 ${actionType.toUpperCase()}` : `指令 [${actionType}]`;
        message.success(resp.data?.message || `${label} 执行成功！`);
      }
      // 操作完成后立即对齐最新持仓与成交历史基线
      void loadBaseline();
      void ordersPanelRef.current?.refreshHistory();
    } catch (e) {
      message.error(apiErrorMessage(e, `指令 [${actionType}] 执行失败`));
    } finally {
      if (actionType === 'pause') setIsPaused(true);
      else if (actionType === 'resume') setIsPaused(false);
      setActionLoading(false);
    }
  };

  return (
    <Spin spinning={pageLoading} tip="正在连接 Exness 交易服务并加载账户基线..." size="large">
      <div style={{ padding: '0 8px' }}>
        {/* 顶部单行全局高密度状态栏：HUD 标识 + 六大资产核心指标 + 行情点差与时钟 + 调参齿轮 */}
        <HudBar
          accountId={accountId}
          isRunning={isRunning}
          wsConnected={wsConnected}
          utcTime={`${utcClock} UTC`}
          balance={balance}
          liveEquity={liveEquity}
          liveFloatingPnl={liveFloatingPnl}
          usedMargin={usedMargin}
          liveFreeMargin={liveFreeMargin}
          liveMarginLevel={liveMarginLevel}
          canEditParams={Boolean(strategyParams && !strategyParams.params_locked)}
          onOpenParams={handleOpenParamsModal}
        />

        {/* 中部：走势图 + 快捷操作（支持右侧垂直居中切换 > / <） */}
        <div
          style={{
            display: 'flex',
            gap: 10,
            marginBottom: 4,
            alignItems: 'stretch',
            width: '100%',
            position: 'relative',
          }}
        >
          {/* 左侧走势图：自适应伸展占满剩余宽度 */}
          <div style={{ flex: 1, minWidth: 0 }}>
            <PriceChart currentBid={currentBid} currentAsk={currentAsk} height={chartHeight} />
          </div>

          {/* 右侧快速交易与应急面板：展开为 360px，折叠时为 24px 侧边条，垂直居中切换 > / < */}
          {!isQuickActionsMinimized ? (
            <div style={{ width: 360, flexShrink: 0, position: 'relative' }}>
              {/* 位于走势图与侧栏分界线垂直居中的折叠把手 (>) */}
              <div
                onClick={() => setIsQuickActionsMinimized(true)}
                title="折叠快速交易面板 (快捷键 Q)"
                style={{
                  position: 'absolute',
                  left: -12,
                  top: '50%',
                  transform: 'translateY(-50%)',
                  zIndex: 20,
                  width: 20,
                  height: 38,
                  background: '#1a1a1a',
                  border: `1px solid ${palette.borderSecondary}`,
                  borderRadius: '6px 0 0 6px',
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  cursor: 'pointer',
                  boxShadow: '-2px 0 6px rgba(0,0,0,0.4)',
                  transition: 'all 0.2s ease',
                }}
              >
                <RightOutlined style={{ color: palette.gold, fontSize: 12 }} />
              </div>
              <QuickActions
                isPaused={isPaused}
                isRunning={isRunning}
                onAction={handleQuickAction}
                loading={actionLoading}
                currentBid={currentBid}
                currentAsk={currentAsk}
                symbol="XAUUSD"
              />
            </div>
          ) : (
            <div
              onClick={() => setIsQuickActionsMinimized(false)}
              title="展开快速交易面板 (快捷键 Q)"
              style={{
                width: 24,
                flexShrink: 0,
                background: palette.bgContainer,
                borderRadius: 8,
                border: `1px solid ${palette.borderSecondary}`,
                cursor: 'pointer',
                display: 'flex',
                flexDirection: 'column',
                alignItems: 'center',
                justifyContent: 'center',
                userSelect: 'none',
                transition: 'all 0.2s ease',
              }}
            >
              <LeftOutlined style={{ color: palette.gold, fontSize: 13 }} />
            </div>
          )}
        </div>

        {/* 图表与底部订单面板之间的居中折叠把手 (^ / v) */}
        <div
          style={{
            display: 'flex',
            justifyContent: 'center',
            alignItems: 'center',
            margin: '2px 0 6px',
          }}
        >
          <Button
            type="text"
            size="small"
            icon={
              isOrdersMinimized ? (
                <UpOutlined style={{ color: palette.gold, fontSize: 12 }} />
              ) : (
                <DownOutlined style={{ color: palette.gold, fontSize: 12 }} />
              )
            }
            onClick={() => setIsOrdersMinimized(!isOrdersMinimized)}
            style={{
              background: '#1a1a1a',
              border: `1px solid ${palette.borderSecondary}`,
              borderRadius: 12,
              height: 20,
              padding: '0 20px',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              cursor: 'pointer',
              color: palette.gold,
              boxShadow: '0 2px 4px rgba(0,0,0,0.3)',
              transition: 'all 0.2s ease',
            }}
            title={isOrdersMinimized ? '展开底部持仓与历史订单面板' : '折叠底部订单面板'}
          />
        </div>

        {/* 底部：持仓订单与历史平仓面板（组件化封装，支持在位折叠最小化） */}
        <TradingOrdersPanel
          ref={ordersPanelRef}
          accountId={accountId}
          positions={livePositions}
          positionsLoading={positionsLoading}
          minimized={isOrdersMinimized}
          onRefreshPositions={() => loadBaseline(true)}
        />

        {/* 策略运行参数微调弹窗（独立组件封装，由右上角齿轮触发） */}
        <StrategyParamsModal
          open={isParamsModalOpen}
          onClose={() => setIsParamsModalOpen(false)}
          accountId={accountId}
          params={strategyParams}
          onParamsSaved={(newParams) => setStrategyParams(newParams)}
        />
      </div>
    </Spin>
  );
};

export default TraderCockpitPage;
