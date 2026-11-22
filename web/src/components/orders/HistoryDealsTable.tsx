import { useEffect, useMemo, useRef, useState, useCallback, useImperativeHandle, forwardRef } from 'react';
import { Table, Tag, Spin } from 'antd';
import type { ColumnsType } from 'antd/es/table';
import { api } from '../../lib/api';
import { palette } from '../../theme';

export interface RawDeal {
  deal_id: string;
  position_id?: string;
  instrument?: string;
  direction?: string;
  volume?: string;
  price?: string;
  profit?: { amount?: string; currency?: string };
  type?: string;
  deal_type?: string;
  create_time?: string;
}

export interface HistoryItem {
  id: string;
  ticket: string;
  symbol: string;
  direction: 'BUY' | 'SELL';
  lots: number;
  closePrice: number;
  profit: number;
  closeTime: string;
}

interface RawDealsResponse {
  items: { deal: RawDeal }[];
  next_cursor?: string;
  has_more?: boolean;
}

export function parseDealsHistory(deals: RawDeal[]): HistoryItem[] {
  if (deals.length === 0) return [];

  // 严格按成交时间降序（最新平仓排最前）
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

export interface HistoryDealsTableRef {
  refresh: () => Promise<void>;
  prependDeals: (deals: RawDeal[]) => void;
}

interface HistoryDealsTableProps {
  accountId: string;
  active?: boolean;
}

export const HistoryDealsTable = forwardRef<HistoryDealsTableRef, HistoryDealsTableProps>(
  ({ accountId, active = true }, ref) => {
    const [historyRows, setHistoryRows] = useState<HistoryItem[]>([]);
    const [historyLoading, setHistoryLoading] = useState(false);
    const [historyCursor, setHistoryCursor] = useState<string | null>(null);
    const [historyHasMore, setHistoryHasMore] = useState(true);

    const containerRef = useRef<HTMLDivElement>(null);
    const cursorRef = useRef<string | null>(null);
    const hasMoreRef = useRef(true);
    const loadingRef = useRef(false);

    useEffect(() => {
      cursorRef.current = historyCursor;
    }, [historyCursor]);
    useEffect(() => {
      hasMoreRef.current = historyHasMore;
    }, [historyHasMore]);
    useEffect(() => {
      loadingRef.current = historyLoading;
    }, [historyLoading]);

    // 拉取历史平仓数据
    const refreshDeals = useCallback(
      async (append = false) => {
        if (!accountId) return;
        if (append && !hasMoreRef.current) return;
        if (append && loadingRef.current) return;

        setHistoryLoading(true);
        loadingRef.current = true;
        try {
          const params = new URLSearchParams({ limit: '20' });
          const cur = cursorRef.current;
          if (append && cur) {
            params.append('cursor', cur);
          } else {
            params.append('deal_type', 'close');
          }

          const res = await api.get<RawDealsResponse>(
            `/accounts/${accountId}/history/deals?${params.toString()}`,
          );
          const items = res.data?.items || [];
          const rawDeals = items.map((it) => it.deal);
          const newRows = parseDealsHistory(rawDeals);

          if (append) {
            setHistoryRows((prev) => {
              const existingIds = new Set(prev.map((r) => r.id));
              const uniqueNew = newRows.filter((r) => !existingIds.has(r.id));
              return [...prev, ...uniqueNew];
            });
          } else {
            setHistoryRows(newRows);
          }

          const nextCursor = res.data?.next_cursor || null;
          const hasMore = res.data?.has_more ?? false;
          setHistoryCursor(nextCursor);
          cursorRef.current = nextCursor;
          setHistoryHasMore(hasMore);
          hasMoreRef.current = hasMore;
        } catch (e) {
          console.warn('[HistoryDealsTable] Load deals error:', e);
        } finally {
          setHistoryLoading(false);
          loadingRef.current = false;
        }
      },
      [accountId],
    );

    useImperativeHandle(
      ref,
      () => ({
        refresh: () => refreshDeals(false),
        prependDeals: (deals: RawDeal[]) => {
          const parsed = parseDealsHistory(deals);
          setHistoryRows((prev) => {
            const existingIds = new Set(prev.map((d) => d.id));
            const toAdd = parsed.filter((d) => !existingIds.has(d.id));
            return [...toAdd, ...prev];
          });
        },
      }),
      [refreshDeals],
    );

    // 触底检测与自动加载下一页
    const checkAndLoadMore = useCallback(() => {
      if (!hasMoreRef.current || loadingRef.current) return;
      const tableBody = containerRef.current?.querySelector('.ant-table-body');
      if (!tableBody) return;

      const { scrollTop, scrollHeight, clientHeight } = tableBody;
      if (scrollHeight - scrollTop - clientHeight < 60) {
        void refreshDeals(true);
      }
    }, [refreshDeals]);

    // 监听内部滚动与首屏自动填充满视口
    useEffect(() => {
      const container = containerRef.current;
      if (!container) return;

      const tableBody = container.querySelector('.ant-table-body');
      if (!tableBody) return;

      const onScroll = () => {
        checkAndLoadMore();
      };

      tableBody.addEventListener('scroll', onScroll, { passive: true });

      if (hasMoreRef.current && !loadingRef.current && historyRows.length > 0) {
        if (tableBody.scrollHeight <= tableBody.clientHeight + 10) {
          void refreshDeals(true);
        }
      }

      return () => {
        tableBody.removeEventListener('scroll', onScroll);
      };
    }, [historyRows.length, active, checkAndLoadMore, refreshDeals]);

    // 初次挂载或切入激活该 Tab 时重新拉取最新成交数据
    const prevActiveRef = useRef(false);
    useEffect(() => {
      if (active && (!prevActiveRef.current || historyRows.length === 0)) {
        if (!loadingRef.current) {
          void refreshDeals(false);
        }
      }
      prevActiveRef.current = active;
    }, [active, historyRows.length, refreshDeals]);

    const columns: ColumnsType<HistoryItem> = useMemo(
      () => [
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
      ],
      [],
    );

    return (
      <div ref={containerRef} onScrollCapture={checkAndLoadMore}>
        <Table
          dataSource={historyRows}
          columns={columns}
          rowKey="id"
          pagination={false}
          size="small"
          loading={historyLoading && historyRows.length === 0}
          scroll={{ y: 380, x: 900 }}
          locale={{ emptyText: '暂无历史平仓记录' }}
          footer={
            historyRows.length === 0
              ? undefined
              : () => {
                  if (historyLoading) {
                    return (
                      <div
                        style={{
                          textAlign: 'center',
                          padding: '6px 0',
                          color: palette.textSecondary,
                          display: 'flex',
                          alignItems: 'center',
                          justifyContent: 'center',
                          gap: 8,
                          fontSize: 12,
                        }}
                      >
                        <Spin size="small" />
                        <span>正在滚动加载更多历史平仓记录...</span>
                      </div>
                    );
                  }
                  if (!historyHasMore) {
                    return (
                      <div
                        style={{
                          textAlign: 'center',
                          padding: '6px 0',
                          color: palette.textSecondary,
                          fontSize: 12,
                        }}
                      >
                        — 已加载全部 {historyRows.length} 笔记录，没有更多了 —
                      </div>
                    );
                  }
                  return (
                    <div
                      style={{
                        textAlign: 'center',
                        padding: '6px 0',
                        color: palette.textSecondary,
                        fontSize: 12,
                      }}
                    >
                      向下滚动自动加载更多
                    </div>
                  );
                }
          }
        />
      </div>
    );
  },
);

export default HistoryDealsTable;
