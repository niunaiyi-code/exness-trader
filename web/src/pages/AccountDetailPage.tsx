import React, { useCallback, useEffect, useState } from 'react';
import {
  Card,
  Button,
  Row,
  Col,
  Statistic,
  Popconfirm,
  message,
  Switch,
  Descriptions,
  Alert,
  Space,
} from 'antd';
import {
  DollarOutlined,
  ShoppingOutlined,
  StopOutlined,
  LockOutlined,
  UnlockOutlined,
} from '@ant-design/icons';
import { useParams } from 'react-router-dom';
import { api, apiErrorMessage } from '../lib/api';

interface AccountDetail {
  id: string;
  account_id: string;
  gateway_url: string;
  state: string;
  strategy_id?: string;
  strategy_name?: string;
  params?: {
    initial_lot: string;
    grid_step: number;
    grid_step_add: number;
    grid_step_sec: number;
    lot_mult: string;
    tp_distance_usd: string;
    lock_trigger_usd: string;
    params_locked: boolean;
  };
}

const AccountDetailPage: React.FC = () => {
  const { id } = useParams<{ id: string }>();
  const [account, setAccount] = useState<AccountDetail | null>(null);
  const [loading, setLoading] = useState(false);
  const [closing, setClosing] = useState(false);
  const [togglingLock, setTogglingLock] = useState(false);

  const fetchAccount = useCallback(async () => {
    setLoading(true);
    try {
      const response = await api.get(`/accounts/${id}`);
      setAccount(response.data);
    } catch {
      message.error('获取账户详情失败');
    } finally {
      setLoading(false);
    }
  }, [id]);

  useEffect(() => {
    if (id) {
      fetchAccount();
    }
  }, [id, fetchAccount]);

  // 切换锁定状态
  const handleToggleLock = async (checked: boolean) => {
    setTogglingLock(true);
    try {
      await api.post(`/accounts/${id}/params/lock`, { locked: checked });
      message.success(`已${checked ? '锁定' : '开放'}该账户策略参数`);
      fetchAccount();
    } catch (error) {
      message.error(apiErrorMessage(error, '切换锁定状态失败'));
    } finally {
      setTogglingLock(false);
    }
  };

  // 紧急平仓：调用与驾驶舱一致的 close_all 动作
  const handleEmergencyClose = async () => {
    setClosing(true);
    try {
      await api.post(`/accounts/${id}/actions`, { action: 'close_all' });
      message.success('已提交全部平仓指令');
    } catch (error) {
      message.error(apiErrorMessage(error, '平仓指令失败'));
    } finally {
      setClosing(false);
    }
  };

  if (!account) {
    return <Card loading={loading} />;
  }

  const isLocked = account.params?.params_locked ?? true;

  return (
    <div>
      <Row gutter={16} style={{ marginBottom: 24 }}>
        <Col xs={24} sm={12} lg={8}>
          <Card>
            <Statistic
              title="账户 ID"
              value={account.account_id}
              prefix={<DollarOutlined />}
            />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={8}>
          <Card>
            <Statistic
              title="运行状态"
              value={account.state}
              prefix={<ShoppingOutlined />}
            />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={8}>
          <Card>
            <Popconfirm
              title="确认紧急平掉该账户全部持仓？"
              description="将立即以市价平掉多空所有订单！"
              onConfirm={handleEmergencyClose}
              okText="立即平仓"
              cancelText="取消"
              okButtonProps={{ danger: true }}
            >
              <Button
                type="primary"
                danger
                block
                icon={<StopOutlined />}
                loading={closing}
              >
                紧急平仓
              </Button>
            </Popconfirm>
          </Card>
        </Col>
      </Row>

      <Card
        title="策略与权限管控"
        extra={
          <Space>
            <span style={{ fontSize: 13, color: '#8c8c8c' }}>
              {isLocked ? <LockOutlined style={{ color: '#ff4d4f' }} /> : <UnlockOutlined style={{ color: '#52c41a' }} />} 防篡改参数锁定:
            </span>
            <Switch
              checked={isLocked}
              loading={togglingLock}
              onChange={handleToggleLock}
              checkedChildren="已锁定"
              unCheckedChildren="已开放"
            />
          </Space>
        }
      >
        <div style={{ marginBottom: 16 }}>
          <Alert
            type={isLocked ? 'warning' : 'success'}
            showIcon
            message={
              isLocked
                ? '防篡改锁定中：该交易员在驾驶舱内只能查看参数，禁止自行微调。'
                : '调参权限已开放：该交易员可在其交易员驾驶舱内自主微调实盘策略参数。'
            }
          />
        </div>

        <Descriptions bordered size="small" column={{ xs: 1, sm: 2, md: 4 }}>
          <Descriptions.Item label="当前绑定策略" span={2}>
            <b>{account.strategy_name || '未关联策略'}</b>
          </Descriptions.Item>
          <Descriptions.Item label="权限状态" span={2}>
            {isLocked ? (
              <span style={{ color: '#ff4d4f', fontWeight: 'bold' }}>🔒 严格锁定（只读）</span>
            ) : (
              <span style={{ color: '#52c41a', fontWeight: 'bold' }}>🔓 授权调参（可编辑）</span>
            )}
          </Descriptions.Item>
          <Descriptions.Item label="初始首单手数">{account.params?.initial_lot || '未设定'}</Descriptions.Item>
          <Descriptions.Item label="基础加仓点距">{account.params?.grid_step ? `${account.params.grid_step} 点` : '未设定'}</Descriptions.Item>
          <Descriptions.Item label="阶梯递增点距">{account.params?.grid_step_add ? `${account.params.grid_step_add} 点` : '未设定'}</Descriptions.Item>
          <Descriptions.Item label="去抖冷却间隔">{account.params?.grid_step_sec ? `${account.params.grid_step_sec} 秒` : '未设定'}</Descriptions.Item>
          <Descriptions.Item label="加仓倍增系数">{account.params?.lot_mult ? `${account.params.lot_mult}x` : '未设定'}</Descriptions.Item>
          <Descriptions.Item label="单层止盈距离">{account.params?.tp_distance_usd ? `$${account.params.tp_distance_usd}` : '未设定'}</Descriptions.Item>
          <Descriptions.Item label="对冲锁仓阈值" span={2}>{account.params?.lock_trigger_usd ? `$${account.params.lock_trigger_usd}` : '未设定'}</Descriptions.Item>
        </Descriptions>
      </Card>
    </div>
  );
};

export default AccountDetailPage;
