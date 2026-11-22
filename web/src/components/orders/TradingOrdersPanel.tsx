import { useState, forwardRef, useImperativeHandle, useRef, useEffect } from 'react';
import { Card, Tabs, Tag, Button } from 'antd';
import {
  UnorderedListOutlined,
  HistoryOutlined,
  ReloadOutlined,
} from '@ant-design/icons';
import { LivePositionsTable, PositionItem } from './LivePositionsTable';
import { HistoryDealsTable, HistoryDealsTableRef, RawDeal } from './HistoryDealsTable';
import { palette } from '../../theme';

export interface TradingOrdersPanelRef {
  prependClosedDeals: (deals: RawDeal[]) => void;
  refreshHistory: () => Promise<void>;
}

interface TradingOrdersPanelProps {
  accountId: string;
  positions: PositionItem[];
  positionsLoading?: boolean;
  minimized?: boolean;
  onRefreshPositions?: () => Promise<void> | void;
}

export const TradingOrdersPanel = forwardRef<TradingOrdersPanelRef, TradingOrdersPanelProps>(
  (
    {
      accountId,
      positions,
      positionsLoading = false,
      minimized = false,
      onRefreshPositions,
    },
    ref,
  ) => {
    const [activeTab, setActiveTab] = useState<'open' | 'history'>('open');
    const historyRef = useRef<HistoryDealsTableRef>(null);

    // 激活对应 Tab 时主动重新拉取最新数据
    const handleTabChange = (key: string) => {
      const nextTab = key as 'open' | 'history';
      setActiveTab(nextTab);
      if (nextTab === 'open') {
        onRefreshPositions?.();
      } else if (nextTab === 'history') {
        void historyRef.current?.refresh();
      }
    };

    // 当面板从折叠状态恢复展开时，重新对齐当前激活 Tab 的最新数据
    const prevMinimizedRef = useRef(minimized);
    useEffect(() => {
      if (prevMinimizedRef.current && !minimized) {
        if (activeTab === 'open') {
          onRefreshPositions?.();
        } else if (activeTab === 'history') {
          void historyRef.current?.refresh();
        }
      }
      prevMinimizedRef.current = minimized;
    }, [minimized, activeTab, onRefreshPositions]);

    useImperativeHandle(
      ref,
      () => ({
        prependClosedDeals: (deals: RawDeal[]) => {
          historyRef.current?.prependDeals(deals);
        },
        refreshHistory: async () => {
          await historyRef.current?.refresh();
        },
      }),
      [],
    );

    return (
      <Card
        style={{
          display: minimized ? 'none' : 'block',
          background: palette.bgContainer,
          borderRadius: 8,
          transition: 'all 0.25s ease',
        }}
        styles={{
          header: {
            padding: '8px 16px',
            minHeight: 48,
            borderBottom: `1px solid ${palette.borderSecondary}`,
          },
          body: {
            padding: '8px 16px 16px',
          },
        }}
        title={
          <Tabs
            activeKey={activeTab}
            onChange={handleTabChange}
            tabBarStyle={{ margin: 0, borderBottom: 'none' }}
            items={[
              {
                key: 'open',
                label: (
                  <span style={{ fontSize: 14, fontWeight: 'bold' }}>
                    <UnorderedListOutlined /> 交易{' '}
                    <Tag color="blue">{positions.length} 单</Tag>
                  </span>
                ),
              },
              {
                key: 'history',
                label: (
                  <span style={{ fontSize: 14, fontWeight: 'bold' }}>
                    <HistoryOutlined /> 历史
                  </span>
                ),
              },
            ]}
          />
        }
        extra={
          activeTab === 'history' && !minimized ? (
            <Button
              type="text"
              size="small"
              icon={<ReloadOutlined />}
              onClick={() => void historyRef.current?.refresh()}
              style={{ color: palette.textSecondary, fontSize: 12 }}
            >
              刷新历史
            </Button>
          ) : null
        }
      >
        <div style={{ display: activeTab === 'open' ? 'block' : 'none' }}>
          <LivePositionsTable positions={positions} loading={positionsLoading} />
        </div>
        <div style={{ display: activeTab === 'history' ? 'block' : 'none' }}>
          <HistoryDealsTable
            ref={historyRef}
            accountId={accountId}
            active={activeTab === 'history'}
          />
        </div>
      </Card>
    );
  },
);

export default TradingOrdersPanel;
