import React from 'react';
import { Tag, Space, Tooltip, Button } from 'antd';
import {
  AlertOutlined,
  ClockCircleOutlined,
  ThunderboltOutlined,
  SettingOutlined,
} from '@ant-design/icons';
import { palette } from '../theme';

export interface HudBarProps {
  accountId: string;
  /** 锁仓状态；后端暂未下发该字段，不传时不显示锁仓标签 */
  isLocked?: boolean;
  /** 重大事件避险窗口（新闻日历链路接通后生效） */
  isNewsAvoid?: boolean;
  isRunning: boolean;
  /** 行情 WS 连线状态 */
  wsConnected?: boolean;
  utcTime?: string;
  balance?: number;
  liveEquity?: number;
  liveFloatingPnl?: number;
  usedMargin?: number;
  liveFreeMargin?: number;
  liveMarginLevel?: number;
  /** 是否允许调参（未锁定时为 true，显示齿轮） */
  canEditParams?: boolean;
  /** 点击调参齿轮回调 */
  onOpenParams?: () => void;
}

export const HudBar: React.FC<HudBarProps> = ({
  accountId,
  isLocked,
  isNewsAvoid,
  isRunning,
  wsConnected: _wsConnected,
  utcTime = `${new Date().toUTCString().slice(17, 25)} UTC`,
  balance = 0,
  liveEquity = 0,
  liveFloatingPnl = 0,
  usedMargin = 0,
  liveFreeMargin = 0,
  liveMarginLevel = 0,
  canEditParams,
  onOpenParams,
}) => {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        padding: '6px 12px',
        background: '#161616',
        borderRadius: 6,
        border: `1px solid ${palette.borderSecondary}`,
        marginBottom: 10,
        flexWrap: 'nowrap',
        overflowX: 'auto',
        gap: 10,
        whiteSpace: 'nowrap',
      }}
    >
      {/* 1. 左侧：策略标识、账号与状态徽章 */}
      <Space direction="horizontal" size={6} wrap={false} align="center" style={{ flexShrink: 0 }}>
        <span style={{ fontSize: 14, fontWeight: 'bold', color: palette.gold, whiteSpace: 'nowrap' }}>
          👑 金色传说 · 驾驶舱
        </span>
        <Tag color="cyan" style={{ fontSize: 12, padding: '0 5px', margin: 0 }}>
          账户: {accountId}
        </Tag>

        {isLocked ? (
          <Tag color="error" icon={<AlertOutlined />} style={{ fontSize: 12, padding: '0 5px', margin: 0 }}>
            [已锁仓]
          </Tag>
        ) : isNewsAvoid ? (
          <Tag color="warning" icon={<ClockCircleOutlined />} style={{ fontSize: 12, padding: '0 5px', margin: 0 }}>
            [重大事件避险]
          </Tag>
        ) : isRunning ? (
          <Tag color="success" icon={<ThunderboltOutlined />} style={{ fontSize: 12, padding: '0 5px', margin: 0 }}>
            [实盘运行中]
          </Tag>
        ) : (
          <Tag color="default" style={{ fontSize: 12, padding: '0 5px', margin: 0 }}>
            [已停止 / 待命]
          </Tag>
        )}
      </Space>

      {/* 2. 中部：六大核心资产资金指标（无缝融合在同一单行） */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexShrink: 0, whiteSpace: 'nowrap' }}>
        <div style={{ width: 1, height: 16, background: '#333' }} />
        <div>
          <span style={{ color: palette.textSecondary, fontSize: 12, marginRight: 4 }}>账户余额:</span>
          <b style={{ color: '#ffffff', fontSize: 13 }}>
            ${balance.toLocaleString(undefined, { minimumFractionDigits: 2 })}
          </b>
        </div>
        <div style={{ width: 1, height: 16, background: '#333' }} />
        <div>
          <span style={{ color: palette.textSecondary, fontSize: 12, marginRight: 4 }}>当前净值:</span>
          <b style={{ color: '#ffd700', fontSize: 13, fontWeight: 'bold' }}>
            ${liveEquity.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
          </b>
        </div>
        <div style={{ width: 1, height: 16, background: '#333' }} />
        <div>
          <span style={{ color: palette.textSecondary, fontSize: 12, marginRight: 4 }}>浮动盈亏:</span>
          <b style={{ color: liveFloatingPnl >= 0 ? palette.profit : palette.loss, fontSize: 13, fontWeight: 'bold' }}>
            {liveFloatingPnl >= 0 ? `+$${liveFloatingPnl.toFixed(2)}` : `-$${Math.abs(liveFloatingPnl).toFixed(2)}`}
          </b>
        </div>
        <div style={{ width: 1, height: 16, background: '#333' }} />
        <div>
          <span style={{ color: palette.textSecondary, fontSize: 12, marginRight: 4 }}>已用保证金:</span>
          <b style={{ color: '#d9d9d9', fontSize: 13 }}>${usedMargin.toFixed(2)}</b>
        </div>
        <div style={{ width: 1, height: 16, background: '#333' }} />
        <div>
          <span style={{ color: palette.textSecondary, fontSize: 12, marginRight: 4 }}>可用保证金:</span>
          <b style={{ color: palette.profit, fontSize: 13 }}>${liveFreeMargin.toFixed(2)}</b>
        </div>
        <div style={{ width: 1, height: 16, background: '#333' }} />
        <div>
          <span style={{ color: palette.textSecondary, fontSize: 12, marginRight: 4 }}>保证金比例:</span>
          <b style={{ color: palette.profit, fontSize: 13 }}>
            {usedMargin > 0 ? `${liveMarginLevel.toFixed(1)}%` : '∞'}
          </b>
        </div>
      </div>

      {/* 3. 右侧：UTC 时钟 + 策略调参齿轮（能调就显示，不能就隐藏） */}
      <Space direction="horizontal" size={8} wrap={false} align="center" style={{ flexShrink: 0 }}>
        <div style={{ width: 1, height: 16, background: '#333' }} />
        <span style={{ color: palette.textSecondary, fontSize: 12 }}>
          时钟: <b style={{ color: '#d9d9d9', fontSize: 13 }}>{utcTime}</b>
        </span>
        {canEditParams && (
          <>
            <div style={{ width: 1, height: 16, background: '#333' }} />
            <Tooltip title="策略运行参数设置 (已获调参授权)">
              <Button
                type="text"
                size="small"
                icon={<SettingOutlined style={{ fontSize: 16, color: palette.gold }} />}
                onClick={onOpenParams}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  padding: '2px 6px',
                  borderRadius: 4,
                  border: '1px solid #3a3a3a',
                  background: '#202020',
                  cursor: 'pointer',
                  color: palette.gold,
                }}
              />
            </Tooltip>
          </>
        )}
      </Space>
    </div>
  );
};
