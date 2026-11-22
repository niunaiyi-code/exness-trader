import React, { useEffect, useMemo, useRef, useState, useCallback } from 'react';
import { Row, Col, Card, Table, Tag, Alert, message, Tabs, Button, Form, Input, InputNumber, Modal, Spin } from 'antd';
import type { ColumnsType } from 'antd/es/table';
import { UnorderedListOutlined, HistoryOutlined, SettingOutlined, UnlockOutlined, SaveOutlined, ReloadOutlined } from '@ant-design/icons';
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

interface PositionItem {
  id: string;
  ticket: string;
  symbol: string;
  direction: 'BUY' | 'SELL';
  lots: number;
  openPrice: number;
  currentPrice: number;
  profit: number;
  openTime: string;
}

export interface HistoryItem {
  id: string;
  ticket: string;
  symbol: string;
  direction?: 'BUY' | 'SELL';
  lots: number;
  closePrice: number;
  profit: number;
  closeTime: string;
}

interface RawDeal {
  deal_id: string;
  order_id?: string | null;
  position_id?: string | null;
  type?: string | null;
  deal_type?: string | null;
  direction?: 'buy' | 'sell' | null;
  instrument?: string | null;
  volume?: string | null;
  price?: string | null;
  profit?: {
    amount: string;
    currency: string;
  } | null;
  create_time?: string | null;
  commission?: string | null;
}

interface RawDealsResponse {
  items?: { event_time: string; deal: RawDeal }[];
  next_cursor?: string | null;
  has_more?: boolean;
}

/**
 * 将 Exness 远端真实成交（Deals）转换为历史记录格式，
 * 按成交时间严格倒序（最新平仓排最前）返回真实订单明细。
 * 注意：后端已通过 deal_type=close 参数过滤，前端无需再过滤。
 */
function parseDealsHistory(deals: RawDeal[]): HistoryItem[] {
  console.log('[TraderCockpit] parseDealsHistory input:', deals.length, 'deals');
  if (deals.length > 0) {
    console.log('[TraderCockpit] First deal:', deals[0]);
  }

  if (deals.length === 0) return [];

  // 严格按成交时间降序（最新平仓排在最前）
  const sorted = [...deals].sort((a, b) => {
    const tA = a.create_time ? new Date(a.create_time).getTime() : 0;
    const tB = b.create_time ? new Date(b.create_time).getTime() : 0;
    return tB - tA;
  });

  return sorted.map((d) => {
    let t = '';
    if (d.create_time) {
      try {
        const dt = new Date(d.create_time);
        t = dt.toISOString().replace('T', ' ').slice(0, 19);
      } catch {
        t = d.create_time;
      }
    }
    return {
      id: `deal-${d.deal_id}`,
      ticket: d.position_id || d.deal_id,
      symbol: d.instrument || 'XAUUSD',
      direction: (d.direction || '').toLowerCase() === 'buy' ? 'BUY' : 'SELL',
      lots: parseFloat(d.volume || '0') || 0,
      closePrice: parseFloat(d.price || '0') || 0,
      profit: parseFloat(d.profit?.amount || '0') || 0,
      closeTime: t,
    };
  });
}

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

interface StrategyParamsData {
  account_id: string;
  initial_lot: string;
  grid_step: number;
  grid_step_add: number;
  grid_step_sec: number;
  lot_mult: string;
  tp_distance_usd: string;
  lock_trigger_usd: string;
  params_locked: boolean;
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
  const [historyRows, setHistoryRows] = useState<HistoryItem[]>([]);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [historyCursor, setHistoryCursor] = useState<string | null>(null);
  const [historyHasMore, setHistoryHasMore] = useState(true);
  const [activeTab, setActiveTab] = useState('open');
  const [isPaused, setIsPaused] = useState(false);
  const [isRunning, setIsRunning] = useState(false);
  const [actionLoading, setActionLoading] = useState(false);
  const [wsConnected, setWsConnected] = useState(false);
  const [utcClock, setUtcClock] = useState(() => new Date().toUTCString().slice(17, 25));

  // 策略运行参数状态
  const [strategyParams, setStrategyParams] = useState<StrategyParamsData | null>(null);
  const [, setParamsLoading] = useState(false);
  const [paramsSaving, setParamsSaving] = useState(false);
  const [isParamsModalOpen, setIsParamsModalOpen] = useState(false);
  const [paramsForm] = Form.useForm();

  const handleOpenParamsModal = () => {
    if (strategyParams) {
      paramsForm.setFieldsValue(strategyParams);
    }
    setIsParamsModalOpen(true);
  };

  const fetchStrategyParams = useCallback(async () => {
    if (!accountId) return;
    setParamsLoading(true);
    try {
      const resp = await api.get<StrategyParamsData>(`/accounts/${accountId}/params`);
      setStrategyParams(resp.data);
      paramsForm.setFieldsValue(resp.data);
    } catch (err) {
      console.warn('[cockpit] Failed to fetch strategy params:', err);
    } finally {
      setParamsLoading(false);
    }
  }, [accountId, paramsForm]);

  useEffect(() => {
    fetchStrategyParams();
  }, [fetchStrategyParams]);

  const handleSaveParams = async (values: Record<string, unknown>) => {
    if (!accountId) return;
    setParamsSaving(true);
    try {
      const resp = await api.put<StrategyParamsData>(`/accounts/${accountId}/params`, {
        initial_lot: String(values.initial_lot),
        grid_step: Number(values.grid_step),
        grid_step_add: Number(values.grid_step_add),
        grid_step_sec: Number(values.grid_step_sec),
        lot_mult: String(values.lot_mult),
        tp_distance_usd: String(values.tp_distance_usd),
        lock_trigger_usd: String(values.lock_trigger_usd),
      });
      message.success('策略参数已更新并实时生效至内存策略引擎！');
      setStrategyParams(resp.data);
      setIsParamsModalOpen(false);
    } catch (error) {
      message.error(apiErrorMessage(error, '参数更新失败'));
    } finally {
      setParamsSaving(false);
    }
  };

  // 用 ref 跟踪最新 bid 与账户币种，避免闭包过期（stale closure）
  const bidRef = useRef(0);
  const currencyRef = useRef('');
  const playedEventsRef = useRef<Set<string>>(new Set());

  // 历史平仓无限滚动相关 Ref，避免闭包过期和重复请求
  const historyTableContainerRef = useRef<HTMLDivElement>(null);
  const historyCursorRef = useRef<string | null>(null);
  const historyHasMoreRef = useRef(true);
  const historyLoadingRef = useRef(false);

  useEffect(() => {
    historyCursorRef.current = historyCursor;
  }, [historyCursor]);
  useEffect(() => {
    historyHasMoreRef.current = historyHasMore;
  }, [historyHasMore]);
  useEffect(() => {
    historyLoadingRef.current = historyLoading;
  }, [historyLoading]);

  // 真实平仓历史：从 Exness Open API 拉取真实已成交 deals，按时间倒序展示
  // 支持无限滚动加载，使用 cursor 分页
  const refreshDeals = useCallback(async (append = false) => {
    if (!accountId) return;
    if (append && !historyHasMoreRef.current) return; // 没有更多数据了
    if (append && historyLoadingRef.current) return;  // 防止重复加载

    setHistoryLoading(true);
    historyLoadingRef.current = true;
    try {
      // 适度增加单页 limit 到 20 条，确保首屏填满并产生正常纵向滚动条
      const params = new URLSearchParams({ limit: '20' });
      const currentCursor = historyCursorRef.current;
      if (append && currentCursor) {
        params.append('cursor', currentCursor);
      } else {
        params.append('deal_type', 'close');
      }

      const res = await api.get<RawDealsResponse>(
        `/accounts/${accountId}/history/deals?${params.toString()}`,
      );
      console.log('[TraderCockpit] API response:', {
        items: res.data?.items?.length,
        has_more: res.data?.has_more,
        next_cursor: res.data?.next_cursor,
        total_items: res.data?.items?.length
      });
      const items = res.data?.items || [];
      const rawDeals = items.map((it) => it.deal);
      console.log('[TraderCockpit] Raw deals:', rawDeals.length);
      const newRows = parseDealsHistory(rawDeals);
      console.log('[TraderCockpit] Parsed rows:', newRows.length);

      if (append) {
        setHistoryRows((prev) => {
          const existingIds = new Set(prev.map((r) => r.id));
          const uniqueNew = newRows.filter((r) => !existingIds.has(r.id));
          console.log('[TraderCockpit] Appending unique', uniqueNew.length, 'rows to', prev.length, 'rows');
          return [...prev, ...uniqueNew];
        });
      } else {
        console.log('[TraderCockpit] Setting initial rows');
        setHistoryRows(newRows);
      }

      const nextCursor = res.data?.next_cursor || null;
      const hasMore = res.data?.has_more ?? false;
      console.log('[TraderCockpit] Updated state:', { has_more: hasMore, cursor: nextCursor });
      setHistoryCursor(nextCursor);
      historyCursorRef.current = nextCursor;
      setHistoryHasMore(hasMore);
      historyHasMoreRef.current = hasMore;
    } catch (e) {
      console.warn('[cockpit] Load deals history error:', e);
    } finally {
      setHistoryLoading(false);
      historyLoadingRef.current = false;
    }
  }, [accountId]);

  // 触底检测与自动加载下一页
  const checkAndLoadMore = useCallback(() => {
    if (!historyHasMoreRef.current || historyLoadingRef.current) return;
    const tableBody = historyTableContainerRef.current?.querySelector('.ant-table-body');
    if (!tableBody) return;

    const { scrollTop, scrollHeight, clientHeight } = tableBody;
    // 距离底部 60px 内时自动触发滚动加载
    if (scrollHeight - scrollTop - clientHeight < 60) {
      void refreshDeals(true);
    }
  }, [refreshDeals]);

  // 监听表格内容滚动及视口不足时自动补充加载
  useEffect(() => {
    const container = historyTableContainerRef.current;
    if (!container) return;

    const tableBody = container.querySelector('.ant-table-body');
    if (!tableBody) return;

    const onScroll = () => {
      checkAndLoadMore();
    };

    tableBody.addEventListener('scroll', onScroll, { passive: true });

    // 若当前条目高度不足以产生滚动条且后端仍有更多数据，自动补充加载下一页
    if (historyHasMoreRef.current && !historyLoadingRef.current && historyRows.length > 0) {
      if (tableBody.scrollHeight <= tableBody.clientHeight + 10) {
        void refreshDeals(true);
      }
    }

    return () => {
      tableBody.removeEventListener('scroll', onScroll);
    };
  }, [historyRows.length, activeTab, checkAndLoadMore, refreshDeals]);

  // 切换至历史 Tab 时，若尚无记录则自动触发刷新
  useEffect(() => {
    if (activeTab === 'history' && historyRows.length === 0 && !historyLoadingRef.current) {
      void refreshDeals(false);
    }
  }, [activeTab, historyRows.length, refreshDeals]);

  useEffect(() => {
    refreshDeals();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [accountId]); // 只在 accountId 变化时重新加载，避免无限循环

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

  // 账户初始基线（余额/净值/保证金 + 持仓 + 状态）。页面加载或WS重连时仅主动请求一次，零定时轮询。
  const loadBaseline = useCallback(async () => {
    if (!accountId) return;
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
        void refreshDeals();
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
                  const parsed = parseDealsHistory(closeDeals);
                  setHistoryRows((prev) => {
                    const existingIds = new Set(prev.map((d) => d.ticket));
                    const toAdd = parsed.filter((d) => !existingIds.has(d.ticket));
                    return [...toAdd, ...prev];
                  });

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
  }, [accountId, loadBaseline, refreshDeals]);

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
      void refreshDeals();
    } catch (e) {
      message.error(apiErrorMessage(e, `指令 [${actionType}] 执行失败`));
    } finally {
      if (actionType === 'pause') setIsPaused(true);
      else if (actionType === 'resume') setIsPaused(false);
      setActionLoading(false);
    }
  };

  const historySummary = useMemo(() => {
    const totalProfit = historyRows.reduce((acc, r) => acc + (r.profit || 0), 0);
    return {
      orderCount: historyRows.length,
      totalProfit,
    };
  }, [historyRows]);

  const columns = [
    { title: '订单号 (Ticket)', dataIndex: 'ticket', key: 'ticket' },
    { title: '品种', dataIndex: 'symbol', key: 'symbol' },
    {
      title: '方向',
      dataIndex: 'direction',
      key: 'direction',
      render: (d: string) => <Tag color={d === 'BUY' ? 'blue' : 'volcano'}>{d}</Tag>,
    },
    { title: '手数', dataIndex: 'lots', key: 'lots', render: (v: number) => v.toFixed(2) },
    { title: '开仓均价', dataIndex: 'openPrice', key: 'openPrice', render: (v: number) => `$${v.toFixed(2)}` },
    {
      title: '当前市价',
      dataIndex: 'currentPrice',
      key: 'currentPrice',
      render: (v: number) => (v > 0 ? `$${v.toFixed(2)}` : '—'),
    },
    {
      title: '浮动盈亏',
      dataIndex: 'profit',
      key: 'profit',
      render: (v: number) => (
        <span style={{ color: v >= 0 ? palette.profit : palette.loss, fontWeight: 'bold' }}>
          {v >= 0 ? `+$${v.toFixed(2)}` : `-$${Math.abs(v).toFixed(2)}`}
        </span>
      ),
    },
    { title: '开仓时间 (UTC)', dataIndex: 'openTime', key: 'openTime' },
  ];

  const historyColumns: ColumnsType<any> = React.useMemo(() => [
    { title: '订单号', dataIndex: 'ticket', key: 'ticket', width: 120 },
    { title: '品种', dataIndex: 'symbol', key: 'symbol', width: 100 },
    {
      title: '方向',
      dataIndex: 'direction',
      key: 'direction',
      width: 80,
      render: (d: string) => <Tag color={d === 'BUY' ? 'blue' : 'volcano'}>{d}</Tag>,
    },
    {
      title: '平仓手数',
      dataIndex: 'lots',
      key: 'lots',
      width: 100,
      render: (v: number) => (v || 0).toFixed(2),
    },
    {
      title: '平仓价格',
      dataIndex: 'closePrice',
      key: 'closePrice',
      width: 120,
      render: (v: number) => (v > 0 ? `$${v.toFixed(2)}` : '—'),
    },
    {
      title: '已实现盈亏',
      dataIndex: 'profit',
      key: 'profit',
      width: 140,
      render: (v: number) => {
        const isPos = (v || 0) >= 0;
        return (
          <span
            style={{
              color: isPos ? palette.profit : palette.loss,
              fontWeight: 'bold',
            }}
          >
            {isPos ? `+$${(v || 0).toFixed(2)}` : `-$${Math.abs(v || 0).toFixed(2)}`}
          </span>
        );
      },
    },
    { title: '平仓时间 (UTC)', dataIndex: 'closeTime', key: 'closeTime', width: 180 },
  ], [palette.profit, palette.loss]);

  return (
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

      {/* 中部：走势图 + 快捷操作 */}
      <Row gutter={12} style={{ marginBottom: 12 }}>
        <Col xs={24} lg={17}>
          <PriceChart currentBid={currentBid} currentAsk={currentAsk} height={480} />
        </Col>
        <Col xs={24} lg={7}>
          <QuickActions
            isPaused={isPaused}
            isRunning={isRunning}
            onAction={handleQuickAction}
            loading={actionLoading}
            currentBid={currentBid}
            currentAsk={currentAsk}
            symbol="XAUUSD"
          />
        </Col>
      </Row>

      {/* 实时持仓订单与平仓历史 Tabs (含平仓小计) */}
      <Card
        style={{ background: palette.bgContainer, borderRadius: 8 }}
        styles={{ body: { padding: '8px 16px 16px' } }}
      >
        <Tabs
          activeKey={activeTab}
          onChange={setActiveTab}
          items={[
            {
              key: 'open',
              label: (
                <span style={{ fontSize: 14, fontWeight: 'bold' }}>
                  <UnorderedListOutlined /> 交易 <Tag color="blue">{positions.length} 单</Tag>
                </span>
              ),
              children: (
                <Table
                  dataSource={livePositions}
                  columns={columns}
                  rowKey="id"
                  pagination={false}
                  size="small"
                />
              ),
            },
            {
              key: 'history',
              label: (
                <span style={{ fontSize: 14, fontWeight: 'bold' }}>
                  <HistoryOutlined /> 历史{' '}
                  <Tag color="green">{historySummary.orderCount} 笔</Tag>
                </span>
              ),
              children: (
                <div ref={historyTableContainerRef} onScrollCapture={checkAndLoadMore}>
                  <div
                    style={{
                      display: 'flex',
                      alignItems: 'center',
                      justifyContent: 'space-between',
                      padding: '6px 12px',
                      background: '#1a1a1a',
                      borderRadius: 4,
                      marginBottom: 8,
                      border: '1px solid #2a2a2a',
                    }}
                  >
                    <span style={{ color: palette.textSecondary, fontSize: 12 }}>
                      💡 Exness 真实历史平仓成交（按平仓时间倒序）
                    </span>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 16 }}>
                      <span style={{ fontSize: 13, color: palette.textSecondary }}>
                        平仓总单数: <b style={{ color: '#faad14' }}>{historySummary.orderCount} 笔</b>
                      </span>
                      <span style={{ fontSize: 13, color: palette.textSecondary }}>
                        已实现累计总盈利:{' '}
                        <b style={{ color: historySummary.totalProfit >= 0 ? palette.profit : palette.loss }}>
                          {historySummary.totalProfit >= 0
                            ? `+$${historySummary.totalProfit.toFixed(2)}`
                            : `-$${Math.abs(historySummary.totalProfit).toFixed(2)}`}
                        </b>
                      </span>
                      <Button
                        size="small"
                        icon={<ReloadOutlined spin={historyLoading} />}
                        onClick={() => refreshDeals(false)}
                        loading={historyLoading}
                        title="重新加载最新平仓历史"
                      >
                        刷新
                      </Button>
                    </div>
                  </div>
                  <Table
                    dataSource={historyRows}
                    columns={historyColumns}
                    rowKey="id"
                    pagination={false}
                    size="small"
                    loading={historyLoading && historyRows.length === 0}
                    scroll={{ y: 380, x: 900 }}
                    footer={() => {
                      if (historyLoading && historyRows.length > 0) {
                        return (
                          <div style={{ textAlign: 'center', padding: '8px 0', color: palette.textSecondary, display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 8 }}>
                            <Spin size="small" />
                            <span>正在滚动加载更多历史平仓记录...</span>
                          </div>
                        );
                      }
                      if (historyHasMore && historyRows.length > 0) {
                        return (
                          <div style={{ textAlign: 'center', padding: '6px 0', display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 12 }}>
                            <span style={{ color: palette.textSecondary, fontSize: 12 }}>向下滚动自动加载</span>
                            <Button
                              size="small"
                              type="link"
                              onClick={() => refreshDeals(true)}
                              loading={historyLoading}
                            >
                              点击加载更多
                            </Button>
                          </div>
                        );
                      }
                      if (!historyHasMore && historyRows.length > 0) {
                        return <div style={{ textAlign: 'center', padding: '10px 0', color: palette.textSecondary }}>已加载全部 {historyRows.length} 条历史平仓记录</div>;
                      }
                      return null;
                    }}
                  />
                </div>
              ),
            },
          ]}
        />
      </Card>

      {/* 策略运行参数微调弹窗（由右上角齿轮触发） */}
      <Modal
        title={
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <SettingOutlined style={{ color: palette.gold, fontSize: 18 }} />
            <span style={{ color: '#ffffff', fontWeight: 'bold' }}>策略运行参数设置</span>
            <Tag color="success" icon={<UnlockOutlined />}>允许自主调参</Tag>
          </div>
        }
        open={isParamsModalOpen}
        onCancel={() => setIsParamsModalOpen(false)}
        footer={null}
        width={720}
        destroyOnClose
      >
        <Alert
          type="info"
          showIcon
          icon={<UnlockOutlined />}
          message="自主微调模式已开放"
          description="当前账户已由管理员开启调参权限。调整参数并点击【保存并热应用到实盘】后，将实时热更新到实盘内存策略引擎中执行。"
          style={{ marginBottom: 20 }}
        />

        <Form
          form={paramsForm}
          layout="vertical"
          onFinish={handleSaveParams}
          initialValues={strategyParams || undefined}
        >
          <Row gutter={24}>
            <Col xs={24} sm={12} md={6}>
              <Form.Item
                label="初始首单手数"
                name="initial_lot"
                rules={[{ required: true, message: '请输入初始手数' }]}
                tooltip="每轮首个方向开单的默认建仓手数（例如 0.01）"
              >
                <Input placeholder="0.01" />
              </Form.Item>
            </Col>
            <Col xs={24} sm={12} md={6}>
              <Form.Item
                label="基础加仓点距 (points)"
                name="grid_step"
                rules={[{ required: true, message: '请输入加仓点距' }]}
                tooltip="逆势加仓的基础点数间距（如 300 点 = 30 pips）"
              >
                <InputNumber min={50} max={5000} style={{ width: '100%' }} />
              </Form.Item>
            </Col>
            <Col xs={24} sm={12} md={6}>
              <Form.Item
                label="间距阶梯递增 (points)"
                name="grid_step_add"
                rules={[{ required: true, message: '请输入间距递增' }]}
                tooltip="每增加一层网格时，间距递增的点数（如 75 点）"
              >
                <InputNumber min={0} max={2000} style={{ width: '100%' }} />
              </Form.Item>
            </Col>
            <Col xs={24} sm={12} md={6}>
              <Form.Item
                label="去抖冷却间隔 (秒)"
                name="grid_step_sec"
                rules={[{ required: true, message: '请输入去抖秒数' }]}
                tooltip="防止急速毛刺连续下多单的最小安全冷却时间，默认 15s"
              >
                <InputNumber min={1} max={3600} style={{ width: '100%' }} />
              </Form.Item>
            </Col>
          </Row>

          <Row gutter={24}>
            <Col xs={24} sm={12} md={8}>
              <Form.Item
                label="加仓倍增系数"
                name="lot_mult"
                rules={[{ required: true, message: '请输入倍增系数' }]}
                tooltip="网格加仓倍率，如 1.45 倍"
              >
                <Input placeholder="1.45" />
              </Form.Item>
            </Col>
            <Col xs={24} sm={12} md={8}>
              <Form.Item
                label="单层止盈距离 (USD)"
                name="tp_distance_usd"
                rules={[{ required: true, message: '请输入止盈距离' }]}
                tooltip="综合持仓均价脱离成本区达到此美元距离时触发整体止盈离场"
              >
                <Input placeholder="1.00" />
              </Form.Item>
            </Col>
            <Col xs={24} sm={12} md={8}>
              <Form.Item
                label="单边对冲锁仓阈值 (USD)"
                name="lock_trigger_usd"
                rules={[{ required: true, message: '请输入锁仓阈值' }]}
                tooltip="单侧浮亏达到此金额时自动触发严格对冲全锁仓"
              >
                <Input placeholder="1000.00" />
              </Form.Item>
            </Col>
          </Row>

          <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 12, marginTop: 12 }}>
            <Button onClick={() => setIsParamsModalOpen(false)}>取消</Button>
            <Button
              type="primary"
              htmlType="submit"
              icon={<SaveOutlined />}
              loading={paramsSaving}
            >
              保存并热应用到实盘
            </Button>
          </div>
        </Form>
      </Modal>
    </div>
  );
};

export default TraderCockpitPage;
