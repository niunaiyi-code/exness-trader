import React, { useRef, useState } from 'react';
import { Button, message } from 'antd';
import {
  CaretUpOutlined,
  CaretDownOutlined,
} from '@ant-design/icons';
import { FormattedMt5Price, formatMt5Price } from '../lib/format';
import { palette } from '../theme';

interface QuickTradePanelProps {
  currentBid: number;
  currentAsk: number;
  symbol?: string;
  onTrade: (side: 'buy' | 'sell', lot: string) => Promise<void>;
  loading?: boolean;
}

const LOT_PRESETS = ['0.01', '0.05', '0.10', '0.50', '1.00', '2.00', '5.00'];
/** 一键模式防连击：同侧两次提交的最小间隔（毫秒） */
const DOUBLE_CLICK_GUARD_MS = 600;

/** MT5 风格顶部 SELL / BUY 标签条 */
const SideStrip: React.FC<{ label: 'SELL' | 'BUY' }> = ({ label }) => (
  <div
    style={{
      background: 'linear-gradient(180deg, #1d3fb6 0%, #132a7e 100%)',
      color: '#ffffff',
      fontWeight: 800,
      fontSize: 14,
      letterSpacing: 1.5,
      textAlign: 'center',
      padding: '5px 0',
      borderBottom: '1px solid rgba(255,255,255,0.2)',
    }}
  >
    {label}
  </div>
);

/** MT5 风格大字价格（点击即成交区） */
const PriceDigits: React.FC<{ fmt: FormattedMt5Price; active: boolean; disabled: boolean }> = ({
  fmt,
  active,
  disabled,
}) => (
  <div
    style={{
      display: 'flex',
      alignItems: 'baseline',
      justifyContent: 'center',
      padding: '12px 6px',
      background: active
        ? 'linear-gradient(180deg, #2b54db 0%, #1d3cb4 100%)'
        : 'linear-gradient(180deg, #132876 0%, #0a174a 100%)',
      cursor: disabled ? 'not-allowed' : 'pointer',
      transition: 'background 0.15s ease',
    }}
  >
    <span style={{ fontSize: 16, fontWeight: 600, color: '#c7d2fe' }}>{fmt.base}</span>
    <span
      style={{
        fontSize: 34,
        fontWeight: 900,
        color: '#ffffff',
        margin: '0 2px',
        lineHeight: 1,
        textShadow: '0 2px 4px rgba(0,0,0,0.6)',
      }}
    >
      {fmt.pips}
    </span>
    {fmt.pipette ? (
      <sup style={{ fontSize: 17, fontWeight: 800, color: '#ffffff', verticalAlign: 'super' }}>
        {fmt.pipette}
      </sup>
    ) : null}
  </div>
);

/** 可点击交易区域：点击即以现价直接执行交易（免弹窗与二次确认） */
const TradeArea: React.FC<{
  side: 'buy' | 'sell';
  disabled: boolean;
  onExecute: (side: 'buy' | 'sell') => void;
  children: React.ReactNode;
}> = ({ side, disabled, onExecute, children }) => (
  <div
    onClick={() => !disabled && onExecute(side)}
    style={{ cursor: disabled ? 'not-allowed' : 'pointer' }}
  >
    {children}
  </div>
);

export const QuickTradePanel: React.FC<QuickTradePanelProps> = ({
  currentBid,
  currentAsk,
  symbol: _symbol = 'XAUUSD',
  onTrade,
  loading = false,
}) => {
  const [lot, setLot] = useState<string>('1.00');
  const [activeSide, setActiveSide] = useState<'buy' | 'sell' | null>(null);
  // 防连击：记录最近一次提交（600ms 内同侧重复点击直接忽略）
  const lastSubmitRef = useRef<{ side: 'buy' | 'sell'; at: number } | null>(null);

  const bidFormatted = formatMt5Price(currentBid);
  const askFormatted = formatMt5Price(currentAsk);

  const adjustLot = (delta: number) => {
    const current = parseFloat(lot) || 0.01;
    const next = Math.max(0.01, Math.round((current + delta) * 100) / 100);
    setLot(next.toFixed(2));
  };

  const handleLotChange = (val: string) => {
    // 只允许数字和最多两位小数
    if (/^\d*\.?\d{0,2}$/.test(val)) {
      setLot(val);
    }
  };

  const executeOrder = async (side: 'buy' | 'sell') => {
    const numLot = parseFloat(lot);
    if (isNaN(numLot) || numLot <= 0) {
      message.error('请输入有效的手数 (例如 0.01 或 1.00)');
      return;
    }
    if (loading) return; // 已有请求在途
    const last = lastSubmitRef.current;
    if (last && last.side === side && Date.now() - last.at < DOUBLE_CLICK_GUARD_MS) return;
    lastSubmitRef.current = { side, at: Date.now() };
    setActiveSide(side);
    try {
      await onTrade(side, lot);
    } finally {
      setTimeout(() => setActiveSide(null), 300);
    }
  };

  const tradeAreaProps = {
    disabled: loading,
    onExecute: executeOrder,
  };

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
      {/* MT5 经典一键下单组件容器 */}
      <div
        style={{
          border: '1px solid #233e8b',
          borderRadius: 6,
          overflow: 'hidden',
          boxShadow: '0 4px 16px rgba(0,0,0,0.6)',
          userSelect: 'none',
        }}
      >
        {/* 顶部控制栏：SELL 标签 | 手数输入框 | BUY 标签 */}
        <div style={{ display: 'flex', background: '#0a174a' }}>
          <div style={{ flex: 1, borderRight: '1px solid #000', zIndex: 1 }}>
            <TradeArea side="sell" {...tradeAreaProps}>
              <SideStrip label="SELL" />
            </TradeArea>
          </div>

          {/* MT5 白色手数输入框与微调按钮 */}
          <div
            style={{
              width: 110,
              background: '#ffffff',
              display: 'flex',
              alignItems: 'center',
              border: '1px solid #000',
              padding: '0 2px',
              zIndex: 2,
            }}
          >
            <button
              onClick={() => adjustLot(-0.01)}
              style={{
                border: 'none',
                background: 'transparent',
                cursor: 'pointer',
                padding: '2px 4px',
                color: '#000000',
                fontSize: 11,
                lineHeight: 1,
              }}
              title="减少 0.01 手"
            >
              <CaretDownOutlined />
            </button>
            <input
              type="text"
              value={lot}
              onChange={(e) => handleLotChange(e.target.value)}
              style={{
                flex: 1,
                border: 'none',
                outline: 'none',
                textAlign: 'center',
                fontWeight: 800,
                fontSize: 14,
                color: '#000000',
                background: 'transparent',
                padding: '2px 0',
                minWidth: 0,
              }}
            />
            <button
              onClick={() => adjustLot(0.01)}
              style={{
                border: 'none',
                background: 'transparent',
                cursor: 'pointer',
                padding: '2px 4px',
                color: '#000000',
                fontSize: 11,
                lineHeight: 1,
              }}
              title="增加 0.01 手"
            >
              <CaretUpOutlined />
            </button>
          </div>

          <div style={{ flex: 1, borderLeft: '1px solid #000', zIndex: 1 }}>
            <TradeArea side="buy" {...tradeAreaProps}>
              <SideStrip label="BUY" />
            </TradeArea>
          </div>
        </div>

        {/* 底部价格行：左侧卖出价 (50%) | 右侧买入价 (50%) */}
        <div style={{ display: 'flex', borderTop: 'none' }}>
          <div style={{ flex: 1, borderRight: '1px solid #09133a' }}>
            <TradeArea side="sell" {...tradeAreaProps}>
              <PriceDigits fmt={bidFormatted} active={activeSide === 'sell'} disabled={loading} />
            </TradeArea>
          </div>
          <div style={{ flex: 1, borderLeft: '1px solid #1c3ba8' }}>
            <TradeArea side="buy" {...tradeAreaProps}>
              <PriceDigits fmt={askFormatted} active={activeSide === 'buy'} disabled={loading} />
            </TradeArea>
          </div>
        </div>
      </div>

      {/* 快捷手数选择标签 */}
      <div>
        <div style={{ fontSize: 12, color: palette.textSecondary, marginBottom: 6 }}>
          常用手数快捷选择:
        </div>
        <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
          {LOT_PRESETS.map((p) => {
            const isSelected = lot === p;
            return (
              <Button
                key={p}
                size="small"
                type={isSelected ? 'primary' : 'default'}
                onClick={() => setLot(p)}
                style={{
                  fontSize: 12,
                  fontWeight: isSelected ? 'bold' : 'normal',
                }}
              >
                {p}
              </Button>
            );
          })}
        </div>
      </div>
    </div>
  );
};
