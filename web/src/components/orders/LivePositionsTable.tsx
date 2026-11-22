import React from 'react';
import { Table, Tag } from 'antd';
import type { ColumnsType } from 'antd/es/table';
import { palette } from '../../theme';

export interface PositionItem {
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

interface LivePositionsTableProps {
  positions: PositionItem[];
  loading?: boolean;
}

export const LivePositionsTable: React.FC<LivePositionsTableProps> = ({
  positions,
  loading = false,
}) => {
  const columns: ColumnsType<PositionItem> = [
    { title: '订单号 (Ticket)', dataIndex: 'ticket', key: 'ticket', width: 140 },
    { title: '品种', dataIndex: 'symbol', key: 'symbol', width: 100 },
    {
      title: '方向',
      dataIndex: 'direction',
      key: 'direction',
      width: 90,
      render: (d: string) => <Tag color={d === 'BUY' ? 'blue' : 'volcano'}>{d}</Tag>,
    },
    {
      title: '手数',
      dataIndex: 'lots',
      key: 'lots',
      width: 100,
      render: (v: number) => v.toFixed(2),
    },
    {
      title: '开仓均价',
      dataIndex: 'openPrice',
      key: 'openPrice',
      width: 120,
      render: (v: number) => `$${v.toFixed(2)}`,
    },
    {
      title: '当前市价',
      dataIndex: 'currentPrice',
      key: 'currentPrice',
      width: 120,
      render: (v: number) => (v > 0 ? `$${v.toFixed(2)}` : '—'),
    },
    {
      title: '浮动盈亏',
      dataIndex: 'profit',
      key: 'profit',
      width: 130,
      render: (v: number) => (
        <span style={{ color: v >= 0 ? palette.profit : palette.loss, fontWeight: 'bold' }}>
          {v >= 0 ? `+$${v.toFixed(2)}` : `-$${Math.abs(v).toFixed(2)}`}
        </span>
      ),
    },
    { title: '开仓时间 (UTC)', dataIndex: 'openTime', key: 'openTime', width: 160 },
  ];

  return (
    <Table
      dataSource={positions}
      columns={columns}
      rowKey="id"
      pagination={false}
      size="small"
      loading={loading}
      scroll={{ x: 800 }}
      locale={{ emptyText: '当前无持仓订单' }}
    />
  );
};

export default LivePositionsTable;
