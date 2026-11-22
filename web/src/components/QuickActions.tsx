import React, { useState, useEffect } from 'react';
import { Card, Button, Segmented, Tag, Popconfirm, message } from 'antd';
import {
  LockOutlined,
  StopOutlined,
  CheckCircleOutlined,
  CloseCircleOutlined,
  ArrowUpOutlined,
  ArrowDownOutlined,
  PauseCircleOutlined,
  PlayCircleOutlined,
} from '@ant-design/icons';
import { QuickTradePanel } from './QuickTradePanel';

interface QuickActionsProps {
  isPaused: boolean;
  isRunning?: boolean;
  onAction: (actionType: string, payload?: Record<string, unknown>) => Promise<void>;
  loading?: boolean;
  currentBid?: number;
  currentAsk?: number;
  symbol?: string;
}

export const QuickActions: React.FC<QuickActionsProps> = ({
  isPaused,
  isRunning = true,
  onAction,
  loading = false,
  currentBid = 0,
  currentAsk = 0,
  symbol = 'XAUUSD',
}) => {
  const [mode, setMode] = useState<'quick_trade' | 'emergency'>('quick_trade');

  // 快捷键：按 "Q" 或 "q" 在快捷交易与紧急控制台之间切换
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
        e.preventDefault();
        setMode((prev) => {
          const next = prev === 'quick_trade' ? 'emergency' : 'quick_trade';
          message.info({
            content: `已切换至：${next === 'quick_trade' ? '⚡ 快速交易面板' : '🚨 应急风控控制台'} (按 Q 切换)`,
            key: 'q_switch_notice',
            duration: 1.5,
          });
          return next;
        });
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, []);

  // 结果提示（成功/失败文案）由父级 handleQuickAction 统一弹出，此处只负责派发
  const handleConfirm = (action: string) => {
    void onAction(action);
  };

  const handleTrade = async (side: 'buy' | 'sell', lot: string) => {
    const price = side === 'buy' ? currentAsk : currentBid;
    await onAction(side, { lot, price, symbol });
  };

  return (
    <Card
      title={
        <Segmented
          className="quick-actions-mode-tabs"
          value={mode}
          onChange={(val) => setMode(val as 'quick_trade' | 'emergency')}
          options={[
            {
              label: (
                <span className="quick-actions-mode-tab-label">
                  ⚡ 快速交易
                  <Tag
                    color="blue"
                    style={{
                      fontSize: 10,
                      margin: 0,
                      padding: '0 4px',
                      lineHeight: '16px',
                      fontWeight: 'bold',
                    }}
                  >
                    Q
                  </Tag>
                </span>
              ),
              value: 'quick_trade',
            },
            {
              label: (
                <span className="quick-actions-mode-tab-label">
                  🚨 应急控制
                  <Tag
                    color="orange"
                    style={{
                      fontSize: 10,
                      margin: 0,
                      padding: '0 4px',
                      lineHeight: '16px',
                      fontWeight: 'bold',
                    }}
                  >
                    Q
                  </Tag>
                </span>
              ),
              value: 'emergency',
            },
          ]}
          style={{ background: '#141414', border: '1px solid #333', width: '100%' }}
        />
      }
      style={{
        background: '#1f1f1f',
        borderRadius: 8,
        height: '100%',
        display: 'flex',
        flexDirection: 'column',
      }}
      styles={{
        body: {
          flex: 1,
          display: 'flex',
          flexDirection: 'column',
          justifyContent: 'space-between',
        },
      }}
    >
      {mode === 'quick_trade' ? (
        <QuickTradePanel
          currentBid={currentBid}
          currentAsk={currentAsk}
          symbol={symbol}
          onTrade={handleTrade}
          loading={loading}
        />
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', height: '100%', gap: 14 }}>
          {/* 紧急六项操作按钮 */}
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, 1fr)', gap: 12 }}>
            <Popconfirm
              title="确认一键锁仓？"
              description="将以当前市价开立等量反向对冲单，锁死当前净值与敞口。"
              onConfirm={() => handleConfirm('lock')}
              okText="立即锁仓"
              cancelText="取消"
              okButtonProps={{ danger: true }}
            >
              <Button
                type="primary"
                danger
                icon={<LockOutlined />}
                size="large"
                block
                loading={loading}
                style={{ fontWeight: 'bold' }}
              >
                一键锁仓
              </Button>
            </Popconfirm>

            <Popconfirm
              title="确认全平所有持仓？"
              description="将立即市价平掉多空所有订单！"
              onConfirm={() => handleConfirm('close_all')}
              okText="确认平仓"
              cancelText="取消"
              okButtonProps={{ danger: true }}
            >
              <Button
                danger
                icon={<StopOutlined />}
                size="large"
                block
                loading={loading}
                style={{ borderColor: '#faad14', color: '#faad14', fontWeight: 'bold' }}
              >
                一键平仓
              </Button>
            </Popconfirm>

            <Popconfirm
              title="确认一键平盈？"
              description="仅平掉当前浮动盈利的单子，保留浮亏订单。"
              onConfirm={() => handleConfirm('close_profit')}
              okText="平盈"
              cancelText="取消"
            >
              <Button
                icon={<CheckCircleOutlined />}
                size="large"
                block
                loading={loading}
                style={{ borderColor: '#52c41a', color: '#52c41a' }}
              >
                一键平盈
              </Button>
            </Popconfirm>

            <Popconfirm
              title="确认一键平亏？"
              description="仅平掉当前处于亏损状态的订单。"
              onConfirm={() => handleConfirm('close_loss')}
              okText="平亏"
              cancelText="取消"
              okButtonProps={{ danger: true }}
            >
              <Button
                danger
                icon={<CloseCircleOutlined />}
                size="large"
                block
                loading={loading}
              >
                一键平亏
              </Button>
            </Popconfirm>

            <Popconfirm
              title="确认平掉所有多单？"
              description="将清空当前多头网格所有持仓订单。"
              onConfirm={() => handleConfirm('close_buy')}
              okText="平多"
              cancelText="取消"
            >
              <Button
                icon={<ArrowUpOutlined />}
                size="large"
                block
                loading={loading}
                style={{ borderColor: '#1890ff', color: '#1890ff' }}
              >
                一键平多
              </Button>
            </Popconfirm>

            <Popconfirm
              title="确认平掉所有空单？"
              description="将清空当前空头网格所有持仓订单。"
              onConfirm={() => handleConfirm('close_sell')}
              okText="平空"
              cancelText="取消"
            >
              <Button
                icon={<ArrowDownOutlined />}
                size="large"
                block
                loading={loading}
                style={{ borderColor: '#fa8c16', color: '#fa8c16' }}
              >
                一键平空
              </Button>
            </Popconfirm>
          </div>

          {/* 策略生命周期控制 */}
          <div>
            {!isRunning ? (
              <Popconfirm
                title="确认启动实盘交易策略？"
                description="启动后将解密交易密钥，连接 Exness 交易通道并自动执行开仓！"
                onConfirm={() => handleConfirm('start')}
                okText="立即启动"
                cancelText="取消"
              >
                <Button
                  type="primary"
                  icon={<PlayCircleOutlined />}
                  size="large"
                  block
                  loading={loading}
                  style={{
                    background: '#52c41a',
                    borderColor: '#52c41a',
                    color: '#ffffff',
                    fontWeight: 'bold',
                    height: 44,
                  }}
                >
                  🚀 启动实盘策略 (START)
                </Button>
              </Popconfirm>
            ) : (
              <div style={{ display: 'flex', gap: 8 }}>
                <Popconfirm
                  title={isPaused ? '恢复自动开仓与加仓？' : '暂停自动开仓与加仓？'}
                  description={
                    isPaused
                      ? '恢复后策略将按信号与网格间距正常开单。'
                      : '暂停后现有持仓继续跟踪止盈，但不再开首单或加仓。'
                  }
                  onConfirm={() => handleConfirm(isPaused ? 'resume' : 'pause')}
                  okText="确定"
                  cancelText="取消"
                >
                  <Button
                    type={isPaused ? 'primary' : 'default'}
                    icon={isPaused ? <PlayCircleOutlined /> : <PauseCircleOutlined />}
                    size="large"
                    style={{
                      flex: 1,
                      borderColor: isPaused ? '#52c41a' : '#faad14',
                      color: isPaused ? '#ffffff' : '#faad14',
                      background: isPaused ? '#52c41a' : undefined,
                      fontWeight: 'bold',
                    }}
                    loading={loading}
                  >
                    {isPaused ? '▶ 恢复开仓' : '⏸ 暂停开仓'}
                  </Button>
                </Popconfirm>

                <Popconfirm
                  title="确认停止策略？"
                  description="将停止后台策略 Runner 进程，不再接收行情开仓！"
                  onConfirm={() => handleConfirm('stop')}
                  okText="停止"
                  cancelText="取消"
                  okButtonProps={{ danger: true }}
                >
                  <Button
                    danger
                    icon={<StopOutlined />}
                    size="large"
                    loading={loading}
                    style={{ fontWeight: 'bold' }}
                  >
                    停止
                  </Button>
                </Popconfirm>
              </div>
            )}
          </div>
        </div>
      )}
    </Card>
  );
};
